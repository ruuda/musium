
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

#[repr(C, packed)]
struct FlatTrieEndNode {
    /// Offset of the data associated with this node.
    ///
    /// Two values have special meaning:
    ///
    /// * 0 indicates that there is no data associated with the node.
    /// * 1 indicates that the node continues in the next struct. The actual
    ///   data offset is stored in the last non-continuation struct.
    data_ptr: u32,

    /// The offsets of the child nodes, corresponding to the symbols.
    child_ptrs: [u32; 5],

    /// The symbols, for which the node offsets are stored.
    symbols: [u8; 5],
}

#[cfg(test)]
mod test {
    use super::FlatTrieNode;
    use std::mem;

    #[test]
    fn entry_has_expected_size() {
        // Four `Entry` instances should fit one cache line exactly.
        assert_eq!(mem::size_of::<Entry>(), 16);
    }

    #[test]
    fn flat_trie_node_has_expected_size() {
        // A `FlatTrieNode` should be one cache line exactly.
        assert_eq!(mem::size_of::<FlatTrieNode>(), 64);
        assert_eq!(mem::size_of::<FlatTrieEndNode>(), 32);
    }
}
