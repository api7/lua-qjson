use rustc_hash::FxHashMap;

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
    pub(crate) child_starts: Vec<u32>,
}

impl SkipCache {
    pub(crate) fn new() -> Self {
        Self {
            slots: vec![SkipSlot { child_starts: Vec::new() }],
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
        self.slots.push(SkipSlot { child_starts: Vec::new() });
        self.by_opener.insert(opener_idx, new);
        (new, false)
    }

    pub(crate) fn slot_mut(&mut self, n: u32) -> &mut SkipSlot {
        &mut self.slots[n as usize]
    }

    pub(crate) fn slot(&self, n: u32) -> &SkipSlot {
        &self.slots[n as usize]
    }

    pub(crate) fn len(&self) -> usize { self.by_opener.len() }
}
