use rustc_hash::FxHashMap;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct SkipCache {
    /// Slot 0 reserved as "no cache" marker (never written to).
    slots: Vec<SkipSlot>,
    /// Map from a container's opener position-in-indices to slot index.
    by_opener: FxHashMap<u32, u32>,
}

pub(crate) struct SkipSlot {
    /// child_starts[i] = position in doc.indices of the i-th child's leading
    /// marker. For object children this is the key's opening '"'; for array
    /// children, the value's first marker.
    pub(crate) child_starts: Rc<[u32]>,
    /// child_ends[i] = the `cursor_end` value for the i-th child (i.e. the
    /// idx_end to put in a Cursor pointing at that child's value). Storing
    /// this lets cache-hit resolution skip the brace-counting find_value_span.
    pub(crate) child_ends:   Rc<[u32]>,
}

impl SkipCache {
    pub(crate) fn new() -> Self {
        let empty: Rc<[u32]> = Rc::from([]);
        Self {
            slots: vec![SkipSlot { child_starts: Rc::clone(&empty), child_ends: empty }],
            by_opener: FxHashMap::default(),
        }
    }

    /// Get an existing slot for this opener idx, or allocate a new (empty) one.
    /// Returns (slot_number, was_already_populated).
    pub(crate) fn get_or_insert(&mut self, opener_idx: u32) -> (u32, bool) {
        if let Some(&slot) = self.by_opener.get(&opener_idx) {
            return (slot, true);
        }
        let new = self.slots.len() as u32;
        let empty: Rc<[u32]> = Rc::from([]);
        self.slots.push(SkipSlot { child_starts: Rc::clone(&empty), child_ends: empty });
        self.by_opener.insert(opener_idx, new);
        (new, false)
    }

    pub(crate) fn slot_mut(&mut self, n: u32) -> &mut SkipSlot {
        &mut self.slots[n as usize]
    }

    pub(crate) fn slot(&self, n: u32) -> &SkipSlot {
        &self.slots[n as usize]
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize { self.by_opener.len() }
}
