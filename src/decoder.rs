use std::cell::RefCell;

use crate::error::qjd_err;
use crate::skip_cache::SkipCache;

/// Lifecycle state of a [`Decoder`].
///
/// `Ready`     — freshly constructed or just reset; no document parsed.
/// `Parsed`    — last parse() succeeded; indices/scratch reflect `buf`.
/// `Destroyed` — destroy() has been called; only free() is valid.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DecoderState {
    Ready,
    Parsed,
    Destroyed,
}

/// Reusable JSON decoder. Owns the structural-offset buffer, the lazy-decode
/// scratch buffer, and the Phase-2 skip cache, all of which are reused across
/// successive parses to avoid per-call allocation traffic.
///
/// `buf` is stored with a `'static` lifetime: the caller (FFI / test helper)
/// is responsible for keeping the underlying bytes alive until the next
/// `parse()`, `reset()`, `destroy()`, or drop of the decoder.
pub struct Decoder {
    pub(crate) indices: Vec<u32>,
    pub(crate) scratch: RefCell<Vec<u8>>,
    pub(crate) skip:    RefCell<SkipCache>,
    /// Active input buffer. Empty slice (`&[]`) when state ≠ Parsed.
    pub(crate) buf:     &'static [u8],
    /// Bumped on every parse(), reset(), and destroy() so prior docs/cursors
    /// can detect that they reference stale state.
    pub(crate) gen:     u32,
    pub(crate) state:   DecoderState,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            indices: Vec::new(),
            scratch: RefCell::new(Vec::new()),
            skip:    RefCell::new(SkipCache::new()),
            buf:     &[],
            gen:     0,
            state:   DecoderState::Ready,
        }
    }

    /// Parse `input` into this decoder, replacing any previous parse.
    ///
    /// On success, `state` becomes `Parsed` and `gen` advances by one.
    /// On parse error, `state` is `Ready`, `gen` has still advanced (so all
    /// prior docs/cursors are stale), and the buffers are conceptually empty
    /// (their capacity is retained for reuse).
    ///
    /// The decoder borrows `input` for the duration of its `Parsed` state.
    /// The caller must ensure `input` is not freed before the next `parse()`,
    /// `reset()`, `destroy()`, or drop.
    pub fn parse(&mut self, input: &[u8]) -> Result<(), qjd_err> {
        if matches!(self.state, DecoderState::Destroyed) {
            return Err(qjd_err::QJD_INVALID_ARG);
        }
        self.gen = self.gen.wrapping_add(1);
        self.indices.truncate(0);
        self.scratch.borrow_mut().truncate(0);
        self.skip.borrow_mut().clear();
        self.buf = &[];
        self.state = DecoderState::Ready;

        crate::scan::scan(input, &mut self.indices)
            .map_err(|_| qjd_err::QJD_PARSE_ERROR)?;
        self.indices.push(u32::MAX);
        // SAFETY: caller upholds the lifetime contract above.
        self.buf = unsafe { std::mem::transmute::<&[u8], &'static [u8]>(input) };
        self.state = DecoderState::Parsed;
        Ok(())
    }

    /// Drop all cached state and release allocated capacity back to the
    /// allocator. After `reset()`, the decoder is reusable.
    ///
    /// Bumps `gen` so any outstanding docs/cursors become stale.
    pub fn reset(&mut self) {
        if matches!(self.state, DecoderState::Destroyed) { return; }
        self.gen = self.gen.wrapping_add(1);
        self.indices = Vec::new();
        *self.scratch.borrow_mut() = Vec::new();
        self.skip.borrow_mut().clear_and_shrink();
        self.buf = &[];
        self.state = DecoderState::Ready;
    }

    /// Permanently retire this decoder. Frees the bulk buffers immediately;
    /// the [`Decoder`] struct itself is reclaimed when its owning Box is
    /// dropped (typically via the FFI free function).
    pub fn destroy(&mut self) {
        if matches!(self.state, DecoderState::Destroyed) { return; }
        self.gen = self.gen.wrapping_add(1);
        let _ = std::mem::take(&mut self.indices);
        let _ = std::mem::take(&mut *self.scratch.borrow_mut());
        self.skip.borrow_mut().clear_and_shrink();
        self.buf = &[];
        self.state = DecoderState::Destroyed;
    }

    /// Convenience for tests and the legacy `qjd_parse` path: construct a
    /// decoder and parse `input` in one call.
    pub fn parse_oneshot(input: &[u8]) -> Result<Self, qjd_err> {
        let mut d = Self::new();
        d.parse(input)?;
        Ok(d)
    }
}

impl Default for Decoder {
    fn default() -> Self { Self::new() }
}

use crate::cursor::{Cursor, find_value_span};
use crate::error::qjd_type;

impl Decoder {
    /// Inspect a cursor and return its JSON value type.
    pub(crate) fn type_of(&self, cur: Cursor) -> Result<qjd_type, qjd_err> {
        let pos = *self.indices.get(cur.idx_start as usize)
            .ok_or(qjd_err::QJD_PARSE_ERROR)? as usize;
        let lead = self.buf.get(pos).copied().ok_or(qjd_err::QJD_PARSE_ERROR)?;
        match lead {
            b'"' => Ok(qjd_type::QJD_T_STR),
            b'{' => Ok(qjd_type::QJD_T_OBJ),
            b'[' => Ok(qjd_type::QJD_T_ARR),
            _ => {
                // For a scalar value the cursor's idx_start points at the
                // structural char AFTER the scalar; the scalar's first byte
                // lives between the previous structural char and this one.
                let scalar_start = self.find_scalar_start(cur.idx_start)?;
                match self.buf.get(scalar_start).copied() {
                    Some(b't') | Some(b'f') => Ok(qjd_type::QJD_T_BOOL),
                    Some(b'n')              => Ok(qjd_type::QJD_T_NULL),
                    Some(b'-') | Some(b'0'..=b'9') => Ok(qjd_type::QJD_T_NUM),
                    _ => Err(qjd_err::QJD_PARSE_ERROR),
                }
            }
        }
    }

    /// Find the byte position of the first non-whitespace byte after the
    /// structural character at `indices[idx - 1]`. Used to locate the first
    /// byte of a scalar value.
    pub(crate) fn find_scalar_start(&self, idx: u32) -> Result<usize, qjd_err> {
        if idx == 0 { return Err(qjd_err::QJD_PARSE_ERROR); }
        let prev = self.indices[(idx - 1) as usize] as usize;
        let mut p = prev + 1;
        while p < self.buf.len() && matches!(self.buf[p], b' '|b'\t'|b'\n'|b'\r') {
            p += 1;
        }
        Ok(p)
    }

    /// Count direct children of the container at `cur`.
    /// Returns QJD_TYPE_MISMATCH for non-container cursors.
    pub(crate) fn cursor_len(&self, cur: Cursor) -> Result<usize, qjd_err> {
        let pos = self.indices[cur.idx_start as usize] as usize;
        let b = *self.buf.get(pos).ok_or(qjd_err::QJD_PARSE_ERROR)?;
        if b != b'{' && b != b'[' {
            return Err(qjd_err::QJD_TYPE_MISMATCH);
        }
        let is_obj = b == b'{';
        // Empty container detection: byte after opener (skipping whitespace)
        // is the closer position itself, meaning no value sits between them.
        let closer_pos = self.indices[cur.idx_end as usize] as usize;
        let mut p = pos + 1;
        while p < closer_pos && matches!(self.buf[p], b' '|b'\t'|b'\n'|b'\r') {
            p += 1;
        }
        if p == closer_pos {
            return Ok(0);
        }
        let mut count: usize = 0;
        let mut i = cur.idx_start + 1;
        let end = cur.idx_end;
        loop {
            count += 1;
            let value_idx_start = if is_obj { i + 3 } else { i };
            let (_cursor_end, skip_end) = find_value_span(self, value_idx_start)?;
            let after_pos = self.indices[skip_end as usize] as usize;
            if after_pos >= self.buf.len() { return Err(qjd_err::QJD_PARSE_ERROR); }
            match self.buf[after_pos] {
                b',' => {
                    i = skip_end + 1;
                    if i > end { return Err(qjd_err::QJD_PARSE_ERROR); }
                }
                b'}' | b']' => break,
                _ => return Err(qjd_err::QJD_PARSE_ERROR),
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_object() {
        let d = Decoder::parse_oneshot(b"{\"a\":1}").unwrap();
        assert!(d.indices.len() >= 5);
        assert_eq!(*d.indices.last().unwrap(), u32::MAX);
        assert_eq!(d.state, DecoderState::Parsed);
        assert_eq!(d.gen, 1);
    }

    #[test]
    fn parse_error_on_malformed() {
        assert!(Decoder::parse_oneshot(b"{").is_err());
    }

    #[test]
    fn parse_then_parse_bumps_gen() {
        let mut d = Decoder::new();
        d.parse(b"{\"a\":1}").unwrap();
        let g1 = d.gen;
        d.parse(b"[1,2,3]").unwrap();
        assert_eq!(d.gen, g1.wrapping_add(1));
    }

    #[test]
    fn parse_error_returns_to_ready_and_bumps_gen() {
        let mut d = Decoder::new();
        d.parse(b"{\"a\":1}").unwrap();
        let g1 = d.gen;
        assert_eq!(d.state, DecoderState::Parsed);

        assert!(d.parse(b"{").is_err());
        assert_eq!(d.state, DecoderState::Ready);
        assert_eq!(d.gen, g1.wrapping_add(1));
        assert!(d.buf.is_empty());
    }

    #[test]
    fn reset_shrinks_capacity() {
        let mut d = Decoder::new();
        let payload: Vec<u8> = {
            let mut s = Vec::from(&b"{\"k\":["[..]);
            for i in 0..100 { if i > 0 { s.push(b','); } s.extend_from_slice(b"1"); }
            s.extend_from_slice(b"]}");
            s
        };
        d.parse(&payload).unwrap();
        assert!(d.indices.capacity() > 0);

        d.reset();
        assert_eq!(d.state, DecoderState::Ready);
        assert_eq!(d.indices.capacity(), 0);
        assert!(d.buf.is_empty());
    }

    #[test]
    fn destroy_sets_terminal_state() {
        let mut d = Decoder::new();
        d.parse(b"{\"a\":1}").unwrap();
        d.destroy();
        assert_eq!(d.state, DecoderState::Destroyed);

        assert_eq!(d.parse(b"{\"b\":2}").unwrap_err(), qjd_err::QJD_INVALID_ARG);
        d.reset();
        assert_eq!(d.state, DecoderState::Destroyed);
    }

    #[test]
    fn destroy_is_idempotent() {
        let mut d = Decoder::new();
        d.parse(b"{\"a\":1}").unwrap();
        let g = d.gen;
        d.destroy();
        let g_after = d.gen;
        d.destroy();
        assert_eq!(d.gen, g_after, "second destroy must not bump gen");
        assert_ne!(g_after, g);
    }

    #[test]
    fn gen_wrap_does_not_panic() {
        let mut d = Decoder::new();
        d.gen = u32::MAX - 1;
        d.parse(b"{\"a\":1}").unwrap();   // gen = MAX
        d.parse(b"{\"b\":2}").unwrap();   // wraps to 0
        assert_eq!(d.gen, 0);
    }
}
