use std::cell::RefCell;

use crate::error::{ParseError, qjson_err};
use crate::skip_cache::SkipCache;

pub struct Document<'a> {
    pub(crate) buf:     &'a [u8],
    pub(crate) indices: Vec<u32>,
    pub(crate) eager_validated: bool,
    pub(crate) scratch: RefCell<Vec<u8>>,
    pub(crate) skip:    RefCell<SkipCache>,
}

impl<'a> Document<'a> {
    pub fn parse(buf: &'a [u8]) -> Result<Self, qjson_err> {
        Self::parse_with_options(buf, &crate::options::Options::default())
    }

    pub fn parse_with_options(
        buf: &'a [u8],
        opts: &crate::options::Options,
    ) -> Result<Self, qjson_err> {
        Self::parse_with_options_detailed(buf, opts).map_err(|err| err.code)
    }

    pub(crate) fn parse_with_options_detailed(
        buf: &'a [u8],
        opts: &crate::options::Options,
    ) -> Result<Self, ParseError> {
        // RFC 8259 §2: "A JSON text is a serialized value."
        // Empty input and whitespace-only input contain no value.
        if buf.iter().all(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
            return Err(ParseError::no_offset(qjson_err::QJSON_PARSE_ERROR));
        }

        let max_depth = opts.effective_max_depth();
        let mut indices = Vec::new();
        crate::scan::scan(buf, &mut indices)
            .map_err(|offset| ParseError::new(qjson_err::QJSON_PARSE_ERROR, offset))?;
        indices.push(u32::MAX);

        if opts.is_eager() {
            crate::validate::validate_trailing_with_offset(buf, &indices)?;
            crate::validate::validate_eager_values_with_offset(buf, &indices, max_depth)?;
        } else {
            crate::validate::validate_depth_with_offset(buf, &indices, max_depth)?;
        }

        Ok(Self {
            buf,
            indices,
            eager_validated: opts.is_eager(),
            scratch: RefCell::new(Vec::new()),
            skip:    RefCell::new(SkipCache::new()),
        })
    }
}

use crate::cursor::{Cursor, find_value_span};
use crate::error::qjson_type;

impl<'a> Document<'a> {
    pub(crate) fn is_root_scalar_cursor(&self, cur: Cursor) -> bool {
        cur.idx_start == 0 && self.indices.len() == 1 && self.indices[0] == u32::MAX
    }

    pub(crate) fn root_scalar_start(&self) -> usize {
        let mut p = 0;
        while p < self.buf.len() && matches!(self.buf[p], b' '|b'\t'|b'\n'|b'\r') {
            p += 1;
        }
        p
    }

    /// Inspect a cursor and return its JSON value type.
    pub(crate) fn type_of(&self, cur: Cursor) -> Result<qjson_type, qjson_err> {
        if self.is_root_scalar_cursor(cur) {
            return self.scalar_type_at(self.root_scalar_start());
        }

        let pos = *self.indices.get(cur.idx_start as usize)
            .ok_or(qjson_err::QJSON_PARSE_ERROR)? as usize;
        let lead = self.buf.get(pos).copied().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
        match lead {
            b'"' => Ok(qjson_type::QJSON_T_STR),
            b'{' => Ok(qjson_type::QJSON_T_OBJ),
            b'[' => Ok(qjson_type::QJSON_T_ARR),
            _ => {
                // For a scalar value the cursor's idx_start points at the
                // structural char AFTER the scalar; the scalar's first byte
                // lives between the previous structural char and this one.
                let scalar_start = self.find_scalar_start(cur.idx_start)?;
                self.scalar_type_at(scalar_start)
            }
        }
    }

    fn scalar_type_at(&self, scalar_start: usize) -> Result<qjson_type, qjson_err> {
        match self.buf.get(scalar_start).copied() {
            Some(b't') | Some(b'f') => Ok(qjson_type::QJSON_T_BOOL),
            Some(b'n')              => Ok(qjson_type::QJSON_T_NULL),
            Some(b'-') | Some(b'0'..=b'9') => Ok(qjson_type::QJSON_T_NUM),
            _ => Err(qjson_err::QJSON_PARSE_ERROR),
        }
    }

    /// Find the byte position of the first non-whitespace byte after the
    /// structural character at `indices[idx - 1]`. Used to locate the first
    /// byte of a scalar value.
    pub(crate) fn find_scalar_start(&self, idx: u32) -> Result<usize, qjson_err> {
        if idx == 0 { return Err(qjson_err::QJSON_PARSE_ERROR); }
        let prev = self.indices[(idx - 1) as usize] as usize;
        let mut p = prev + 1;
        while p < self.buf.len() && matches!(self.buf[p], b' '|b'\t'|b'\n'|b'\r') {
            p += 1;
        }
        Ok(p)
    }

    /// Find the i-th key/value entry of an object cursor. Returns the
    /// indices position of the key (so the caller can decode it via the
    /// existing string-decode path) and the value's `Cursor`.
    ///
    /// Returns `QJSON_TYPE_MISMATCH` for non-object cursors, `QJSON_NOT_FOUND`
    /// when `i` is past the end.
    pub(crate) fn nth_object_entry(&self, cur: Cursor, n: usize) -> Result<(u32, Cursor), qjson_err> {
        if self.is_root_scalar_cursor(cur) {
            return Err(qjson_err::QJSON_TYPE_MISMATCH);
        }

        let pos = self.indices[cur.idx_start as usize] as usize;
        let b = *self.buf.get(pos).ok_or(qjson_err::QJSON_PARSE_ERROR)?;
        if b != b'{' {
            return Err(qjson_err::QJSON_TYPE_MISMATCH);
        }
        // Mirror cursor_len's walk, but stop at the n-th child rather than counting.
        let closer_pos = self.indices[cur.idx_end as usize] as usize;
        let mut p = pos + 1;
        while p < closer_pos && matches!(self.buf[p], b' '|b'\t'|b'\n'|b'\r') {
            p += 1;
        }
        if p == closer_pos {
            return Err(qjson_err::QJSON_NOT_FOUND);
        }
        let mut i = cur.idx_start + 1;
        let end = cur.idx_end;
        let mut count: usize = 0;
        loop {
            // For objects, the key occupies indices[i..=i+1] (open & close quote);
            // the value cursor starts at i+3 (after the colon at i+2).
            let key_idx_start = i;
            let value_idx_start = i + 3;
            let (cursor_end, skip_end) = crate::cursor::find_value_span(self, value_idx_start)?;
            if count == n {
                return Ok((key_idx_start, Cursor { idx_start: value_idx_start, idx_end: cursor_end }));
            }
            count += 1;
            let after_pos = self.indices[skip_end as usize] as usize;
            if after_pos >= self.buf.len() { return Err(qjson_err::QJSON_PARSE_ERROR); }
            match self.buf[after_pos] {
                b',' => {
                    i = skip_end + 1;
                    if i > end { return Err(qjson_err::QJSON_NOT_FOUND); }
                }
                b'}' => return Err(qjson_err::QJSON_NOT_FOUND),
                _ => return Err(qjson_err::QJSON_PARSE_ERROR),
            }
        }
    }

    /// Count direct children of the container at `cur`.
    /// Returns QJSON_TYPE_MISMATCH for non-container cursors.
    pub(crate) fn cursor_len(&self, cur: Cursor) -> Result<usize, qjson_err> {
        if self.is_root_scalar_cursor(cur) {
            return Err(qjson_err::QJSON_TYPE_MISMATCH);
        }

        let pos = self.indices[cur.idx_start as usize] as usize;
        let b = *self.buf.get(pos).ok_or(qjson_err::QJSON_PARSE_ERROR)?;
        if b != b'{' && b != b'[' {
            return Err(qjson_err::QJSON_TYPE_MISMATCH);
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
            if after_pos >= self.buf.len() { return Err(qjson_err::QJSON_PARSE_ERROR); }
            match self.buf[after_pos] {
                b',' => {
                    i = skip_end + 1;
                    if i > end { return Err(qjson_err::QJSON_PARSE_ERROR); }
                }
                b'}' | b']' => break,
                _ => return Err(qjson_err::QJSON_PARSE_ERROR),
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
        let doc = Document::parse(b"{\"a\":1}").unwrap();
        assert!(doc.indices.len() >= 5);
        assert_eq!(*doc.indices.last().unwrap(), u32::MAX);
    }

    #[test]
    fn parse_error_on_malformed() {
        assert!(Document::parse(b"{").is_err());
    }

    #[test]
    fn parse_with_options_defaults_match_parse() {
        let opts = crate::options::Options::default();
        let a = Document::parse(b"{\"a\":1}").unwrap();
        let b = Document::parse_with_options(b"{\"a\":1}", &opts).unwrap();
        assert_eq!(a.indices, b.indices);
    }

    #[test]
    fn parse_with_lazy_skips_eager_validation() {
        // Trailing content is an eager-only check; lazy must accept it.
        let opts = crate::options::Options { mode: crate::options::QJSON_MODE_LAZY, max_depth: 0 };
        assert!(Document::parse_with_options(b"{}garbage", &opts).is_ok());
    }
}
