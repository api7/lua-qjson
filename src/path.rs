use crate::error::qjson_err;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PathSeg<'a> {
    Key(&'a [u8]),
    Idx(u32),
}

pub(crate) struct PathIter<'a> {
    rest: &'a [u8],
}

impl<'a> PathIter<'a> {
    pub(crate) fn new(path: &'a [u8]) -> Self { Self { rest: path } }
}

impl<'a> Iterator for PathIter<'a> {
    type Item = Result<PathSeg<'a>, qjson_err>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }

        let first = self.rest[0];

        if first == b'[' {
            // Index segment: [digits]
            let close = match self.rest.iter().position(|&c| c == b']') {
                Some(p) => p,
                None => return Some(Err(qjson_err::QJSON_INVALID_PATH)),
            };
            let digits = &self.rest[1..close];
            if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
                return Some(Err(qjson_err::QJSON_INVALID_PATH));
            }
            let mut n: u32 = 0;
            for &c in digits {
                n = match n.checked_mul(10)
                    .and_then(|x| x.checked_add((c - b'0') as u32))
                {
                    Some(v) => v,
                    None => return Some(Err(qjson_err::QJSON_INVALID_PATH)),
                };
            }
            self.rest = &self.rest[close + 1..];
            return Some(Ok(PathSeg::Idx(n)));
        }

        if first == b'.' {
            // Separator before a key. Skip it then require a key.
            self.rest = &self.rest[1..];
            if self.rest.is_empty() {
                return Some(Err(qjson_err::QJSON_INVALID_PATH));
            }
            return self.next();
        }

        // Key segment: read until '.' or '[' or end.
        let end = self.rest.iter()
            .position(|&c| c == b'.' || c == b'[')
            .unwrap_or(self.rest.len());
        if end == 0 {
            return Some(Err(qjson_err::QJSON_INVALID_PATH));
        }
        let key = &self.rest[..end];
        self.rest = &self.rest[end..];
        Some(Ok(PathSeg::Key(key)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(p: &[u8]) -> Result<Vec<PathSeg<'_>>, qjson_err> {
        PathIter::new(p).collect()
    }

    #[test]
    fn empty_path_yields_no_segs() {
        assert_eq!(parse(b""), Ok(vec![]));
    }

    #[test]
    fn single_key() {
        assert_eq!(parse(b"body"), Ok(vec![PathSeg::Key(b"body")]));
    }

    #[test]
    fn dotted_keys() {
        assert_eq!(
            parse(b"body.model"),
            Ok(vec![PathSeg::Key(b"body"), PathSeg::Key(b"model")]),
        );
    }

    #[test]
    fn array_index_after_key() {
        assert_eq!(
            parse(b"messages[0]"),
            Ok(vec![PathSeg::Key(b"messages"), PathSeg::Idx(0)]),
        );
    }

    #[test]
    fn complex_path() {
        assert_eq!(
            parse(b"body.messages[42].role"),
            Ok(vec![
                PathSeg::Key(b"body"),
                PathSeg::Key(b"messages"),
                PathSeg::Idx(42),
                PathSeg::Key(b"role"),
            ]),
        );
    }

    #[test]
    fn consecutive_indices() {
        assert_eq!(
            parse(b"data[3][1]"),
            Ok(vec![PathSeg::Key(b"data"), PathSeg::Idx(3), PathSeg::Idx(1)]),
        );
    }

    #[test]
    fn leading_index() {
        assert_eq!(parse(b"[5]"), Ok(vec![PathSeg::Idx(5)]));
    }

    #[test]
    fn unterminated_index_is_error() {
        assert_eq!(parse(b"a[3"), Err(qjson_err::QJSON_INVALID_PATH));
    }

    #[test]
    fn non_digit_in_index_is_error() {
        assert_eq!(parse(b"a[abc]"), Err(qjson_err::QJSON_INVALID_PATH));
    }

    #[test]
    fn trailing_dot_is_error() {
        assert_eq!(parse(b"a."), Err(qjson_err::QJSON_INVALID_PATH));
    }
}
