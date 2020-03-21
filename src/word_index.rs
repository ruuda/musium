// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::cmp;
use std::mem;
use std::fmt;

#[repr(align(8))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Key {
    offset: u32,
    len: u32,
}

#[repr(align(8))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Values {
    offset: u32,
    len: u32,
}

/// An index that associates one or more items with string keys.
pub trait WordIndex {
    type Item;

    /// Return the number of words in the index.
    fn len(&self) -> usize;

    /// Return the value ranges for all keys of which `prefix` is a prefix.
    fn search_prefix(&self, prefix: &str) -> &[Values];

    fn get_values(&self, range: Values) -> &[Self::Item];
}

pub struct MemoryWordIndex<T> {
    key_data: String,
    key_slices: Vec<Key>,
    value_data: Vec<T>,
    value_slices: Vec<Values>,
}

pub struct WordIndexSize {
    key_data_bytes: usize,
    value_data_bytes: usize,
    slice_bytes: usize,
    num_keys: usize,
    num_values: usize,
}

impl fmt::Display for WordIndexSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f,
            "{:4} keys, {:5} values, {:3} kB ({:3} kB keys, {:3} kB values, {:3} kB slices)",
            self.num_keys,
            self.num_values,
            (self.key_data_bytes + self.value_data_bytes + self.slice_bytes) / 1000,
            self.key_data_bytes / 1000,
            self.value_data_bytes / 1000,
            self.slice_bytes / 1000,
        )
    }
}

impl<T> MemoryWordIndex<T> {
    /// Build a memory word index from a sorted sequence of (word, value) pairs.
    pub fn new<'a, I>(elements: I) -> MemoryWordIndex<T>
    where
        I: IntoIterator<Item = &'a (String, T)>,
        T: 'a + Copy
    {
        let mut key_data = String::new();
        let mut value_data = Vec::new();

        let mut key_slices = Vec::new();
        let mut value_slices = Vec::new();

        let mut prev_word = "";
        let mut values = Values {
            offset: 0,
            len: 0,
        };

        for &(ref word, value) in elements {
            if word != prev_word {
                // Finish up the previous value slice, if any.
                if values.len > 0 {
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
            values.len += 1;
        }

        // Finish up the last word.
        value_slices.push(values);

        MemoryWordIndex {
            value_data: value_data,
            value_slices: value_slices,
            key_data: key_data,
            key_slices: key_slices,
        }
    }

    pub fn size(&self) -> WordIndexSize {
        WordIndexSize {
            key_data_bytes: self.key_data.len(),
            value_data_bytes: self.value_data.len() * mem::size_of::<T>(),
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

    fn get_values(&self, range: Values) -> &[T] {
        &self.value_data[range.offset as usize..range.offset as usize + range.len as usize]
    }

    /// Compare `prefix` to the same-length prefix of the `index`-th key.
    fn cmp_prefix(&self, prefix: &str, index: usize) -> cmp::Ordering {
        let key = self.get_key(self.key_slices[index]);

        // Compare bytes, limiting the key if it is longer.
        let n = cmp::min(prefix.len(), key.len());
        prefix.cmp(&key[..n])
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
        self.get_values(range)
    }

    fn search_prefix(&self, prefix: &str) -> &[Values] {
        let min = self.find_lower(prefix);
        let max = self.find_upper(prefix);
        &self.value_slices[min..max]
    }
}

#[cfg(test)]
mod test {
    use super::{MemoryWordIndex, Key, Values, WordIndex};
    use std::collections::BTreeSet;

    #[test]
    fn test_build_word_index_all_unique() {
        let mut elems = BTreeSet::new();
        elems.insert(("A".to_string(), 2));
        elems.insert(("BB".to_string(), 3));
        elems.insert(("C".to_string(), 5));

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
        elems.insert(("A".to_string(), 2));
        elems.insert(("A".to_string(), 5));
        elems.insert(("B".to_string(), 2));
        elems.insert(("B".to_string(), 5));
        elems.insert(("B".to_string(), 7));
        elems.insert(("C".to_string(), 11));

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
        elems.insert(("appendix".to_string(), 1));
        elems.insert(("asterisk".to_string(), 2));
        elems.insert(("asterism".to_string(), 3));
        elems.insert(("asterism".to_string(), 4));
        elems.insert(("astrology".to_string(), 5));
        elems.insert(("astronomy".to_string(), 6));
        elems.insert(("attribute".to_string(), 7));
        elems.insert(("borealis".to_string(), 8));

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
        elems.insert(("a".to_string(), 1));
        elems.insert(("as".to_string(), 2));
        elems.insert(("tea".to_string(), 3));
        elems.insert(("the".to_string(), 4));
        elems.insert(("theo".to_string(), 5));
        elems.insert(("theremin".to_string(), 6));
        elems.insert(("thermos".to_string(), 7));

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
        elems.insert(("hybrid".to_string(), 1));
        elems.insert(("hypotenuse".to_string(), 2));
        elems.insert(("minds".to_string(), 3));
        elems.insert(("tycho".to_string(), 4));

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
        elems.insert(("hybrid".to_string(), 1));
        elems.insert(("minds".to_string(), 2));
        elems.insert(("tycho".to_string(), 3));

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
}
