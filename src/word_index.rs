// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::cmp;

#[repr(align(8))]
#[derive(Copy, Clone, Eq, PartialEq)]
struct Key {
    offset: u32,
    len: u32,
}

#[repr(align(8))]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Values {
    offset: u32,
    len: u32,
}

/// An index that associates one or more items with string keys.
pub trait WordIndex {
    /// Return the number of words in the index.
    fn len(&self) -> usize;

    /// Return the value ranges for all keys of which `prefix` is a prefix.
    fn search_prefix(&self, prefix: &str) -> &[Values];
}

struct MemoryWordIndex<T> {
    key_data: String,
    key_slices: Vec<Key>,
    value_data: Vec<T>,
    value_slices: Vec<Values>,
}

impl<T> MemoryWordIndex<T> {
    fn new<'a, I>(elements: I) -> MemoryWordIndex<T>
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

    fn get_key(&self, key: Key) -> &str {
        &self.key_data[key.offset as usize..key.offset as usize + key.len as usize]
    }

    fn get_values(&self, range: Values) -> &[T] {
        &self.value_data[range.offset as usize..range.offset as usize + range.len as usize]
    }

    /// Compare `prefix` to the same-length prefix of the `index`-th key.
    fn cmp_prefix(&self, prefix: &str, index: usize) -> cmp::Ordering {
        let key = self.get_key(self.key_slices[index]);

        // Compare bytes, up to the shortest of the two.
        let n = cmp::min(prefix.len(), key.len());
        let lhs = &prefix[..n];
        let rhs = &key[..n];

        lhs.cmp(rhs)
    }

    /// Return the index of the first key that has the given prefix.
    fn find_lower(&self, prefix: &str) -> usize {
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
        // Min is now the last key less than the prefix,
        // max is the first key greater or equal.
        max
    }

    /// Return the index of the first key after those with the given prefix.
    fn find_upper(&self, prefix: &str) -> usize {
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
        // Min is now the last key less than or equal to the prefix,
        // max is the first key greater.
        max
    }
}

impl<T> WordIndex for MemoryWordIndex<T> {
    fn len(&self) -> usize {
        self.key_slices.len()
    }

    fn search_prefix(&self, prefix: &str) -> &[Values] {
        let min = self.find_lower(prefix);
        let max = self.find_upper(prefix);
        &self.value_slices[min..max]
    }
}
