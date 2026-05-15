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
