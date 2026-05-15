use crate::doc::Document;
use crate::error::qjd_err;
use crate::path::{PathIter, PathSeg};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cursor {
    /// Position in doc.indices of the opening '{' or '[', or the value's
    /// first-byte structural marker (e.g. opening '"' for a string).
    pub(crate) idx_start: u32,
    /// One past the closing '}' / ']' in doc.indices. For scalar values,
    /// idx_end == idx_start + 1.
    pub(crate) idx_end: u32,
}

impl Cursor {
    pub(crate) fn root(doc: &Document) -> Self {
        // Find the closing index of the outermost container.
        // indices has a u32::MAX sentinel at the end.
        let n = doc.indices.len() as u32;
        debug_assert!(n >= 2);
        Cursor { idx_start: 0, idx_end: n - 2 }
    }

    pub(crate) fn resolve(self, doc: &Document, path: &[u8]) -> Result<Cursor, qjd_err> {
        let mut cur = self;
        for seg in PathIter::new(path) {
            let seg = seg?;
            cur = step(doc, cur, &seg)?;
        }
        Ok(cur)
    }
}

fn step(doc: &Document, cur: Cursor, seg: &PathSeg) -> Result<Cursor, qjd_err> {
    // The cursor must point at a container.
    let opener_byte = container_opener_byte(doc, cur)
        .ok_or(qjd_err::QJD_TYPE_MISMATCH)?;
    match (seg, opener_byte) {
        (PathSeg::Key(_), b'{') | (PathSeg::Idx(_), b'[') => {}
        _ => return Err(qjd_err::QJD_TYPE_MISMATCH),
    }

    walk_children(doc, cur, seg)
}

/// If `cur` points at a container, return its opener byte (`{` or `[`).
/// Returns None for scalars.
fn container_opener_byte(doc: &Document, cur: Cursor) -> Option<u8> {
    if cur.idx_start as usize >= doc.indices.len() { return None; }
    let pos = doc.indices[cur.idx_start as usize] as usize;
    let b = *doc.buf.get(pos)?;
    if b == b'{' || b == b'[' { Some(b) } else { None }
}

/// Iterate children of the container at `cur` and return a Cursor for the
/// matching child. Populates the skip cache on the first visit; uses it on
/// subsequent visits.
fn walk_children(doc: &Document, cur: Cursor, seg: &PathSeg) -> Result<Cursor, qjd_err> {
    let is_obj = matches!(seg, PathSeg::Key(_));
    let mut cache = doc.skip.borrow_mut();
    let (slot_n, was_cached) = cache.get_or_insert(cur.idx_start);

    if was_cached {
        // Fast path: iterate cached (start, end) pairs. No brace counting.
        let slot = cache.slot(slot_n);
        let starts = slot.child_starts.clone();
        let ends   = slot.child_ends.clone();
        drop(cache);
        return resolve_in_known_children(doc, &starts, &ends, is_obj, seg);
    }

    // Slow path: walk all children, populate cache fully, record match if any.
    let mut starts: Vec<u32> = Vec::new();
    let mut ends:   Vec<u32> = Vec::new();
    let mut i = cur.idx_start + 1;
    let end = cur.idx_end;
    let mut arr_idx: u32 = 0;
    let mut result: Option<Cursor> = None;

    while i < end {
        starts.push(i);

        let value_idx_start = if is_obj { i + 3 } else { i };
        let (cursor_end, skip_end) = find_value_span(doc, value_idx_start)?;
        ends.push(cursor_end);

        // Match check (we keep walking after a match to populate the cache).
        if result.is_none() {
            let matched = if is_obj {
                let key_open = doc.indices[i as usize] as usize;
                let key_close = doc.indices[(i + 1) as usize] as usize;
                if doc.buf.get(key_open).copied() != Some(b'"') {
                    return Err(qjd_err::QJD_PARSE_ERROR);
                }
                let key_bytes = &doc.buf[key_open + 1 .. key_close];
                matches!(seg, PathSeg::Key(want) if key_bytes == *want)
            } else {
                matches!(seg, PathSeg::Idx(want) if arr_idx == *want)
            };
            if matched {
                result = Some(Cursor { idx_start: value_idx_start, idx_end: cursor_end });
            }
        }

        // Advance to next sibling.
        let after_pos = doc.indices[skip_end as usize] as usize;
        if after_pos >= doc.buf.len() { break; }
        match doc.buf[after_pos] {
            b',' => { i = skip_end + 1; arr_idx += 1; }
            b'}' | b']' => break,
            _ => return Err(qjd_err::QJD_PARSE_ERROR),
        }
    }

    let slot = cache.slot_mut(slot_n);
    slot.child_starts = starts;
    slot.child_ends   = ends;

    match result {
        Some(c) => Ok(c),
        None    => Err(qjd_err::QJD_NOT_FOUND),
    }
}

fn resolve_in_known_children(
    doc: &Document, starts: &[u32], ends: &[u32], is_obj: bool, seg: &PathSeg,
) -> Result<Cursor, qjd_err> {
    for (k, (&i, &cursor_end)) in starts.iter().zip(ends.iter()).enumerate() {
        let matched = if is_obj {
            let key_open = doc.indices[i as usize] as usize;
            let key_close = doc.indices[(i + 1) as usize] as usize;
            let key_bytes = &doc.buf[key_open + 1 .. key_close];
            matches!(seg, PathSeg::Key(want) if key_bytes == *want)
        } else {
            matches!(seg, PathSeg::Idx(want) if (k as u32) == *want)
        };
        if matched {
            let value_idx_start = if is_obj { i + 3 } else { i };
            return Ok(Cursor { idx_start: value_idx_start, idx_end: cursor_end });
        }
    }
    Err(qjd_err::QJD_NOT_FOUND)
}

/// Given the indices position of a value's first marker, return:
///   (cursor_end, skip_end)
///
/// cursor_end: the idx_end value to store in a Cursor pointing at this value.
///   - container: index of the matching closer in `indices`
///   - string: index of the close '"' in `indices` (= start + 1)
///   - scalar:  start + 1  (convention: one past the bounding structural char)
///
/// skip_end: the indices position whose buf byte is the separator (',') or
///   parent closer ('}'/']') that immediately follows this value.
///   - container: index after the matching closer (= closer_idx + 1)
///   - string:    index after the close '"' (= start + 2)
///   - scalar:    start itself (indices[start] IS the separator/closer)
pub(crate) fn find_value_span(doc: &Document, start: u32) -> Result<(u32, u32), qjd_err> {
    let pos = doc.indices[start as usize] as usize;
    let b = *doc.buf.get(pos).ok_or(qjd_err::QJD_PARSE_ERROR)?;
    match b {
        b'{' | b'[' => {
            // Brace-count to matching closer.
            let want_close = if b == b'{' { b'}' } else { b']' };
            let mut depth: i32 = 1;
            let mut k = start + 1;
            while (k as usize) < doc.indices.len() {
                let cb_pos = doc.indices[k as usize] as usize;
                if cb_pos >= doc.buf.len() { return Err(qjd_err::QJD_PARSE_ERROR); }
                let cb = doc.buf[cb_pos];
                match cb {
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            if cb != want_close { return Err(qjd_err::QJD_PARSE_ERROR); }
                            // cursor_end = closer index (k)
                            // skip_end   = one past closer (k+1), pointing at ','
                            //              or parent closer
                            return Ok((k, k + 1));
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            Err(qjd_err::QJD_PARSE_ERROR)
        }
        b'"' => {
            // String value: indices has both opening (start) and closing (start+1) quotes.
            // cursor_end = start+1 (close '"')
            // skip_end   = start+2 (char after close '"', i.e., ',' or closer)
            Ok((start + 1, start + 2))
        }
        _ => {
            // Scalar (number/true/false/null): no own structural marker.
            // indices[start] IS the separator or closer after the scalar.
            // cursor_end = start+1  (convention: idx_end = idx_start + 1)
            // skip_end   = start    (indices[start] is the separator/closer)
            Ok((start + 1, start))
        }
    }
}

pub(crate) fn resolve_single_key(doc: &Document, cur: Cursor, key: &[u8]) -> Result<Cursor, qjd_err> {
    step(doc, cur, &PathSeg::Key(key))
}

pub(crate) fn resolve_single_idx(doc: &Document, cur: Cursor, idx: u32) -> Result<Cursor, qjd_err> {
    step(doc, cur, &PathSeg::Idx(idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_of(s: &[u8]) -> Document<'_> { Document::parse(s).unwrap() }

    #[test]
    fn root_path_returns_root() {
        let d = doc_of(b"{\"a\":1}");
        let c = Cursor::root(&d).resolve(&d, b"").unwrap();
        assert_eq!(c, Cursor::root(&d));
    }

    #[test]
    fn simple_key() {
        let d = doc_of(b"{\"a\":1}");
        let c = Cursor::root(&d).resolve(&d, b"a").unwrap();
        assert_ne!(c, Cursor::root(&d));
    }

    #[test]
    fn nested_key() {
        let d = doc_of(b"{\"a\":{\"b\":2}}");
        let _ = Cursor::root(&d).resolve(&d, b"a.b").unwrap();
    }

    #[test]
    fn missing_key_is_not_found() {
        let d = doc_of(b"{\"a\":1}");
        let r = Cursor::root(&d).resolve(&d, b"b");
        assert_eq!(r, Err(qjd_err::QJD_NOT_FOUND));
    }

    #[test]
    fn type_mismatch_on_index_into_object() {
        let d = doc_of(b"{\"a\":1}");
        let r = Cursor::root(&d).resolve(&d, b"[0]");
        assert_eq!(r, Err(qjd_err::QJD_TYPE_MISMATCH));
    }

    #[test]
    fn type_mismatch_on_key_into_array() {
        let d = doc_of(b"[1,2,3]");
        let r = Cursor::root(&d).resolve(&d, b"a");
        assert_eq!(r, Err(qjd_err::QJD_TYPE_MISMATCH));
    }

    #[test]
    fn array_index() {
        let d = doc_of(b"[10,20,30]");
        let _ = Cursor::root(&d).resolve(&d, b"[1]").unwrap();
    }

    #[test]
    fn array_out_of_bounds() {
        let d = doc_of(b"[10,20]");
        let r = Cursor::root(&d).resolve(&d, b"[5]");
        assert_eq!(r, Err(qjd_err::QJD_NOT_FOUND));
    }

    #[test]
    fn cache_hit_on_repeated_access() {
        let d = doc_of(b"{\"a\":1,\"b\":2,\"c\":3}");
        let r1 = Cursor::root(&d).resolve(&d, b"a").unwrap();
        let r2 = Cursor::root(&d).resolve(&d, b"b").unwrap();
        let r3 = Cursor::root(&d).resolve(&d, b"c").unwrap();
        // All succeed; they should differ.
        assert_ne!(r1, r2);
        assert_ne!(r2, r3);
        // Verify exactly one cache slot was created for the root container.
        let cache = d.skip.borrow();
        assert_eq!(cache.len(), 1);
    }
}
