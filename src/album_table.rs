// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Defines a specialized hash table keyed on album id.

use crate::AlbumId;

/// A hash table with album id keys.
///
/// Album ids, and their use case, have a few properties that allow a
/// specialized hash table to make different trade-offs than a general one:
///
/// * Album ids are random by construction (based off Musicbrainz UUIDs).
/// * We never delete entries.
/// * Building the table is done once at startup, it is worthwhile to optimize
///   for lookups at the expense of inserts.
///
/// With that in mind, we go for an open-addressing, robin hood table.
///
/// Works best for 8-byte payloads.
pub struct AlbumTable<T: Copy> {
    elements: Box<[(AlbumId, T)]>,
    mask: usize,
    max_probe_len: usize,
}

impl<T: Copy> AlbumTable<T> {
    /// Create a new table that can hold at most `n` elements.
    ///
    /// We need to provide a dummy value to initialize unused slots. The dummy
    /// value is never exposed on lookups, so it does not have to be a sentinel
    /// value, it can be a value that would normally be valid.
    pub fn new(n: usize, dummy: T) -> AlbumTable<T> {
        let num_slots = n.next_power_of_two();
        AlbumTable {
            elements: vec![(AlbumId(0), dummy); num_slots].into_boxed_slice(),
            mask: num_slots - 1,
            max_probe_len: 0,
        }
    }

    /// Return `x` such that `(i0 + x) & mask = i1`.
    fn offset(&self, i0: usize, i1: usize) -> usize {
        debug_assert!(i0 <= self.mask, "Index i0 must already be wrapped.");
        debug_assert!(i1 <= self.mask, "Index i1 must already be wrapped.");

        if i0 <= i1 {
            i1 - i0
        } else {
            (i1 + self.mask + 1) - i0
        }
    }

    /// "Hash" an album id to its preferred index. The hash function is identity.
    fn index(&self, key: AlbumId) -> usize {
        (key.0 as usize) & self.mask
    }

    fn is_slot_empty(&self, index: usize) -> bool {
        debug_assert!(index <= self.mask, "Index must be wrapped.");
        self.elements[index].0 == AlbumId(0)
    }

    pub fn insert(&mut self, mut key: AlbumId, mut value: T) {
        debug_assert_ne!(key, AlbumId(0), "Album id 0 is the sentinel, it cannot be inserted.");

        let mut base_index = self.index(key);
        let mut probe_len = 0;

        while probe_len <= self.mask {
            let index = (base_index + probe_len) & self.mask;

            // If the desired slot is free, fill it, and then we are done.
            if self.is_slot_empty(index) {
                self.elements[index] = (key, value);
                self.max_probe_len = self.max_probe_len.max(probe_len);
                return
            }

            let current = self.elements[index];
            let current_base_index = self.index(current.0);
            let current_probe_len = self.offset(current_base_index, index);

            // If the existing element at this slot has a smaller offset from
            // its ideal location than the key we are inserting does, then we
            // steal this slot and move the other key instead.
            if probe_len > current_probe_len {
                self.elements[index] = (key, value);
                self.max_probe_len = self.max_probe_len.max(probe_len);
                // Find a new slot for the element that we just evicted.
                base_index = current_base_index;
                probe_len = current_probe_len;
                key = current.0;
                value = current.1;
            }

            probe_len = probe_len + 1;
        }
        panic!("Failed to insert, the table is full.");
    }

    pub fn get(&self, key: AlbumId) -> Option<T> {
        debug_assert_ne!(key, AlbumId(0), "Album id 0 is the sentinel, it is never present.");
        let base_index = self.index(key);

        // Probe from the ideal position, until either we find an empty slot
        // (and then we know the key is not present), or until the max probe
        // length (and then we also know we aren't going to find the key).
        for off in 0..=self.max_probe_len {
            let index = (base_index + off) & self.mask;
            match self.elements[index].0 {
                x if x == key => return Some(self.elements[index].1),
                AlbumId(0) => return None,
                _ => continue,
            }
        }

        None
    }

    /// Return the maximum number of elements that the table can hold.
    pub fn capacity(&self) -> usize {
        self.elements.len()
    }

    /// Return the maximum probe length required to look up any key.
    pub fn max_probe_len(&self) -> usize {
        self.max_probe_len
    }
}

#[cfg(test)]
mod tests {
    use crate::AlbumId;
    use super::AlbumTable;

    #[test]
    fn album_table_offset_non_wrapping() {
        let t = AlbumTable::new(64, 0_u64);
        assert_eq!(t.offset(0, 0), 0);
        assert_eq!(t.offset(0, 10), 10);
        assert_eq!(t.offset(0, 63), 63);
        assert_eq!(t.offset(32, 33), 1);
        assert_eq!(t.offset(49, 59), 10);
    }

    #[test]
    fn album_table_offset_wrapping() {
        let t = AlbumTable::new(64, 0_u64);
        assert_eq!(t.offset(0, 0), 0);
        assert_eq!(t.offset(10, 0), 54);
        assert_eq!(t.offset(63, 0), 1);
        assert_eq!(t.offset(33, 32), 63);
        assert_eq!(t.offset(59, 49), 54);
    }

    #[test]
    fn album_table_insert_then_get_no_collisions() {
        // Try insertion in 5 different orders.
        for base in 0..5 {
            let mut t = AlbumTable::new(5, 0_u64);
            for i in 0..5 {
                // In this test the keys are all distinct, and smaller than the
                // capacity, so there are never collisions.
                let k = 1 + ((base + i) % 5);
                t.insert(AlbumId(k), k);
            }
            for i in 1..6 {
                assert_eq!(t.get(AlbumId(i)), Some(i));
            }
            for i in 6..11 {
                assert_eq!(t.get(AlbumId(i)), None);
            }
        }
    }

    #[test]
    fn album_table_insert_then_get_all_collisions() {
        // Try insertion in 5 different orders.
        for base in 0..5 {
            let mut t = AlbumTable::new(5, 0_u64);
            for i in 0..5 {
                let k = 1 + ((base + i) % 5);
                // In this test the keys are all equivalent modulo 8 (the
                // capacity of the table), so every insert is a collision.
                t.insert(AlbumId(k * 8), k);
            }
            for i in 1..6 {
                assert_eq!(t.get(AlbumId(i * 8)), Some(i));
            }
            for i in 6..11 {
                assert_eq!(t.get(AlbumId(i * 8)), None);
            }
        }
    }

    #[test]
    fn album_table_insert_then_get_some_collisions() {
        // Try insertion in 5 different orders.
        for base in 0..5 {
            let mut t = AlbumTable::new(5, 0_u64);
            for i in 0..5 {
                let k = 1 + ((base + i) % 5);
                // In this test the keys fall into two equivalence classes
                // modulo 8, so keys collide, but not all on the same base
                // bucket.
                t.insert(AlbumId(k * 4), k);
            }
            for i in 1..6 {
                assert_eq!(t.get(AlbumId(i * 4)), Some(i));
            }
            for i in 6..11 {
                assert_eq!(t.get(AlbumId(i * 4)), None);
            }
        }
    }
}
