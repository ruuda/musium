// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::cmp;
use std::collections::BinaryHeap;
use std::iter;

use crate::word_index::{Values, WordIndex, WordMeta};

/// Iterator over a value range of a word index.
struct IndexIter<'a, I: 'a + WordIndex> {
    index: &'a I,
    begin: u32,
    end: u32,
}

impl<'a, I: 'a + WordIndex> IndexIter<'a, I> {
    pub fn new(index: &'a I, values: Values) -> IndexIter<'a, I> {
        IndexIter {
            index: index,
            begin: values.offset,
            end: values.offset + values.len,
        }
    }

    pub fn peek_value(&self) -> Option<&'a I::Item> {
        if self.begin < self.end {
            Some(self.index.get_value(self.begin))
        } else {
            None
        }
    }

    pub fn peek_meta(&self) -> Option<&'a WordMeta> {
        if self.begin < self.end {
            Some(self.index.get_meta(self.begin))
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.begin == self.end
    }

    pub fn advance(&mut self) {
        self.begin += 1;
    }
}

// The ordering to put the iters into collections::binary_heap. Note that that
// heap is a max-heap, so we implement the reverse order here. The heap should
// not contain empty iterators, in that case we panic.
impl<'a, I: 'a + WordIndex> cmp::Ord for IndexIter<'a, I> where I::Item: cmp::Ord {
    fn cmp(&self, other: &IndexIter<'a, I>) -> cmp::Ordering {
        let v_self = self.peek_value().expect("Only non-empty IndexIters can be compared.");
        let v_other = other.peek_value().expect("Only non-empty IndexIters can be compared.");
        // Note the reversed order for the max-heap.
        v_other.cmp(v_self)
    }
}

impl<'a, I: 'a + WordIndex> cmp::PartialOrd for IndexIter<'a, I> where I::Item: cmp::Ord {
    fn partial_cmp(&self, other: &IndexIter<'a, I>) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, I: 'a + WordIndex> cmp::PartialEq for IndexIter<'a, I> where I::Item: cmp::PartialEq {
    fn eq(&self, other: &IndexIter<'a, I>) -> bool {
        let v_self = self.peek_value().expect("Only non-empty IndexIters can be compared.");
        let v_other = other.peek_value().expect("Only non-empty IndexIters can be compared.");
        *v_self == *v_other
    }
}

impl<'a, I: 'a + WordIndex> cmp::Eq for IndexIter<'a, I> where I::Item: cmp::Eq {}

/// Iterator over the union of multiple value ranges of a word index.
struct Union<'a, I: 'a + WordIndex> {
    value_slices: &'a [Values],
    iters: BinaryHeap<IndexIter<'a, I>>
}

impl<'a, I: 'a + WordIndex> Union<'a, I> where I::Item: cmp::Ord {
    pub fn new(index: &'a I, value_slices: &'a [Values]) -> Union<'a, I> {
        let mut iters = BinaryHeap::new();
        for &vs in value_slices {
            let iter = IndexIter::new(index, vs);
            if !iter.is_empty() {
                iters.push(iter);
            }
        }
        Union {
            value_slices: value_slices,
            iters: iters,
        }
    }

    /// Return the number of elements in this union.
    pub fn len(&self) -> usize {
        self.value_slices.iter().map(|v| v.len as usize).sum()
    }
}

impl<'a, I: 'a + WordIndex> iter::Iterator for Union<'a, I> where I::Item: cmp::Ord {
    type Item = (&'a I::Item, &'a WordMeta);

    fn next(&mut self) -> Option<(&'a I::Item, &'a WordMeta)> {
        match self.iters.pop() {
            None => None,
            Some(mut iter) => {
                let value = iter.peek_value().expect("Union should only store non-empty iters.");
                let meta = iter.peek_meta().expect("Union should only store non-empty iters.");
                iter.advance();

                if !iter.is_empty() {
                    self.iters.push(iter);
                }

                Some((value, meta))
            }
        }
    }
}

pub fn search<'a, I: 'a + WordIndex>(
    index: &'a I,
    word: &'a str,
    into: &mut Vec<I::Item>
) where I::Item: cmp::Ord + Copy {
    let mut results = Vec::new();
    let ranges = index.search_prefix(word);
    for (item, meta) in Union::new(index, ranges) {
        if meta.rank() > 0 {
            results.push((*item, *meta));
        }
    }

    results.sort_by_key(|&(_, meta)| {
        let mut penalty = 0_i32;

        // Add a penalty quadratic in the excess word length. This way we still
        // strongly prefer exact matches over prefix matches, but as the prefix
        // gets less complete, the rank plummets.
        let excess = meta.word_len() as i32 - word.len() as i32;
        penalty += excess * excess;

        // Discourage prefix matches further if the word is very common, on top
        // of the other frequency penalty below.
        penalty *= (meta.log_frequency() + 1) as i32;

        // A single excess character is better than a word that occurs later,
        // but the word position incurs a linear penalty, so it wins in the end.
        // Denote by "a" the factor. Then we have the following examples:
        //
        // * For query "time", "In Time" : "Times" = a : 1
        // * For query "beat", "Helena Beat" : "Beating Heart" = a : 9
        // * For query "bear", "Minus the Bear" : "Solar Bears" = 2a : a + 1
        //
        // We'll take a = 0.1 for now.
        penalty = 10 * penalty + meta.index() as i32;

        // Add a penalty for common words: users are unlikely to search for a
        // common word, so it is better to show distinctive words (even though
        // it may only be a prefix match and not an exact match) over common
        // words. E.g. when typing "a", the prefix matches "Aja" or "Animals"
        // should be more likely than "A Night at the Opera".
        penalty += 10 * (meta.log_frequency() * meta.log_frequency()) as i32;

        // The rank (2 for words in title, 0 for non-unique results in the
        // artist) acts as a multiplier, lower ranks are worse, and lead to
        // higher penalties.
        penalty *= 3 - meta.rank() as i32;

        // If we have the same penalty at this point, break ties by preferring
        // items where the word is a greater portion of the total. We can
        // consider the portion of the query word, or the portion of the matched
        // word. The former puts more relevant results first in the case of
        // exact matches, but the latter leads to stabler results during
        // search-as-you type, because an extra character does not change the
        // penalty. Because the penalty should already take care of putting
        // relevant results first, we go for the latter.
        (penalty, -100 * meta.word_len() as i32 / meta.total_len() as i32)
    });

    for (item, _meta) in results.drain(..) {
        into.push(item);
    }
}
