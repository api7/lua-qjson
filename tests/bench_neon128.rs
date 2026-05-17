//! Micro-benchmark: vmaxvq_u8 fast probe early-exit for NEON scanner.
//!
//! Run: cargo test --release --test bench_neon128 -- --nocapture --ignored

#![cfg(target_arch = "aarch64")]

use core::arch::aarch64::*;
use std::time::Instant;

fn has_quote_or_backslash(chunk: &[u8]) -> bool {
    chunk.iter().any(|&b| b == b'"' || b == b'\\')
}

fn make_probe_heavy_payload(size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    buf.extend_from_slice(b"{\"data\":\"");
    while buf.len() < size - 2 {
        buf.push(b'A');
    }
    buf.extend_from_slice(b"\"}");
    buf
}

fn make_mixed_payload(size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    buf.push(b'[');
    let mut i = 1;
    while i < size - 1 {
        let remaining = size - 1 - i;
        if remaining < 20 {
            buf.extend(std::iter::repeat_n(b' ', remaining));
            break;
        }
        buf.extend_from_slice(b"{\"k\":\"");
        let str_len = std::cmp::min(100, remaining - 10);
        buf.extend(std::iter::repeat_n(b'x', str_len));
        buf.extend_from_slice(b"\"},");
        i = buf.len();
    }
    if buf.last() == Some(&b',') {
        buf.pop();
    }
    buf.push(b']');
    buf
}

fn make_small_objects_payload(size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    buf.push(b'[');
    while buf.len() < size - 2 {
        buf.extend_from_slice(b"{\"a\":1},");
    }
    if buf.last() == Some(&b',') {
        buf.pop();
    }
    buf.push(b']');
    buf
}

/// Current approach: byte_mask64 using movemask16 (pairwise add chain)
#[inline(always)]
unsafe fn current_probe(
    c0: uint8x16_t,
    c1: uint8x16_t,
    c2: uint8x16_t,
    c3: uint8x16_t,
) -> bool {
    #[inline(always)]
    unsafe fn movemask16(v: uint8x16_t) -> u16 {
        const LANE_BITS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
        let lane_mask = vld1q_u8(LANE_BITS.as_ptr());
        let hi = vshrq_n_s8(vreinterpretq_s8_u8(v), 7);
        let weighted = vandq_u8(vreinterpretq_u8_s8(hi), lane_mask);
        let s16 = vpaddlq_u8(weighted);
        let s32 = vpaddlq_u16(s16);
        let s64 = vpaddlq_u32(s32);
        let lo = vgetq_lane_u64(s64, 0) as u16;
        let hi = vgetq_lane_u64(s64, 1) as u16;
        lo | (hi << 8)
    }

    #[inline(always)]
    unsafe fn byte_mask16(bytes: uint8x16_t, needle: u8) -> u16 {
        movemask16(vceqq_u8(bytes, vdupq_n_u8(needle)))
    }

    #[inline(always)]
    unsafe fn byte_mask64(
        c0: uint8x16_t,
        c1: uint8x16_t,
        c2: uint8x16_t,
        c3: uint8x16_t,
        needle: u8,
    ) -> u64 {
        (byte_mask16(c0, needle) as u64)
            | ((byte_mask16(c1, needle) as u64) << 16)
            | ((byte_mask16(c2, needle) as u64) << 32)
            | ((byte_mask16(c3, needle) as u64) << 48)
    }

    let quote_mask = byte_mask64(c0, c1, c2, c3, b'"');
    let backslash_mask = byte_mask64(c0, c1, c2, c3, b'\\');
    (quote_mask | backslash_mask) != 0
}

/// New approach: vmaxvq_u8 on OR'd comparison results
#[inline(always)]
unsafe fn vmaxvq_probe(
    c0: uint8x16_t,
    c1: uint8x16_t,
    c2: uint8x16_t,
    c3: uint8x16_t,
) -> bool {
    let quote = vdupq_n_u8(b'"');
    let backslash = vdupq_n_u8(b'\\');

    // Check for quote OR backslash in each register
    let m0 = vorrq_u8(vceqq_u8(c0, quote), vceqq_u8(c0, backslash));
    let m1 = vorrq_u8(vceqq_u8(c1, quote), vceqq_u8(c1, backslash));
    let m2 = vorrq_u8(vceqq_u8(c2, quote), vceqq_u8(c2, backslash));
    let m3 = vorrq_u8(vceqq_u8(c3, quote), vceqq_u8(c3, backslash));

    // OR all together and check max
    let m01 = vorrq_u8(m0, m1);
    let m23 = vorrq_u8(m2, m3);
    let m = vorrq_u8(m01, m23);

    vmaxvq_u8(m) != 0
}

#[test]
#[ignore]
fn bench_realistic_scanner_path() {
    println!("\n=== Realistic Scanner Path Comparison ===\n");
    println!("Simulating the actual in_string fast path in the scanner.\n");
    println!("Current: 2x byte_mask64 (each uses movemask16 with pairwise add chain)");
    println!("New: vmaxvq_u8 on OR'd comparison results (single horizontal max)\n");

    let payload = make_probe_heavy_payload(10 * 1024 * 1024);
    println!("Payload: {} MB (probe-heavy: long base64 string)", payload.len() / 1024 / 1024);

    // Count chunks with quote/backslash
    let chunks: Vec<&[u8]> = payload.chunks_exact(64).collect();
    let has_qb = chunks.iter().filter(|c| has_quote_or_backslash(c)).count();
    println!(
        "Chunks with quote/backslash: {} / {} ({:.2}%)\n",
        has_qb,
        chunks.len(),
        100.0 * has_qb as f64 / chunks.len() as f64
    );

    let iters = 100;

    // Warmup
    for _ in 0..10 {
        let mut i = 0usize;
        unsafe {
            while i + 64 <= payload.len() {
                let c0 = vld1q_u8(payload.as_ptr().add(i));
                let c1 = vld1q_u8(payload.as_ptr().add(i + 16));
                let c2 = vld1q_u8(payload.as_ptr().add(i + 32));
                let c3 = vld1q_u8(payload.as_ptr().add(i + 48));
                std::hint::black_box(current_probe(c0, c1, c2, c3));
                i += 64;
            }
        }
    }

    // Current approach
    let t0 = Instant::now();
    for _ in 0..iters {
        let mut i = 0usize;
        let mut skip_count = 0u64;
        unsafe {
            while i + 64 <= payload.len() {
                let c0 = vld1q_u8(payload.as_ptr().add(i));
                let c1 = vld1q_u8(payload.as_ptr().add(i + 16));
                let c2 = vld1q_u8(payload.as_ptr().add(i + 32));
                let c3 = vld1q_u8(payload.as_ptr().add(i + 48));

                if !current_probe(c0, c1, c2, c3) {
                    skip_count += 1;
                }
                i += 64;
            }
        }
        std::hint::black_box(skip_count);
    }
    let current_ms = t0.elapsed().as_millis();

    // New approach
    let t0 = Instant::now();
    for _ in 0..iters {
        let mut i = 0usize;
        let mut skip_count = 0u64;
        unsafe {
            while i + 64 <= payload.len() {
                let c0 = vld1q_u8(payload.as_ptr().add(i));
                let c1 = vld1q_u8(payload.as_ptr().add(i + 16));
                let c2 = vld1q_u8(payload.as_ptr().add(i + 32));
                let c3 = vld1q_u8(payload.as_ptr().add(i + 48));

                if !vmaxvq_probe(c0, c1, c2, c3) {
                    skip_count += 1;
                }
                i += 64;
            }
        }
        std::hint::black_box(skip_count);
    }
    let vmaxvq_ms = t0.elapsed().as_millis();

    println!("Current (2x byte_mask64):  {} ms ({} iters)", current_ms, iters);
    println!("vmaxvq probe:              {} ms ({} iters)", vmaxvq_ms, iters);

    let speedup = current_ms as f64 / vmaxvq_ms as f64;
    let pct = (1.0 - vmaxvq_ms as f64 / current_ms as f64) * 100.0;
    println!("\nSpeedup: {:.2}x ({:+.1}%)", speedup, pct);

    if speedup > 1.1 {
        println!("\nCONCLUSION: vmaxvq_u8 probe shows significant benefit ({:.0}% faster)", pct);
    } else if speedup > 0.95 {
        println!("\nCONCLUSION: vmaxvq_u8 probe is roughly equivalent (within noise)");
    } else {
        println!("\nCONCLUSION: vmaxvq_u8 probe is slower - not recommended");
    }
}

#[test]
#[ignore]
fn bench_full_scanner_comparison() {
    use quickdecode::__test_api::{NeonScanner, Scanner};

    println!("\n=== Full Scanner Throughput (current implementation) ===\n");

    let scenarios = [
        ("probe-heavy 100KB", make_probe_heavy_payload(100 * 1024)),
        ("probe-heavy 1MB", make_probe_heavy_payload(1024 * 1024)),
        ("mixed 100KB", make_mixed_payload(100 * 1024)),
        ("mixed 1MB", make_mixed_payload(1024 * 1024)),
        ("small objects 100KB", make_small_objects_payload(100 * 1024)),
        ("small objects 1MB", make_small_objects_payload(1024 * 1024)),
    ];

    for (name, payload) in &scenarios {
        let iters = if payload.len() > 500_000 { 500 } else { 2000 };

        // Warmup
        for _ in 0..10 {
            let mut out = Vec::new();
            let _ = NeonScanner::scan(payload, &mut out);
        }

        let t0 = Instant::now();
        for _ in 0..iters {
            let mut out = Vec::new();
            let _ = NeonScanner::scan(payload, &mut out);
            std::hint::black_box(&out);
        }
        let elapsed = t0.elapsed();
        let ns_per_iter = elapsed.as_nanos() as f64 / iters as f64;
        let throughput_gbps = (payload.len() as f64 * iters as f64 * 8.0)
            / elapsed.as_secs_f64()
            / 1_000_000_000.0;

        println!(
            "{:25} {:>10.0} ns/iter  {:.2} Gbps",
            name, ns_per_iter, throughput_gbps
        );
    }
}

#[test]
#[ignore]
fn analyze_chunk_distribution() {
    println!("\n=== Chunk Distribution Analysis ===\n");
    println!("Checking if vmaxvq_u8 < 0x22 early-exit would help.\n");

    let scenarios = [
        ("probe-heavy 1MB", make_probe_heavy_payload(1024 * 1024)),
        ("mixed 1MB", make_mixed_payload(1024 * 1024)),
        ("small objects 1MB", make_small_objects_payload(1024 * 1024)),
    ];

    for (name, payload) in &scenarios {
        println!("--- {} ---", name);

        let chunks: Vec<&[u8]> = payload.chunks_exact(64).collect();
        let mut max_bytes: Vec<u8> = Vec::with_capacity(chunks.len());

        for chunk in &chunks {
            let max = chunk.iter().copied().max().unwrap_or(0);
            max_bytes.push(max);
        }

        let below_threshold = max_bytes.iter().filter(|&&m| m < 0x22).count();
        let at_or_above = max_bytes.len() - below_threshold;

        println!(
            "  Total chunks: {}, max < 0x22: {} ({:.1}%), max >= 0x22: {} ({:.1}%)",
            chunks.len(),
            below_threshold,
            100.0 * below_threshold as f64 / chunks.len() as f64,
            at_or_above,
            100.0 * at_or_above as f64 / chunks.len() as f64
        );

        // Distribution of max bytes
        let mut histogram = [0usize; 256];
        for &m in &max_bytes {
            histogram[m as usize] += 1;
        }

        println!("  Max byte distribution (top 3):");
        let mut sorted: Vec<(u8, usize)> = histogram
            .iter()
            .enumerate()
            .filter(|(_, &c)| c > 0)
            .map(|(b, &c)| (b as u8, c))
            .collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (byte, count) in sorted.iter().take(3) {
            println!(
                "    0x{:02x} '{}': {} ({:.1}%)",
                byte,
                if *byte >= 0x20 && *byte < 0x7f {
                    *byte as char
                } else {
                    '.'
                },
                count,
                100.0 * *count as f64 / chunks.len() as f64
            );
        }
        println!();
    }

    println!("CONCLUSION: vmaxvq_u8 < 0x22 early-exit has ZERO benefit because");
    println!("typical JSON content (letters, digits, base64) all have max >= 0x22.");
}
