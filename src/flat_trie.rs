
// Some stats: on the index of words that occur in track titles, for my personal
// collection, the index contains roughly 8k unique words. There are 52 distinct
// first bytes of those words. For the number of distinct second bytes (given
// the first), the minimum is 0 (a 1-character word), the median is 8, and the
// maximum is 28. However, counting like this is not entirely fair, because the
// nodes with many leaves are likely also the common search queries.
//
// So what are the options? A binary search would need 12 or 13 = log2(8000)
// lookups to locate a word, and if these are all behind a pointer, that is 24
// cache misses (12 for the binary search random indexing; 12 for the pointer
// chase to sthe string). Note that the time to search depends on the number
// words in the index, not on the size of the query word. (Lexicographic
// comparison of the words is practically free, because the strings will fit in
// a cache line, even though in theory the comparison takes time proportional to
// the length of the string.) So 24 cache misses, can we do better?
//
// We could inline the strings up to some maximum, and have some marker for
// overflow into the next slot. No more pointer chase, just 12 misses per query.
// And actually, the element half-way is probably going to be hot in the cache,
// so let's say 11 misses. Can we do better?
//
// We could store the bounds into the string array in a table of 256 entries,
// and index that based on the first byte of the query. Assuming the words are
// rougly uniformly distributed, starting with 26 distinct characters (there are
// more, but these are uncommon), that reduces the search space from 8k to 308,
// so the binary search costs only 8 or 9 misses (and the table we can assume is
// hot).
//
// We could take the 256-entry array to the extreme, and make a 256-ary tree.
// At that point, the number of misses would be proportional to the length of
// the query, not to the number of words in the index. However, that would waste
// a *lot* of memory, which puts pressure on the cache.
//
// Would something proportional to the size of the query actually be
// advantageous? The min, median, and max word length, where the median is
// weighed by the number of occurrences (so "the" is counted once for every
// track title it occurs in), are 1, 4, and 31 bytes. Although the median in
// this case is likely skewed by short common words such as "the" and "in". When
// looking at unique words only, the median length is 6 bytes.
//
// We could do a trie-like structure and have something that incurs 5 or 6
// misses in the median case, but for something proportional to the query size,
// we might be unlucky and need more than 9 misses. Suddenly the sorted array
// with inline strings sounds very attractive, because it is so simple, and it
// has worst-case guarantees.
//
// Idea (sorry for the stream of thoughts): don't use a trie, use a regular
// tree! I want a sorted map/dictionary, a tree is the canonical implementatin
// of that. And here too, the string can be inlined. For a binary search tree,
// that would have the same number of cache misses as a binary search. But now
// for the killer trick: the tree does not need to be binary. With 7-byte
// strings we could store 4 key/value pairs per cache line, plus 5 child
// pointers. So instead of loading log2(8000) cache lines, we need to load only
// log5(8000) => 5 or 6 cache lines. Or for 12-byte strings, have 4 children,
// and 6 or 7 cache misses. Minus the root which is likely hot, we are now down
// to 4-6 misses, with efficient memory usage (4 + 4/n additional bytes per
// string, compared to the array, but nowhere near the waste of a 256-ary trie).
//
// Some more statistics: with all words combined from track title, album title,
// album artist, and track artist, I have 9222 unique words. Length percentiles:
// p0: 1, p50: 6, p75: 8, p90: 9, p100: 31. When I discard 90% of the input data
// (of the raw words, which may contain duplicates), I get 2135 unique words.
// When discarding 50% of the input data, I get 6358 unique words. Based on
// this, I am not entirely comfortable assuming that word indexes would fit in
// 16 bits.
//
// We now need to make a choice: store data in the leaf nodes only, or also in
// the internal nodes? For a branching factor of 5, strings in the internal
// nodes take up up, 20% of the strings, so this would require 20% more memory.
// But in exchange, we don't need to store data pointers in internal nodes, so
// strings can be longer (11 bytes, as opposed to 7 bytes if they also need to
// store a data pointer). Besides, branching factors of 4 and 5 could both be
// viable. So the options:
//
// * Data in internal nodes, branching factor 4. 3 strings of 12 bytes in the
//   internal nodes. 3 x 4 bytes data pointers, and 4 x 4 bytes child pointers,
//   fills one cache line. Leaves could store 4 12-byte strings.
//
//   Solving `4p + 3q = 9200, p = q * (\sum_{n=1}^\infty (1/4)^n)` yields
//   approximately p = 708, q = 2123, so we would need 2811 cache lines in
//   total. (Assuming strings longer than 12 bytes are sufficiently rare.)
//
//   In the worst case we would access 7 cache lines to query a given string,
//   but in 25% of the cases we definitely do better. In the best case we would
//   access 1 cache line.
//
// * Internal nodes duplicate strings, branching factor 5. 4 strings of 11 bytes
//   in the internal nodes, 4 strings of 12 bytes in the leaves.
//
//   Solving `q = 9200/4, p = q * (\sum_{n=1}^\infty (1/5)^n)` yields p = 575,
//   q = 2300, so we would need 2875 cache lines. (Assuming again strings longer
//   than 11 bytes are negligible ... they might not be, 483 of 9222 words were
//   at least 11 bytes, 252 were at least 12 bytes.
//
//   In the worst case we would access 6 cache lines. In the best cache we would
//   access 5 cache lines.
//
// My conclusion: let's try duplicating the strings in the nodes. By the way, we
// really do need to deal with the long string edge case: truncated strings are
// not necessarily unique. All of these occur in my corpus:
//
// * conversation
// * conversationalist
// * conversations
// * shapeshifted
// * shapeshifter
// * shapeshifters

#[repr(C, packed)]
struct InternalNode {
    /// Indices of the leaf nodes.
    ///
    /// Element `i` points to the leaf for queries where `q <= keys[i]`
    /// lexicographically. The last element points to the leaf for queries
    /// bigger than any of the keys.
    ///
    /// A value of 0 is used to indicate that the key did not fit in a single
    /// slot, and overflows into the next slot. This can happen multiple times,
    /// also into the next `InternalNode`.
    leaves: [u32; 5],

    /// String keys corresponding to the maximum of each leaf node.
    ///
    /// This contains the bytes of the UTF-8 encoded string. Strings shorter
    /// than 11 characters are padded with zeros at the end. For strings longer
    /// than 11 characters, the corresponding leaf index is set to 0, and the
    /// string continues in the next key, which might be in the next node.
    keys: [[u8; 11]; 4],
}

/// Entry in a leaf node. Four of these together form one leaf.
#[repr(C, packed)]
struct Entry {
    /// Offset of the data associated with this entry.
    ///
    /// A 0 indicates that the key did not fit in this entry, and it continues
    /// in the next entry. The actual data offset is stored in the last
    /// non-continuation entry.
    data_ptr: u32,

    /// The key, padded at the end with zeros if it is shorter than 12 bytes.
    key: [u8; 12],
}

pub struct FlatTreeBuilder {
    leaves: Vec<Entry>,
    last_key: Vec<u8>,
    internal_full: Vec<InternalNode>,
    internal_open: Vec<InternalNode>,
}

impl FlatTreeBuilder {
    pub fn new() -> FlatTreeBuilder {
        FlatTreeBuilder {
            leaves: Vec::new(),
            last_key: Vec::new(),
            internal_full: Vec::new(),
            internal_open: Vec::new(),
        }
    }

    pub fn insert(&mut self, key: &[u8], value: u32) {
        // If the entry would overflow into the next cache line, then instead
        // pad the current cache line with unused entries, and insert the
        // current entry on the next cache line. This way we can ensure that
        // lookups require loading only a single cache line, except when a
        // single entry is bigger than one cache line (48 bytes of key).
        let slots_left = 4 - (self.leaves.len() % 4);
        let slots_required = (key.len() + 11) / 12;
        if slots_required > slots_left {
            // TODO: Pad to the next cache line.
        }

        // Insert the internal node to finalize the previous cache line, if
        // applicable.
        if self.leaves.len() % 4 == 0 {
            // TODO: acutally insert internal nodes.
        }

        // Insert the entry, or multiple if the key is too long for a single
        // entry.
        let mut i = 0;
        loop {
            let j = key.len().min(i + 12);
            let mut entry_key = [0; 12];
            (&mut entry_key[..j - i]).copy_from_slice(&key[i..j]);

            let entry = Entry {
                data_ptr: if j == key.len() { value } else { 0 },
                key: entry_key,
            };
            self.leaves.push(entry);

            i = j;
            if j == key.len() { break }
            println!("overflow, {}/{}, leaf {}", j, key.len(), self.leaves.len());
        }

        // Store the last key added, which is used to add internal nodes.
        self.last_key.clear();
        self.last_key.extend(key.iter());
    }
}

#[repr(C, packed)]
struct FlatTrieNode {
    /// Offset of the data associated with this node.
    ///
    /// Two values have special meaning:
    ///
    /// * 0 indicates that there is no data associated with the node.
    /// * 1 indicates that the node continues in the next struct. The actual
    ///   data offset is stored in the last non-continuation struct.
    data_ptr: u32,

    /// The offsets of the child nodes, corresponding to the symbols.
    child_ptrs: [u32; 12],

    /// The symbols, for which the node offsets are stored.
    symbols: [u8; 12],
}

#[cfg(test)]
mod test {
    use super::FlatTrieNode;
    use std::mem;

    #[test]
    fn nodes_have_expected_size() {
        // Four `Entry` instances should fit one cache line exactly.
        assert_eq!(mem::size_of::<Entry>(), 16);

        // An internal node should be the size of a cache line.
        assert_eq!(mem::size_of::<InternalNode>(), 64);
    }
}
