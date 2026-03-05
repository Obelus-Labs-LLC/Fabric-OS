//! Fixed-size hash map for alloc-free capability store (TD-008).
//!
//! Open-addressing hash table with u64 keys, Fibonacci hashing, and
//! backward-shift deletion (no tombstones). Lazily allocates a single
//! `Box<[Option<(u64, V)>]>` on first use — after that, zero allocations.
//!
//! Replaces `BTreeMap<u64, V>` in the capability store, budget tracker,
//! and nonce tracker for deterministic O(1) amortized performance.

#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

/// Default capacity (must be power of 2).
/// 16384 slots at 75% max load = 12288 usable entries.
pub const DEFAULT_CAPACITY: usize = 16384;

/// Fibonacci hash: multiply by golden ratio constant, take upper bits.
#[inline]
fn fib_hash(key: u64, mask: usize) -> usize {
    let h = key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((h >> 32) as usize) & mask
}

/// A fixed-capacity hash map with u64 keys and open addressing.
///
/// Designed for kernel use: one-time allocation at init, then zero-alloc
/// operations with O(1) amortized lookup/insert/remove.
pub struct FixedMap<V> {
    /// Slot array: None = empty, Some((key, value)) = occupied.
    slots: Option<Box<[Option<(u64, V)>]>>,
    /// Total capacity (always power of 2).
    capacity: usize,
    /// Mask for modular indexing: capacity - 1.
    mask: usize,
    /// Number of occupied slots.
    len: usize,
}

impl<V> FixedMap<V> {
    /// Create an uninitialized map. Backing storage is allocated lazily
    /// on first insert (after the heap is available).
    pub const fn new() -> Self {
        Self {
            slots: None,
            capacity: 0,
            mask: 0,
            len: 0,
        }
    }

    /// Ensure backing storage is allocated.
    #[inline]
    fn ensure_init(&mut self) {
        if self.slots.is_none() {
            self.alloc_slots(DEFAULT_CAPACITY);
        }
    }

    /// Allocate slot array with the given capacity (must be power of 2).
    fn alloc_slots(&mut self, capacity: usize) {
        debug_assert!(capacity > 0 && capacity.is_power_of_two());
        let mut v = Vec::with_capacity(capacity);
        v.resize_with(capacity, || None);
        self.slots = Some(v.into_boxed_slice());
        self.capacity = capacity;
        self.mask = capacity - 1;
    }

    /// Hash a u64 key to a slot index using Fibonacci hashing.
    #[inline]
    fn hash(&self, key: u64) -> usize {
        fib_hash(key, self.mask)
    }

    /// Number of entries in the map.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the map is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Insert a key-value pair. If the key already exists, the value is updated.
    /// Returns `true` on success, `false` if the map is full.
    pub fn insert(&mut self, key: u64, value: V) -> bool {
        self.ensure_init();

        let mask = self.mask;
        let cap = self.capacity;
        let mut idx = fib_hash(key, mask);
        let slots = self.slots.as_mut().unwrap();

        loop {
            match &slots[idx] {
                None => {
                    // Empty slot — insert here
                    if self.len >= cap * 3 / 4 {
                        return false; // Load factor exceeded
                    }
                    slots[idx] = Some((key, value));
                    self.len += 1;
                    return true;
                }
                Some((k, _)) if *k == key => {
                    // Key exists — update value
                    slots[idx] = Some((key, value));
                    return true;
                }
                _ => {
                    idx = (idx + 1) & mask;
                }
            }
        }
    }

    /// Look up a value by key.
    pub fn get(&self, key: &u64) -> Option<&V> {
        let mask = self.mask;
        let mut idx = fib_hash(*key, mask);
        let slots = self.slots.as_ref()?;

        loop {
            match &slots[idx] {
                None => return None,
                Some((k, v)) if *k == *key => return Some(v),
                _ => idx = (idx + 1) & mask,
            }
        }
    }

    /// Look up a mutable reference by key.
    pub fn get_mut(&mut self, key: &u64) -> Option<&mut V> {
        let mask = self.mask;
        let mut idx = fib_hash(*key, mask);
        let slots = self.slots.as_mut()?;

        // Find the index first (immutable scan)
        loop {
            match &slots[idx] {
                None => return None,
                Some((k, _)) if *k == *key => break,
                _ => idx = (idx + 1) & mask,
            }
        }

        // Now mutably access the found slot
        slots[idx].as_mut().map(|(_, v)| v)
    }

    /// Check if a key exists.
    #[inline]
    pub fn contains_key(&self, key: &u64) -> bool {
        self.get(key).is_some()
    }

    /// Remove a key and return its value.
    ///
    /// Uses backward-shift deletion to avoid tombstones: after removing
    /// an entry, subsequent entries in the same probe chain are shifted
    /// backward to fill the gap, maintaining O(1) lookup performance.
    pub fn remove(&mut self, key: &u64) -> Option<V> {
        let mask = self.mask;
        let mut idx = fib_hash(*key, mask);
        let slots = self.slots.as_mut()?;

        // Find the slot containing this key
        loop {
            match &slots[idx] {
                None => return None,
                Some((k, _)) if *k == *key => break,
                _ => idx = (idx + 1) & mask,
            }
        }

        // Extract the value
        let removed = slots[idx].take().map(|(_, v)| v);
        self.len -= 1;

        // Backward-shift: move subsequent displaced entries back to fill the gap
        let mut empty = idx;
        let mut scan = (idx + 1) & mask;

        loop {
            if slots[scan].is_none() {
                break;
            }

            let natural = fib_hash(slots[scan].as_ref().unwrap().0, mask);

            // Should we move the entry at `scan` into `empty`?
            // Yes, if `empty` falls in the range [natural, scan) cyclically,
            // meaning the entry was probed past the gap position.
            let should_move = if natural <= scan {
                empty >= natural && empty < scan
            } else {
                empty >= natural || empty < scan
            };

            if should_move {
                slots.swap(empty, scan);
                empty = scan;
            }

            scan = (scan + 1) & mask;
        }

        removed
    }

    /// Get or insert: return a mutable reference to the value for `key`,
    /// inserting `default` if the key doesn't exist.
    ///
    /// This is the FixedMap equivalent of `BTreeMap::entry(key).or_insert(default)`.
    pub fn get_or_insert(&mut self, key: u64, default: V) -> &mut V {
        self.ensure_init();

        // Check if key exists
        if !self.contains_key(&key) {
            self.insert(key, default);
        }

        self.get_mut(&key).expect("get_or_insert: key must exist after insert")
    }

    /// Iterate over all key-value pairs (unordered).
    pub fn iter(&self) -> impl Iterator<Item = (&u64, &V)> {
        self.slots
            .iter()
            .flat_map(|s| s.iter())
            .filter_map(|slot| slot.as_ref().map(|(k, v)| (k, v)))
    }

    /// Clear all entries without deallocating the backing storage.
    pub fn clear(&mut self) {
        if let Some(ref mut slots) = self.slots {
            for slot in slots.iter_mut() {
                *slot = None;
            }
        }
        self.len = 0;
    }
}
