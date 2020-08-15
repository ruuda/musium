// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Defines the word index data structure and interface.
//!
//! The word index consists of several packed arrays of data, together with
//! arrays of indexes into those.
//!
//! * The **key data** is a single long string (UTF-8) of all of the words in
//!   the index concatenated.
//! * The **key slices array** is an array of (offset, len) pairs that slice a
//!   word out of the key data. Key slices are sorted in memcmp order to
//!   facilitate a binary search for matching prefixes.
//! * The **value data** is an array of values (the value type is a generic
//!   type, instantiated to track id, album id, or artist id).
//! * The **value slices array** is an array of (offset, len) pairs that slice
//!   one or more values out of the value data. The length of the value slices
//!   array is the same as that of the key slices array: for the key at index
//!   _i_, the value slice at index _i_ lists all values associated with that
//!   key.
//! * The **medatada array** is an array of match metadata, the same length as
//!   the value data array. For the value at index _i_, the match metadata at
//!   index _i_ contains metadata used to rank the matched value among other
//!   matches.
//!
//! Typically a search works like this:
//!
//! * Perform two binary searches on the key slices to find the range of keys
//!   that have the search needle as prefix.
//! * For each matching key, gather associated values and match metadata.
//! * Use match metadata to rank the matches.

use std::cmp;
use std::mem;
use std::fmt;

/// Packed metadata about a an entry in the word index.
///
/// Fields by bit range (lower bound inclusive, upper bound exclusive):
///
/// * 0..8: `word_len`
/// * 8..16: `total_len`
/// * 16..24: `index`
/// * 24..30: `log_frequency`
/// * 30..32: `rank`
///
/// See getters for more details about these fields.
#[repr(C, align(4))]
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WordMeta(u32);

impl WordMeta {
    /// The length of the word itself.
    #[inline]
    pub fn word_len(self) -> u32 {
        (self.0 >> 0) & 0xff
    }

    /// The total length of the string in which the word occurs.
    ///
    /// Used for ranking results: if the makes up is a greater portion of the
    /// total string, the result is more relevant.
    #[inline]
    pub fn total_len(self) -> u32 {
        (self.0 >> 8) & 0xff
    }

    /// The 0-based word index at which the word occurs in the string.
    ///
    /// Used for ranking results: if the word occurs early in the string, the
    /// result is more relevant.
    #[inline]
    pub fn index(self) -> u32 {
        (self.0 >> 16) & 0xff
    }

    /// Log2 of the number of values for the word.
    ///
    /// Used for ranking results: a word that occurs in many tracks is less
    /// informative than a word that occurs only in a single track, so when the
    /// prefix is still short, this pushes down common words like “to” or “was”.
    #[inline]
    pub fn log_frequency(self) -> u32 {
        (self.0 >> 24) & 0b0011_1111
    }

    /// The rank of the entry.
    ///
    /// The following ranks are used:
    ///
    /// 0. Tertiary, not shown by default. Used for words from the artist name
    ///    in the album index, used for words from the track artist that also
    ///    occur in the album artist in the track index.
    /// 1. Secondary. Used for words from the track artist that do not occur in
    ///    the album artist, in the track index.
    /// 2. Primary, the word occurs in the album title or track title.
    ///
    /// This means that higher ranks are better, and an a track or album should
    /// have at least one word of nonzero rank to be included in the results.
    #[inline]
    pub fn rank(self) -> u32 {
        (self.0 >> 30) & 0b11
    }

    pub fn new(
        word_len: usize,
        total_len: usize,
        index: usize,
        rank: u8
    ) -> WordMeta {
        /// Narrow an `usize` to a `bits`-wide unsigned integer, saturating on
        /// overflow.
        fn clamp(bits: usize, x: usize) -> u32 {
            x.min(1 << bits) as u32
        }

        WordMeta(
            0
            | (clamp(8, word_len) << 0)
            | (clamp(8, total_len) << 8)
            | (clamp(8, index) << 16)
            | (clamp(8, rank as usize) << 30)
        )
    }

    /// Return a copy of the word meta, with log-frequency filled in.
    fn set_frequency(self, frequency: u64) -> WordMeta {
        debug_assert!(frequency > 0, "Frequency must be positive.");
        let log2_frequency = 63 - frequency.leading_zeros();

        // The 6-bit log-frequency is at offset 24.
        WordMeta(self.0 | ((log2_frequency as u32) << 24))
    }
}

#[repr(align(8))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Key {
    offset: u32,
    len: u32,
}

/// A slice of values in the word index, usually all values associated with a key.
#[repr(align(8))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Values {
    pub offset: u32,
    pub len: u32,
}

/// An index that associates one or more items with string keys.
pub trait WordIndex {
    type Item;

    /// Return the number of words in the index.
    fn len(&self) -> usize;

    /// Return the value ranges for all keys of which `prefix` is a prefix.
    fn search_prefix(&self, prefix: &str) -> &[Values];

    /// Return the values for a value range returned from a search.
    fn get_values(&self, range: Values) -> &[Self::Item];

    /// Return the metadata associated with the values in the value range.
    fn get_metas(&self, range: Values) -> &[WordMeta];

    /// Return the values for a value from a range returned from a search.
    fn get_value(&self, offset: u32) -> &Self::Item;

    /// Return the metadata associated with the values at the offset.
    fn get_meta(&self, offset: u32) -> &WordMeta;
}

pub struct MemoryWordIndex<T> {
    key_slices: Vec<Key>,
    value_slices: Vec<Values>,
    key_data: String,
    // TODO: Benchmark (Vec<T>, Vec<WordMeta>) against Vec<(T, WordMeta)>. It is
    // not obvious which will be better for locality: in an intersection query,
    // we expect most values to be out of the intersection, so there we avoid
    // wasting cache on the metadata, and we can pack more values in the same
    // cache lines. But if there is a single query word, we load everything, and
    // having to jump around between two places is probably worse then when they
    // are adjacent.
    value_data: Vec<T>,
    meta_data: Vec<WordMeta>,
}

pub struct WordIndexSize {
    key_data_bytes: usize,
    value_data_bytes: usize,
    meta_data_bytes: usize,
    slice_bytes: usize,
    num_keys: usize,
    num_values: usize,
}

impl fmt::Display for WordIndexSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f,
            "{:5} keys, {:5} values, {:4} kB ({:3} kB keys, {:3} kB values, {:3} kB meta, {:3} kB slices)",
            self.num_keys,
            self.num_values,
            (self.key_data_bytes + self.value_data_bytes + self.meta_data_bytes + self.slice_bytes) / 1000,
            self.key_data_bytes / 1000,
            self.value_data_bytes / 1000,
            self.meta_data_bytes / 1000,
            self.slice_bytes / 1000,
        )
    }
}

impl<T> MemoryWordIndex<T> {
    /// Build a memory word index from a sorted sequence of (word, value) pairs.
    pub fn new<'a, I>(elements: I) -> MemoryWordIndex<T>
    where
        I: IntoIterator<Item = &'a (String, T, WordMeta)>,
        T: 'a + Copy
    {
        let mut key_data = String::new();
        let mut value_data = Vec::new();
        let mut meta_data = Vec::new();

        let mut key_slices = Vec::new();
        let mut value_slices = Vec::new();

        let mut prev_word = "";
        let mut values = Values {
            offset: 0,
            len: 0,
        };

        fn fixup_meta_frequency(metas: &mut [WordMeta], values: Values) {
            let from = values.offset as usize;
            let to = from + values.len as usize;
            for meta in &mut metas[from..to] {
                *meta = meta.set_frequency(values.len as u64);
            }
        }

        for &(ref word, value, meta) in elements {
            if word != prev_word {
                // Finish up the previous value slice, if any.
                if values.len > 0 {
                    fixup_meta_frequency(&mut meta_data[..], values);
                    value_slices.push(values);
                }

                let word_slice = Key {
                    offset: key_data.len() as u32,
                    len: word.len() as u32,
                };

                key_data.push_str(word);
                key_slices.push(word_slice);

                values = Values {
                    offset: value_data.len() as u32,
                    len: 0,
                };

                prev_word = word;
            }

            value_data.push(value);
            meta_data.push(meta);
            values.len += 1;
        }

        // Finish up the last word.
        fixup_meta_frequency(&mut meta_data[..], values);
        value_slices.push(values);

        MemoryWordIndex {
            key_slices: key_slices,
            value_slices: value_slices,
            key_data: key_data,
            value_data: value_data,
            meta_data: meta_data,
        }
    }

    pub fn size(&self) -> WordIndexSize {
        WordIndexSize {
            key_data_bytes: self.key_data.len(),
            value_data_bytes: self.value_data.len() * mem::size_of::<T>(),
            meta_data_bytes: self.meta_data.len() * mem::size_of::<WordMeta>(),
            slice_bytes:
                self.key_slices.len() * mem::size_of::<Key>() +
                self.value_slices.len() * mem::size_of::<Values>(),
            num_keys: self.key_slices.len(),
            num_values: self.value_data.len(),
        }
    }


    fn get_key(&self, key: Key) -> &str {
        &self.key_data[key.offset as usize..key.offset as usize + key.len as usize]
    }

    /// Compare `prefix` to the same-length prefix of the `index`-th key.
    fn cmp_prefix(&self, prefix: &str, index: usize) -> cmp::Ordering {
        let key = self.get_key(self.key_slices[index]);

        // Compare bytes, limiting the key if it is longer.
        let n = cmp::min(prefix.len(), key.len());
        prefix.as_bytes().cmp(&key.as_bytes()[..n])
    }

    /// Return the index of the first key that has the given prefix.
    ///
    /// If no key has the prefix, returns the index of the key before which to
    /// insert to keep the order sorted.
    fn find_lower(&self, prefix: &str) -> usize {
        // Invariant: keys[min] <= prefix <= keys[max].
        let mut min = 0;
        let mut max = self.key_slices.len();
        while max - min > 1 {
            let i = (min + max) / 2;
            match self.cmp_prefix(prefix, i) {
                // The prefix goes before key i, we learned a tighter upper bound.
                cmp::Ordering::Less => max = i,
                // We are in the key range of the prefix. Because we look for
                // the start of that range, we learned a tighter upper bound.
                cmp::Ordering::Equal => max = i,
                // The prefix goes after key i, we learned a tighter lower bound.
                cmp::Ordering::Greater => min = i,
            }
        }

        // From the invariant alone, we can't tell if min or max is the answer,
        // we need to check min itself too (max can't be checked, it may be out
        // of bounds). If prefix would sort before the min, return 0 for the
        // lower bound, not -1. The upper bound will also be 0, so the slice has
        // length zero.
        match self.cmp_prefix(prefix, min) {
            cmp::Ordering::Less => 0,
            cmp::Ordering::Equal => min,
            cmp::Ordering::Greater => max,
        }
    }

    /// Return the index of the first key after those with the given prefix.
    ///
    /// If no key has the prefix, returns the index of the key before which to
    /// insert to keep the order sorted.
    fn find_upper(&self, prefix: &str) -> usize {
        // Invariant: keys[min] < keys[max], prefix < keys[max].
        let mut min = 0;
        let mut max = self.key_slices.len();
        while max - min > 1 {
            let i = (min + max) / 2;
            match self.cmp_prefix(prefix, i) {
                // The prefix goes before key i, we learned a tighter upper bound.
                cmp::Ordering::Less => max = i,
                // We are in the key range of the prefix. Because we look for
                // the end of that range, we learned a tighter lower bound.
                cmp::Ordering::Equal => min = i,
                // The prefix goes after key i, we learned a tighter lower bound.
                cmp::Ordering::Greater => min = i,
            }
        }

        // From the invariant alone, we can't tell if min or max is the answer,
        // we need to check min itself too (max can't be checked, it may be out
        // of bounds). If prefix would sort before the min, return 0 for the
        // upper bound, so the slice has length zero.
        match self.cmp_prefix(prefix, min) {
            cmp::Ordering::Less => 0,
            cmp::Ordering::Equal => max,
            cmp::Ordering::Greater => max,
        }
    }
}

impl<T> WordIndex for MemoryWordIndex<T> {
    type Item = T;

    fn len(&self) -> usize {
        self.key_slices.len()
    }

    fn get_values(&self, range: Values) -> &[T] {
        &self.value_data[range.offset as usize..range.offset as usize + range.len as usize]
    }

    fn get_metas(&self, range: Values) -> &[WordMeta] {
        &self.meta_data[range.offset as usize..range.offset as usize + range.len as usize]
    }

    fn get_value(&self, offset: u32) -> &T {
        &self.value_data[offset as usize]
    }

    fn get_meta(&self, offset: u32) -> &WordMeta {
        &self.meta_data[offset as usize]
    }

    fn search_prefix(&self, prefix: &str) -> &[Values] {
        let min = self.find_lower(prefix);
        let max = self.find_upper(prefix);
        &self.value_slices[min..max]
    }
}

#[cfg(test)]
mod test {
    use super::{MemoryWordIndex, Key, Values, WordIndex, WordMeta};
    use std::collections::BTreeSet;

    /// Dummy word metadata for use in these tests.
    const M0: WordMeta = WordMeta(0);

    #[test]
    fn test_word_meta_fits_u32() {
        use std::mem;
        assert_eq!(mem::size_of::<WordMeta>(), 4);
    }

    #[test]
    fn test_build_word_index_all_unique() {
        let mut elems = BTreeSet::new();
        elems.insert(("A".to_string(),  2, M0));
        elems.insert(("BB".to_string(), 3, M0));
        elems.insert(("C".to_string(),  5, M0));

        let index = MemoryWordIndex::new(&elems);

        assert_eq!(&index.key_data, "ABBC");
        assert_eq!(&index.value_data, &[2, 3, 5]);

        assert_eq!(index.key_slices[0], Key { offset: 0, len: 1});
        assert_eq!(index.key_slices[1], Key { offset: 1, len: 2});
        assert_eq!(index.key_slices[2], Key { offset: 3, len: 1});

        assert_eq!(index.value_slices[0], Values { offset: 0, len: 1});
        assert_eq!(index.value_slices[1], Values { offset: 1, len: 1});
        assert_eq!(index.value_slices[2], Values { offset: 2, len: 1});
    }

    #[test]
    fn test_build_word_index_many_per_word() {
        let mut elems = BTreeSet::new();
        elems.insert(("A".to_string(),  2, M0));
        elems.insert(("A".to_string(),  5, M0));
        elems.insert(("B".to_string(),  2, M0));
        elems.insert(("B".to_string(),  5, M0));
        elems.insert(("B".to_string(),  7, M0));
        elems.insert(("C".to_string(), 11, M0));

        let index = MemoryWordIndex::new(&elems);

        assert_eq!(&index.key_data, "ABC");
        assert_eq!(&index.value_data, &[2, 5, 2, 5, 7, 11]);

        assert_eq!(index.key_slices[0], Key { offset: 0, len: 1});
        assert_eq!(index.key_slices[1], Key { offset: 1, len: 1});
        assert_eq!(index.key_slices[2], Key { offset: 2, len: 1});

        assert_eq!(index.value_slices[0], Values { offset: 0, len: 2});
        assert_eq!(index.value_slices[1], Values { offset: 2, len: 3});
        assert_eq!(index.value_slices[2], Values { offset: 5, len: 1});
    }

    #[test]
    fn test_search_prefix_shorter() {
        let mut elems = BTreeSet::new();
        elems.insert(("appendix".to_string(),  1, M0));
        elems.insert(("asterisk".to_string(),  2, M0));
        elems.insert(("asterism".to_string(),  3, M0));
        elems.insert(("asterism".to_string(),  4, M0));
        elems.insert(("astrology".to_string(), 5, M0));
        elems.insert(("astronomy".to_string(), 6, M0));
        elems.insert(("attribute".to_string(), 7, M0));
        elems.insert(("borealis".to_string(),  8, M0));

        let index = MemoryWordIndex::new(&elems);

        let vs: Vec<_> = index
            .search_prefix("aste")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![2, 3, 4]);

        let vs: Vec<_> = index
            .search_prefix("ast")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![2, 3, 4, 5, 6]);

        let vs: Vec<_> = index
            .search_prefix("a")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![1, 2, 3, 4, 5, 6, 7]);

        let vs: Vec<_> = index
            .search_prefix("")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_search_prefix_longer() {
        let mut elems = BTreeSet::new();
        elems.insert(("a".to_string(),        1, M0));
        elems.insert(("as".to_string(),       2, M0));
        elems.insert(("tea".to_string(),      3, M0));
        elems.insert(("the".to_string(),      4, M0));
        elems.insert(("theo".to_string(),     5, M0));
        elems.insert(("theremin".to_string(), 6, M0));
        elems.insert(("thermos".to_string(),  7, M0));

        let index = MemoryWordIndex::new(&elems);

        let vs: Vec<_> = index
            .search_prefix("theo")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![5]);

        assert_eq!(index.search_prefix("theorem"), &[]);
        assert_eq!(index.search_prefix("thermometer"), &[]);
        assert_eq!(index.search_prefix("astronomy"), &[]);

        let vs: Vec<_> = index
            .search_prefix("as")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![2]);
    }

    #[test]
    fn test_search_bounds() {
        // This test failed once on real index data.

        let mut elems = BTreeSet::new();
        elems.insert(("hybrid".to_string(),     1, M0));
        elems.insert(("hypotenuse".to_string(), 2, M0));
        elems.insert(("minds".to_string(),      3, M0));
        elems.insert(("tycho".to_string(),      4, M0));

        let index = MemoryWordIndex::new(&elems);

        // "a" would be before element 0, spans 0..0.
        assert_eq!(index.find_lower("a"), 0);
        assert_eq!(index.find_upper("a"), 0);

        // "h" spans 0..2
        assert_eq!(index.find_lower("h"), 0);
        assert_eq!(index.find_upper("h"), 2);

        // "hy" also still
        assert_eq!(index.find_lower("hy"), 0);
        assert_eq!(index.find_upper("hy"), 2);

        // "hyb" matches only 0, "hyp" only 1.
        assert_eq!(index.find_lower("hyb"), 0);
        assert_eq!(index.find_upper("hyb"), 1);
        assert_eq!(index.find_lower("hyp"), 1);
        assert_eq!(index.find_upper("hyp"), 2);

        // "k" would fall between 1 and 2, so 2..2.
        assert_eq!(index.find_lower("k"), 2);
        assert_eq!(index.find_upper("k"), 2);

        // "m" hits 2 exactly.
        assert_eq!(index.find_lower("m"), 2);
        assert_eq!(index.find_upper("m"), 3);

        // "o" would fall between 2 and 3, so 3..3.
        assert_eq!(index.find_lower("o"), 3);
        assert_eq!(index.find_upper("o"), 3);

        // "ty" hits 3 exactly.
        assert_eq!(index.find_lower("ty"), 3);
        assert_eq!(index.find_upper("ty"), 4);

        // "v" is past the end.
        assert_eq!(index.find_lower("v"), 4);
        assert_eq!(index.find_upper("v"), 4);
    }

    #[test]
    fn test_search_prefix_regression() {
        // This test failed once on real index data.

        let mut elems = BTreeSet::new();
        elems.insert(("hybrid".to_string(), 1, M0));
        elems.insert(("minds".to_string(),  2, M0));
        elems.insert(("tycho".to_string(),  3, M0));

        let index = MemoryWordIndex::new(&elems);

        assert_eq!(index.find_lower("hy"), 0);
        assert_eq!(index.find_upper("hy"), 1);

        let vs: Vec<_> = index
            .search_prefix("hy")
            .iter()
            .flat_map(|&v| index.get_values(v).iter().cloned())
            .collect();
        assert_eq!(vs, vec![1]);

        assert_eq!(index.find_lower("e"), 0);
        assert_eq!(index.find_upper("e"), 0);
    }

    #[test]
    fn test_search_multibyte_key() {
        let mut elems = BTreeSet::new();
        elems.insert(("abacus".to_string(),     1, M0));
        elems.insert(("zenith".to_string(),     2, M0));
        elems.insert(("クリスタル".to_string(), 3, M0));

        let index = MemoryWordIndex::new(&elems);

        assert_eq!(index.find_lower("z"), 1);
        assert_eq!(index.find_upper("z"), 2);
    }

    #[test]
    fn test_search_multibyte_needles() {
        let mut elems = BTreeSet::new();
        elems.insert(("abacus".to_string(),     1, M0));
        elems.insert(("zenith".to_string(),     2, M0));
        elems.insert(("クリスタル".to_string(), 3, M0));

        let index = MemoryWordIndex::new(&elems);

        assert_eq!(index.find_lower("ク"), 2);
        assert_eq!(index.find_upper("ク"), 3);
    }
}
