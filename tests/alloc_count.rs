//! Allocation-count regression test for the pooled decoder API.
//!
//! Installs a counting `GlobalAlloc` and runs both APIs across N parses,
//! asserting:
//!   1. The legacy `qjd_parse` path allocates at least once per call.
//!   2. The pooled `qjd_decoder_parse` path allocates strictly less than
//!      the legacy path — the indices / scratch / skip-cache Vecs and the
//!      decoder Box are no longer rebuilt per parse.
//!
//! There are unavoidable per-parse allocations the pooled path still
//! incurs (the `Box<qjd_doc>` handle, the bracket-balance check's
//! `Vec::with_capacity(32)` stack in `validate_brackets`). They are small
//! and fixed-size, so the assertion uses a ratio rather than a hard
//! ceiling: pooled must be at most half of legacy.
//!
//! Gated behind the `count-allocs` Cargo feature because swapping the
//! global allocator is process-wide and would interfere with other tests.

#![cfg(feature = "count-allocs")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::os::raw::c_int;
use std::sync::atomic::{AtomicUsize, Ordering};

use quickdecode::ffi::*;

struct CountingAlloc {
    inner: System,
    count: AtomicUsize,
}

impl CountingAlloc {
    const fn new() -> Self {
        Self { inner: System, count: AtomicUsize::new(0) }
    }
    fn count(&self) -> usize { self.count.load(Ordering::Relaxed) }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.inner.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

const PAYLOAD: &[u8] = include_bytes!("../benches/fixtures/medium_resp.json");
const ITERS: usize = 1_000;

fn count_legacy() -> usize {
    unsafe {
        // Warmup allocator caches.
        for _ in 0..5 {
            let mut err: c_int = -1;
            let doc = qjd_parse(PAYLOAD.as_ptr(), PAYLOAD.len(), &mut err);
            qjd_free(doc);
        }
        let baseline = ALLOC.count();
        for _ in 0..ITERS {
            let mut err: c_int = -1;
            let doc = qjd_parse(PAYLOAD.as_ptr(), PAYLOAD.len(), &mut err);
            assert!(!doc.is_null());
            qjd_free(doc);
        }
        ALLOC.count() - baseline
    }
}

fn count_pooled() -> usize {
    unsafe {
        let dec = qjd_decoder_new();
        assert!(!dec.is_null());
        // Warmup: let Vec capacities reach steady state.
        for _ in 0..5 {
            let mut err: c_int = -1;
            let doc = qjd_decoder_parse(dec, PAYLOAD.as_ptr(), PAYLOAD.len(), &mut err);
            assert!(!doc.is_null());
            qjd_free(doc);
        }
        let baseline = ALLOC.count();
        for _ in 0..ITERS {
            let mut err: c_int = -1;
            let doc = qjd_decoder_parse(dec, PAYLOAD.as_ptr(), PAYLOAD.len(), &mut err);
            assert!(!doc.is_null());
            qjd_free(doc);
        }
        let delta = ALLOC.count() - baseline;
        qjd_decoder_free(dec);
        delta
    }
}

#[test]
fn legacy_path_allocates_per_parse() {
    let delta = count_legacy();
    assert!(
        delta >= ITERS,
        "legacy path allocated only {} times across {} iterations (expected >= {})",
        delta, ITERS, ITERS
    );
}

#[test]
fn pooled_path_uses_fewer_allocations_than_legacy() {
    let legacy = count_legacy();
    let pooled = count_pooled();

    // Pooled should be at most half of legacy. Looser than "near zero" but
    // robust to small per-iter allocations we cannot avoid (Box<qjd_doc>,
    // validate_brackets stack) without a bigger refactor.
    let ceiling = legacy / 2;
    assert!(
        pooled < ceiling,
        "pooled={} legacy={} ceiling={} (pooled must be < legacy/2)",
        pooled, legacy, ceiling
    );

    // Emit the absolute numbers so CI logs make regressions visible.
    eprintln!("alloc_count: legacy={} pooled={} (across {} iters)", legacy, pooled, ITERS);
}
