use std::cell::RefCell;

use crate::error::qjd_err;
use crate::scan::Scanner;
use crate::scan::scalar::ScalarScanner;
use crate::skip_cache::SkipCache;

#[allow(dead_code)]
pub struct Document<'a> {
    pub(crate) buf:     &'a [u8],
    pub(crate) indices: Vec<u32>,
    pub(crate) scratch: Vec<u8>,
    pub(crate) skip:    RefCell<SkipCache>,
}

impl<'a> Document<'a> {
    pub fn parse(buf: &'a [u8]) -> Result<Self, qjd_err> {
        let mut indices = Vec::new();
        ScalarScanner::scan(buf, &mut indices).map_err(|_| qjd_err::QJD_PARSE_ERROR)?;
        // Sentinel simplifies boundary checks during Phase 2.
        indices.push(u32::MAX);
        Ok(Self {
            buf,
            indices,
            scratch: Vec::new(),
            skip: RefCell::new(SkipCache::new()),
        })
    }
}

use crate::cursor::{Cursor, find_value_span};
use crate::error::qjd_type;

impl<'a> Document<'a> {
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
        let doc = Document::parse(b"{\"a\":1}").unwrap();
        assert!(doc.indices.len() >= 5);
        assert_eq!(*doc.indices.last().unwrap(), u32::MAX);
    }

    #[test]
    fn parse_error_on_malformed() {
        assert!(Document::parse(b"{").is_err());
    }
}
