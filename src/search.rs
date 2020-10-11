// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::cmp;
use std::cmp::Ordering;
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

    /// Peek the value of the next element in the union.
    fn peek_value(&self) -> Option<&'a I::Item> {
        match self.iters.peek() {
            None => None,
            Some(iter) => iter.peek_value(),
        }
    }

    /// Peek the word metadata of the next element in the union.
    fn peek_meta(&self) -> Option<&'a WordMeta> {
        match self.iters.peek() {
            None => None,
            Some(iter) => iter.peek_meta(),
        }
    }

    /// Step ahead to the next element in the union.
    fn advance(&mut self) {
        let mut iter = self.iters.pop().expect("Should only advance if peek was succesful.");
        iter.advance();
        if !iter.is_empty() {
            self.iters.push(iter);
        }
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

fn intersect<'a, I: 'a + WordIndex, F: FnMut(&I::Item, &[WordMeta])>(
    index: &'a I,
    full_word_slices: &[Values],
    mut prefix_values: Union<'a, I>,
    mut on_match: F,
) where
  I::Item: cmp::Ord + Copy + std::fmt::Debug
{
    let mut iters = Vec::new();
    let mut values = Vec::new();
    let mut metas = Vec::new();

    for &vs in full_word_slices {
        let iter = IndexIter::new(index, vs);

        match iter.peek_value() {
            // If any of the iterators is empty, the intersection is empty,
            // so we have nothing to do here.
            None => return,
            Some(v) => {
                iters.push(iter);
                values.push(v);
            }
        }
    }

    match prefix_values.peek_value() {
        None => return,
        Some(v) => values.push(v),
    }

    let mut value = *values.iter().max().expect("We have at least the union value.");

    'matches: loop {
        metas.clear();

        'iters: for iter in iters.iter_mut() {
            'values: while let Some(ref v) = iter.peek_value() {
                match value.cmp(v) {
                    Ordering::Greater => {
                        // This iterator is still less than the maximum, advance
                        // until we match or pass it.
                        iter.advance();
                        continue 'values
                    }
                    Ordering::Equal => {
                        // Looks like we have a match so far, let's collect the
                        // associated metadata as well.
                        // TODO: Skip match if meta is not unique, for dupe words.
                        metas.push(*iter.peek_meta().expect("Meta must match value."));
                        continue 'iters
                    }
                    Ordering::Less => {
                        // We found a new maximum, now we need to start over
                        // with the other iters to see if they match.
                        value = v;
                        continue 'matches
                    }
                }
            }

            // If we get here, then one of the iterators is exhausted, which
            // means the remainder is not in the intersection, so we can stop.
            return
        }

        // If we get here, then all iterators are currently peeking the same
        // value, which means we potentially have a match. We still have to
        // check the final union iterator as well.
        'final_values: while let Some(ref v) = prefix_values.peek_value() {
            match value.cmp(v) {
                Ordering::Greater => {
                    prefix_values.advance();
                    continue 'final_values
                }
                Ordering::Equal => {
                    // We found an element of the intersection!
                    metas.push(*prefix_values.peek_meta().expect("Meta must match value."));
                    // Report the match through the callback.
                    on_match(value, &metas[..]);
                    // Then advance all iterators to move on to the next match.
                    for iter in iters.iter_mut() { iter.advance(); }
                    prefix_values.advance();
                    continue 'matches
                }
                Ordering::Less => {
                    // It was not a match after all, we have a new max now.
                    value = v;
                    continue 'matches
                }
            }
        }
        // If we get here, then the union iterator is exhausted, and the
        // intersection also ends.
        return
    }
}

pub fn search<'a, I: 'a + WordIndex, W: 'a + AsRef<str>>(
    index: &'a I,
    words: &'a [W],
    into: &mut Vec<I::Item>
) where I::Item: cmp::Ord + Copy + std::fmt::Debug{
    let mut results = Vec::new();

    // Break the search query in words to search only exact matches for, and the
    // final word, for which we also search for prefix matches. The idea is that
    // for search-as-you-type, the last word is incomplete, but the others are
    // complete.
    let mut words_iter = words.iter().rev();
    let prefix_word = match words_iter.next() {
        // If there are no search words at all, then there are no results either.
        None => return,
        Some(word) => word.as_ref(),
    };

    let mut exact_ranges = Vec::with_capacity(words.len() - 1);
    for word in words_iter {
        match index.search_exact(word.as_ref()) {
            // If any of the query words is not present in the index, then the
            // result is empty.
            None => return,
            Some(range) => exact_ranges.push(range),
        }
    }

    let prefix_ranges = index.search_prefix(prefix_word);
    let prefix_matches = Union::new(index, prefix_ranges);

    intersect(
        index,
        &exact_ranges[..],
        prefix_matches,
        |item, metas| {
            for (meta, word) in metas.iter().zip(words.iter()) {
                if meta.rank() > 0 {
                    // TODO: Take all metas into account when searching.
                    results.push((*item, word.as_ref(), *meta));
                    break
                }
            }
        },
    );

    results.sort_by_key(|&(_, word, meta)| {
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

    for (item, _word, _meta) in results.drain(..) {
        into.push(item);
    }
}
