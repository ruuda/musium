// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Utilities for string deduplication and word splitting and normalization.

use std::mem;

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

pub struct StringDeduper {
    strings_to_id: HashMap<String, u32>,
    strings: Vec<String>,
}

impl StringDeduper {
    pub fn new() -> StringDeduper {
        StringDeduper {
            strings_to_id: HashMap::new(),
            strings: Vec::new(),
        }
    }

    /// Insert the string, or return its index if it was present already.
    pub fn insert(&mut self, string: &str) -> u32 {
        // If the string exists already, return its id, otherwise insert it.
        // This does involve two lookups in the case of insert, but it does save
        // an allocation that turns the &str into a String when an insert is
        // required. We expect inserts to occur less than half of the time
        // (usually the sort artist is the same as the artist, and many tracks
        // share the same artist), therefore opt for the check first. Empirical
        // evidence: on my personal library, about 22% of the strings need to be
        // inserted (12.6k out of 57.8k total strings).
        let next_id = self.strings.len() as u32;
        // TODO: Unicode-normalize the string.
        if let Some(id) = self.strings_to_id.get(string) { return *id }
        self.strings_to_id.insert(string.to_string(), next_id);
        self.strings.push(string.to_string());
        next_id
    }

    /// Return the underlying string vector, destroying the deduplicator.
    pub fn into_vec(self) -> Vec<String> {
        self.strings
    }

    /// Return the string with the given index. Panics when out of bounds.
    pub fn get(&self, index: u32) -> &str {
        &self.strings[index as usize]
    }

    /// Replace most straigt quotes (') in strings with typographer’s quotes (‘ and ’).
    ///
    /// Although some tags use typographer’s quotes, most do not, also on
    /// Musicbrainz. But the typographer’s quotes look nicer, especially in Work
    /// Sans, which is used for Musium’s library browser. So apply a heuristic
    /// to replace most straigh quotes with curly ones.
    ///
    /// This is a heuristic, it is not perfect. In particular, this function
    /// mistakes apostrophes before a word, for opening quotes. The tags must be
    /// edited to sidestep this shortcoming.
    pub fn upgrade_quotes(&mut self) {
        for s in self.strings.iter_mut() {
            // NOTE: We could use memchr for this if it turns out to be a
            // bottleneck.
            let mut from = 0;
            while let Some(off) = &s[from..].find('\'') {
                let i = from + off;

                let before = if i > 0 { s.as_bytes()[i - 1] } else { b' ' };
                let after = if i < s.len() - 1 { s.as_bytes()[i + 1] } else { b' ' };

                let after_word = after == b' ' || after == b',' || after == b')';
                let after_letter = before.is_ascii_alphabetic();
                let after_digit = before.is_ascii_digit();
                let before_word = before == b' ';
                let before_letter = after.is_ascii_alphabetic();
                let before_digit = after.is_ascii_digit();

                let replacement = match () {
                    // Contractions like n’t, names like O’Neil.
                    _ if after_letter && before_letter => Some("’"),
                    // Abbreviations like dreamin’.
                    _ if after_letter && after_word => Some("’"),
                    // Usually years or other numbers, like 80’s or 5’s.
                    _ if after_digit && before_letter => Some("’"),
                    // Usually years, like ’93.
                    _ if before_word && before_digit => Some("’"),
                    // Often opening quote, but it can also be a contraction,
                    // like ’cause, ’em, or ’til, and then this gets it wrong
                    // ... To remove all doubt, your tags.
                    _ if before_word && before_letter => Some("‘"),
                    // What remains in my collection are things like contractions
                    // in non-ascii words (e.g. C’était), and quotes after
                    // numbers, which I think stands for a length in feet.
                    // Non-ascii letters are difficult to detect, and for the
                    // numbers, the straight quote is appropriate, so we'll
                    // leave it at this.
                    _ => None
                };

                if let Some(r) = replacement {
                    s.replace_range(i..i + 1, r);
                    from = i + r.len();
                } else {
                    from = i + "'".len();
                }
            }
        }
    }
}

fn push_word(dest: &mut Vec<String>, word: &mut String) {
    if word.len() == 0 {
        return
    }

    let mut w = String::new();
    mem::swap(&mut w, word);
    dest.push(w);
}

/// Fills the vector with the words in the string in normalized form.
///
/// This first normalizes words to Unicode Normalization Form KD, which
/// decomposes characters with accents into the character and the accent
/// separately. The "KD" form, as opposed to the "D" form, also replaces more
/// things that have the same semantic meaning, such as replacing superscripts
/// with normal digits. Finally (not part of the KD normalization), everything
/// is lowercased, and accents and some punctuation are removed.
pub fn normalize_words(title: &str, dest: &mut Vec<String>) {
    // We assume that in the majority of the cases, the transformations
    // below do not change the number of bytes.
    let mut word = String::new();
    let mut num_dots = 0;

    // Drop some punctuation characters and accents. We remove punctuation that
    // is unlikely to contain a lot of information about the title. (Deadmau5
    // can go and use some normal titles next time.) We remove accents to make
    // searching easier without having to type the exact accent.
    // U+309a and U+3099 are Japanese diacritics.
    let drop = "“”‘’'\"`()[]«»,❦|\u{300}\u{301}\u{302}\u{303}\u{304}\u{306}\u{307}\u{308}\u{30a}\u{30c}\u{323}\u{327}\u{328}\u{309a}\u{3099}";
    let keep = "$€#&=*%∆";

    // Cut words at the following punctuation characters, but still include them
    // as a word of their own. This ensures that words are broken up properly,
    // but it still allows searching for this punctuation. This is important,
    // because some artists are under the illusion that it is cool to use
    // punctuation as part of a name.
    let cut = "/\\@_+-:;!?<>";

    // Loop over the characters, normalized and lowercased.
    for ch in title.nfkd().flat_map(|nch| nch.to_lowercase()) {
        match ch {
            // Split words at whitespace or at the cut characters.
            _ if ch.is_whitespace() => {
                push_word(dest, &mut word);
            }
            _ if cut.contains(ch) => {
                push_word(dest, &mut word);
                dest.push(ch.to_string());
            }
            // The period is special: generally we don't want to include it as a
            // word, and simply ignore it altogether. (E.g. "S.P.Y" turns into
            // "spy".) But the ellipisis (...) we do want to keep. There are
            // even tracks titled "...". So we need to detect the ellipsis.
            '.' => {
                num_dots += 1;
                if num_dots == 3 {
                    dest.push("...".to_string());
                    word = String::new();
                }
                continue
            }
            // Treat the upside-down question mark as a separator like the
            // regular one, but then do include the upright one as the word,
            // so you can search for ¿ by typing ?. Same for exclamation mark.
            '¿' => {
                push_word(dest, &mut word);
                dest.push("?".to_string());
            }
            '¡' => {
                push_word(dest, &mut word);
                dest.push("!".to_string());
            }
            // Cut on an en- and em-dash just like we cut on a hyphen, but
            // include the hyphen, so they are equivalent for the purpose of
            // search.
            '–' | '—' => {
                push_word(dest, &mut word);
                dest.push("-".to_string());
            }
            // Normalize a few characters to more common ones.
            // Sometimes used in "n°", map to "no".
            '°' => word.push('o'),
            '♯' => word.push('#'),
            'ø' => word.push('o'),
            'ð' => word.push('d'),
            '×' => word.push('x'),
            'æ' => word.push_str("ae"),
            'œ' => word.push_str("oe"),
            // A hyphen, use the ascii one instead.
            '\u{2010}' => word.push('-'),
            // I do want to be able to find my Justice albums with a normal
            // keyboard.
            '✝' => {
                push_word(dest, &mut word);
                dest.push("cross".to_string());
            }
            '∞' => {
                push_word(dest, &mut word);
                dest.push("infinity".to_string());
            }
            '¥' => {
                push_word(dest, &mut word);
                dest.push("yen".to_string());
            }
            // Drop characters that we don't care for, keep characters that we
            // definitely care for.
            _ if drop.contains(ch) => {}
            _ if keep.contains(ch) || ch.is_alphanumeric() => word.push(ch),
            _ => panic!("Unknown character {} ({}) in title: {}", ch, ch.escape_unicode(), title),
        }

        // Reset the ellipsis counter after every non-period character.
        num_dots = 0;
    }

    // Push the final word.
    push_word(dest, &mut word);
}

#[cfg(test)]
mod test {
    use super::normalize_words;

    fn expect_normalize_words(input: &str, expected_output: &[&str]) {
        let mut words = Vec::new();
        normalize_words(input, &mut words);
        let words_slice: Vec<&str> = words.iter().map(|s| &s[..]).collect();
        assert_eq!(&words_slice[..], expected_output);
    }

    #[test]
    pub fn test_normalize_words() {
        expect_normalize_words("Ṣānnu yārru lī", &["sannu", "yarru", "li"]);
        expect_normalize_words("Orð vǫlu", &["ord", "volu"]);
    }
}
