// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::str::FromStr;
use std::sync::mpsc::SyncSender;

use crate::prim::{AlbumId, Album, ArtistId, Artist, TrackId, Track, Date, Lufs, FilenameRef, StringRef, get_track_id};
use crate::string_utils::{StringDeduper, normalize_words};
use crate::word_index::{WordMeta};
use crate::database::FileMetadata;

#[derive(Clone, Debug)]
pub enum IssueDetail {
    /// A required metadata field is missing. Contains the field name.
    FieldMissingError(&'static str),

    /// A recommended metadata field is missing. Contains the field name.
    FieldMissingWarning(&'static str),

    /// A metadata field could be parsed. Contains the field name.
    FieldParseFailedError(&'static str),

    /// A track title contains the phrase "(feat. ",
    /// which likely belongs in the artist instead.
    TrackTitleContainsFeat,

    /// Two different titles were found for albums with the same mbid.
    /// Contains the title used, and the discarded alternative.
    AlbumTitleMismatch(AlbumId, String, String),

    /// Two different release dates were found for albums with the same mbid.
    /// Contains the date used, and the discarded alternative.
    AlbumReleaseDateMismatch(AlbumId, Date, Date),

    /// Two different artists were found for albums with the same mbid.
    /// Contains the artist used, and the discarded alternative.
    AlbumArtistMismatch(AlbumId, ArtistId, ArtistId),

    /// Two different album loudnesses were found for albums with the same mbid.
    /// Contains the loudness used, and the discarded alternative.
    AlbumLoudnessMismatch(AlbumId, Option<Lufs>, Option<Lufs>),

    /// Two different names were found for album artists with the same mbid.
    /// Contains the name used, and the discarded alternative.
    ArtistNameMismatch(ArtistId, String, String),

    /// Two different sort names were found for album artists with the same mbid.
    /// Contains the name used, and the discarded alternative.
    ArtistSortNameMismatch(ArtistId, String, String),

    /// The file does not contain exactly two channels.
    NotStereo,

    /// The file does not use either 16 or 24 bits per sample.
    UnsupportedBitDepth(u32),
}

impl IssueDetail {
    pub fn for_file(self, filename: String) -> Issue {
        Issue {
            filename: filename,
            detail: self,
        }
    }
}

#[derive(Debug)]
pub struct Issue {
    pub filename: String,
    pub detail: IssueDetail,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:\n  ", self.filename)?;
        match self.detail {
            IssueDetail::FieldMissingError(field) =>
                write!(f, "error: field '{}' missing.", field),
            IssueDetail::FieldMissingWarning(field) =>
                write!(f, "warning: field '{}' missing.", field),
            IssueDetail::FieldParseFailedError(field) =>
                write!(f, "error: failed to parse field '{}'.", field),
            IssueDetail::TrackTitleContainsFeat =>
                write!(f, "warning: track title contains '(feat. '."),
            IssueDetail::NotStereo =>
                write!(f, "error: the file is not stereo"),
            IssueDetail::UnsupportedBitDepth(bits) =>
                write!(f, "error: {} bits per sample is not supported", bits),
            IssueDetail::AlbumTitleMismatch(_id, ref title, ref alt) =>
                write!(f, "warning: discarded inconsistent album title '{}' in favour of '{}'.", alt, title),
            IssueDetail::AlbumReleaseDateMismatch(_id, ref date, ref alt) =>
                write!(f, "warning: discarded inconsistent album release date {} in favour of {}.", alt, date),
            IssueDetail::AlbumArtistMismatch(_id, ref artist, ref alt) =>
                write!(f, "warning: discarded inconsistent album artist '{}' in favour of '{}'.", alt, artist),
            IssueDetail::ArtistNameMismatch(_id, ref name, ref alt) =>
                write!(f, "warning: discarded inconsistent artist name '{}' in favour of '{}'.", alt, name),
            IssueDetail::ArtistSortNameMismatch(_id, ref sort_name, ref alt) =>
                write!(f, "warning: discarded inconsistent sort name '{}' in favour of '{}'.", alt, sort_name),
            IssueDetail::AlbumLoudnessMismatch(_id, Some(loudness), Some(alt)) =>
                write!(f, "warning: discarded inconsistent loudness '{}' in favour of '{}'.", alt, loudness),
            IssueDetail::AlbumLoudnessMismatch(_id, Some(loudness), None) =>
                write!(f, "warning: replaced inconsistently missing loudness with '{}'.", loudness),
            IssueDetail::AlbumLoudnessMismatch(_id, None, Some(alt)) =>
                write!(f, "warning: ignored loudness '{}' because it is not unanimous.", alt),
            IssueDetail::AlbumLoudnessMismatch(_id, None, None) =>
                panic!("Not actually a loudness mismatch."),
        }
    }
}

#[derive(Debug)]
pub enum Progress {
    /// A number of files have been indexed.
    Indexed(u32),
    /// An issue with a file was encountered.
    Issue(Issue),
}

fn parse_date(date_str: &str) -> Option<Date> {
    // We expect at least a year.
    if date_str.len() < 4 { return None }

    let year = u16::from_str(&date_str[0..4]).ok()?;
    let mut month: u8 = 0;
    let mut day: u8 = 0;

    // If there is something following the year, it must be dash, and there must
    // be at least two digits for the month.
    if date_str.len() > 4 {
        if date_str.as_bytes()[4] != b'-' { return None }
        if date_str.len() < 7 { return None }
        month = u8::from_str(&date_str[5..7]).ok()?;
    }

    // If there is something following the month, it must be dash, and there
    // must be exactly two digits for the day.
    if date_str.len() > 7 {
        if date_str.as_bytes()[7] != b'-' { return None }
        if date_str.len() != 10 { return None }
        day = u8::from_str(&date_str[8..10]).ok()?;
    }

    // This is not the most strict date well-formedness check that we can do,
    // but it is something at least. Note that we do allow the month and day to
    // be zero, to indicate the entire month or entire year.
    if month > 12 || day > 31 {
        return None
    }

    Some(Date::new(year, month, day))
}

/// Parse a part of a 128-bit hexadecimal UUID into a 64-bit unsigned integer.
fn parse_uuid(uuid: &str) -> Option<u64> {
    // Validate that the textual format of the UUID is as expected.
    // E.g. `1070cbb2-ad74-44ce-90a4-7fa1dfd8164e`.
    if uuid.len() != 36 { return None }
    if uuid.as_bytes()[8] != b'-' { return None }
    if uuid.as_bytes()[13] != b'-' { return None }
    if uuid.as_bytes()[18] != b'-' { return None }
    if uuid.as_bytes()[23] != b'-' { return None }
    // We parse the first and last 4 bytes and use these as the 8-byte id.
    // See the comments above for the motivation for using only 64 of the 128
    // bits. We take the front and back of the string because it is easy, there
    // are no dashes to strip. Also, the non-random version bits are in the
    // middle, so this way we avoid using those.
    let high = u32::from_str_radix(&uuid[..8], 16).ok()? as u64;
    let low = u32::from_str_radix(&uuid[28..], 16).ok()? as u64;
    Some((high << 32) | low)
}

/// Return an issue if the two albums are not equal.
pub fn albums_different(
    strings: &StringDeduper,
    id: AlbumId,
    a: &Album,
    b: &Album)
    -> Option<IssueDetail>
{
    let title_a = strings.get(a.title.0);
    let title_b = strings.get(b.title.0);

    if title_a != title_b {
        return Some(IssueDetail::AlbumTitleMismatch(
            id,
            title_a.into(),
            title_b.into(),
        ));
    }

    if a.original_release_date != b.original_release_date {
        return Some(IssueDetail::AlbumReleaseDateMismatch(
            id,
            a.original_release_date,
            b.original_release_date,
        ));
    }

    if a.loudness != b.loudness {
        return Some(IssueDetail::AlbumLoudnessMismatch(
            id,
            a.loudness,
            b.loudness,
        ));
    }

    if a.artist_id != b.artist_id {
        return Some(IssueDetail::AlbumArtistMismatch(
            id,
            a.artist_id,
            b.artist_id,
        ));
    }

    None
}

/// Return an issue if the two artists are not equal.
pub fn artists_different(
    strings: &StringDeduper,
    id: ArtistId,
    a: &Artist,
    b: &Artist)
    -> Option<IssueDetail>
{
    let name_a = strings.get(a.name.0);
    let name_b = strings.get(b.name.0);
    let sort_name_a = strings.get(a.name_for_sort.0);
    let sort_name_b = strings.get(b.name_for_sort.0);

    if name_a != name_b {
        return Some(IssueDetail::ArtistNameMismatch(
            id,
            name_a.into(),
            name_b.into(),
        ));
    }

    if sort_name_a != sort_name_b {
        return Some(IssueDetail::ArtistSortNameMismatch(
            id,
            sort_name_a.into(),
            sort_name_b.into(),
        ));
    }

    None
}

pub struct BuildMetaIndex {
    pub artists: BTreeMap<ArtistId, Artist>,
    pub albums: BTreeMap<AlbumId, Album>,
    pub tracks: BTreeMap<TrackId, Track>,
    pub strings: StringDeduper,
    pub filenames: Vec<String>,

    pub words_artist: BTreeSet<(String, ArtistId, WordMeta)>,
    pub words_album: BTreeSet<(String, AlbumId, WordMeta)>,
    pub words_track: BTreeSet<(String, TrackId, WordMeta)>,

    // For album and artist data, store the file from which the metadata was
    // taken. This is later used for error reporting when the builders are
    // merged. If there are inconsistencies at that point, we need to be able to
    // attribute the album or artist metadata in this builder to a specific
    // file. If all files agree then it's an arbitrary one, but if there was
    // already inconsistent data in this builder's input, we need to remember
    // which file we chose. Values are indices into the `filenames` vector.
    pub album_sources: HashMap<AlbumId, FilenameRef>,
    pub artist_sources: HashMap<ArtistId, FilenameRef>,

    // TODO: This option, to drop it when processing is done, is a bit of a
    // hack. It would be nice to not have it in the builder at all.
    pub progress: Option<SyncSender<Progress>>,
}

impl BuildMetaIndex {
    pub fn new(progress: SyncSender<Progress>) -> BuildMetaIndex {
        BuildMetaIndex {
            artists: BTreeMap::new(),
            albums: BTreeMap::new(),
            tracks: BTreeMap::new(),
            strings: StringDeduper::new(),
            filenames: Vec::new(),
            words_artist: BTreeSet::new(),
            words_album: BTreeSet::new(),
            words_track: BTreeSet::new(),
            album_sources: HashMap::new(),
            artist_sources: HashMap::new(),
            progress: Some(progress),
        }
    }

    fn issue(&mut self, filename: String, detail: IssueDetail) {
        let issue = detail.for_file(filename);
        self.progress.as_mut().unwrap().send(Progress::Issue(issue)).unwrap();
    }

    fn error_missing_field(&mut self, filename: String, field: &'static str) {
        self.issue(filename, IssueDetail::FieldMissingError(field));
    }

    fn warning_missing_field(&mut self, filename: String, field: &'static str) {
        self.issue(filename, IssueDetail::FieldMissingWarning(field));
    }

    fn warning_track_title_contains_feat(&mut self, filename: String) {
        self.issue(filename, IssueDetail::TrackTitleContainsFeat);
    }

    fn error_parse_failed(&mut self, filename: String, field: &'static str) {
        self.issue(filename, IssueDetail::FieldParseFailedError(field));
    }

    fn error_not_stereo(&mut self, filename: String) {
        self.issue(filename, IssueDetail::NotStereo);
    }

    fn error_unsupported_bit_depth(&mut self, filename: String, bits: u32) {
        self.issue(filename, IssueDetail::UnsupportedBitDepth(bits));
    }

    pub fn insert(
        &mut self,
        file: FileMetadata,
    ) {
        let filename_id = self.filenames.len() as u32;
        let filename_string = file.filename;

        // It simplifies many things for playback if I can assume that all files
        // are stereo, so reject any non-stereo files. At the time of writing,
        // all 16k tracks in my library are stereo. The same holds for bit
        // depths, in practice 16 or 24 bits per sample are used, so for
        // playback I only support these.
        if file.streaminfo_channels != 2 {
            return self.error_not_stereo(filename_string);
        }
        match file.streaminfo_bits_per_sample {
            16 => { /* Ok, supported. */ }
            24 => { /* Ok, supported. */ }
            n => return self.error_unsupported_bit_depth(filename_string, n as u32),
        }

        // TODO: Potentially we could move ownership, now that the `file` has
        // `String` rather than `&str`.
        let title = file.tag_title.map(|v| self.strings.insert(&v));
        let artist = file.tag_artist.map(|v| self.strings.insert(&v));
        let album = file.tag_album.map(|v| self.strings.insert(&v));
        let album_artist = file.tag_albumartist.map(|v| self.strings.insert(&v));
        let album_artist_for_sort = file.tag_albumartistsort.map(|v| self.strings.insert(&v));

        let disc_number = match file.tag_discnumber {
            Some(v) => match u8::from_str(&v) {
                Ok(n) => n,
                Err(..) => return self.error_parse_failed(filename_string, "discnumber"),
            }
            // If the disc number is not set, assume disc 1.
            None => 1,
        };

        let track_number = match file.tag_tracknumber {
            Some(v) => match u8::from_str(&v) {
                Ok(n) => n,
                Err(..) => return self.error_parse_failed(filename_string, "tracknumber"),
            }
            None => return self.error_missing_field(filename_string, "tracknumber"),
        };

        let mbid_artist = match file.tag_musicbrainz_albumartistid {
            Some(v) => match parse_uuid(&v) {
                Some(id) => id,
                None => return self.error_parse_failed(filename_string, "musicbrainz_albumartistid"),
            }
            None => return self.error_missing_field(filename_string, "musicbrainz_albumartistid"),
        };
        let mbid_album = match file.tag_musicbrainz_albumid {
            Some(v) => match parse_uuid(&v) {
                Some(id) => id,
                None => return self.error_parse_failed(filename_string, "musicbrainz_albumid"),
            }
            None => return self.error_missing_field(filename_string, "musicbrainz_albumid"),
        };

        let original_date = match file.tag_originaldate {
            Some(v) => match parse_date(&v) {
                Some(date) => Some(date),
                None => return self.error_parse_failed(filename_string, "originaldate"),
            }
            None => None
        };
        let date = match file.tag_date {
            Some(v) => match parse_date(&v) {
                Some(date) => Some(date),
                None => return self.error_parse_failed(filename_string, "date"),
            }
            None => None
        };

        let track_loudness = match file.tag_bs17704_track_loudness {
            Some(v) => match Lufs::from_str(&v) {
                Ok(lufs) => Some(lufs),
                // Unfortunately we have no way to include more details
                // about the parse failure with the error message at this
                // point.
                Err(_) => return self.error_parse_failed(filename_string, "bs17704_track_loudness"),
            }
            None => None,
        };
        let album_loudness = match file.tag_bs17704_album_loudness {
            Some(v) => match Lufs::from_str(&v) {
                Ok(lufs) => Some(lufs),
                Err(_) => return self.error_parse_failed(filename_string, "bs17704_track_loudness"),
            }
            None => None,
        };

        let f_title = match title {
            Some(t) => t,
            None => return self.error_missing_field(filename_string, "title"),
        };
        let f_track_artist = match artist {
            Some(a) => a,
            None => return self.error_missing_field(filename_string, "artist"),
        };
        let f_album = match album {
            Some(a) => a,
            None => return self.error_missing_field(filename_string, "album"),
        };
        let f_album_artist = match album_artist {
            Some(a) => a,
            None => return self.error_missing_field(filename_string, "albumartist"),
        };

        // Use the 'originaldate' field, fall back to 'date' if it is not set.
        let f_date = match original_date.or(date) {
            Some(d) => d,
            None => return self.error_missing_field(filename_string, "originaldate"),
        };

        // Emit a warning when loudness is not present. Emit only one of the two
        // warnings, because it is likely that both are absent, and then you get
        // two warnings per file, which is extremely noisy.
        if track_loudness.is_none() {
            self.warning_missing_field(filename_string.clone(), "bs17704_track_loudness");
        }
        else if album_loudness.is_none() {
            self.warning_missing_field(filename_string.clone(), "bs17704_album_loudness");
        }

        // Warn about track titles containing "(feat. ", these should probably
        // be in the artist metadata instead.
        {
            let track_title = &self.strings.get(f_title);
            if track_title.contains("(feat. ") {
                self.warning_track_title_contains_feat(filename_string.clone());
            }
        }

        let artist_id = ArtistId(mbid_artist);
        let album_id = AlbumId(mbid_album);
        let track_id = get_track_id(album_id, disc_number, track_number);

        // Split the title, album, and album artist, on words, and add those to
        // the indexes, to allow finding the track/album/artist later by word.
        let mut words = Vec::new();
        let mut words_album_artist = Vec::new();

        {
            let track_title = &self.strings.get(f_title);
            let album_title = &self.strings.get(f_album);
            let album_artist = &self.strings.get(f_album_artist);
            let track_artist = &self.strings.get(f_track_artist);

            // Fill the indexes with the words that occur in the titles. The artist
            // is also present in the album and track indexes, but with rank 0, such
            // that including the artist in the search terms would not make the
            // intersection empty.
            normalize_words(album_artist, &mut words_album_artist);
            for (i, w) in words_album_artist.iter().enumerate() {
                let meta_rank_2 = WordMeta::new(w.len(), album_artist.len(), i, 2);
                let meta_rank_0 = WordMeta::new(w.len(), album_artist.len(), i, 0);
                self.words_artist.insert((w.clone(), artist_id, meta_rank_2));
                self.words_album.insert((w.clone(),  album_id,  meta_rank_0));
                self.words_track.insert((w.clone(),  track_id,  meta_rank_0));
            }
            normalize_words(album_title, &mut words);
            for (i, w) in words.drain(..).enumerate() {
                let meta_rank_2 = WordMeta::new(w.len(), album_title.len(), i, 2);
                self.words_album.insert((w, album_id, meta_rank_2));
            }
            normalize_words(track_title, &mut words);
            for (i, w) in words.drain(..).enumerate() {
                let meta_rank_2 = WordMeta::new(w.len(), track_title.len(), i, 2);
                self.words_track.insert((w, track_id, meta_rank_2));
            }

            // Extend the track index with the words that occur uniquely in the
            // track artist, and not in the album artist. For example, feat.
            // artists, but also the full artist on complication albums. These get
            // rank 1 to set them apart from album artist words (rank 0) and title
            // words (rank 2).
            normalize_words(track_artist, &mut words);
            for (i, w) in words.drain(..).enumerate() {
                if !words_album_artist.contains(&w) {
                    let meta_rank_1 = WordMeta::new(w.len(), track_artist.len(), i, 1);
                    self.words_track.insert((w, track_id, meta_rank_1));
                }
            }
        }

        // Normalize the sort artist too. Generally, the only thing it is useful
        // for is to turn e.g. "The Who" into "Who, The". (Data from Musicbrainz
        // also puts the last name first for artists who use their real name,
        // but I dislike this.) But this is not sufficient for sorting alone:
        // there can still be case differences (e.g. "dEUS" and "deadmau5"
        // sorting last because they are lowercase) and accents (e.g. "Étienne
        // de Crécy" sorting last, and not with the "E"). The correct sort
        // ordering depends on locale. I am going to ignore all of that and turn
        // characters into the lowercase ascii character that looks most like
        // it, then sort by that.
        // TODO: Avoid inserting non-normalized sort artist in the string
        // deduplicator above; the non-normalized string is never referenced
        // except here temporarily.
        normalize_words(&self.strings.get(album_artist_for_sort.unwrap_or(f_album_artist)), &mut words);
        let sort_artist = words.join(" ");
        let f_album_artist_for_sort = self.strings.insert(&sort_artist);
        words.clear();

        // TODO: Check for u16 overflow.
        // TODO: Warn if `streaminfo.samples` is None.
        let samples = file.streaminfo_num_samples.unwrap_or(0) as u64;
        let samples_per_sec = file.streaminfo_sample_rate as u64;
        // Compute the duration in seconds. Add half the denominator in order to
        // round properly.
        let seconds = (samples + samples_per_sec / 2) / samples_per_sec;

        let track = Track {
            album_id: album_id,
            disc_number: disc_number,
            track_number: track_number,
            title: StringRef(f_title),
            artist: StringRef(f_track_artist),
            duration_seconds: seconds as u16,
            filename: FilenameRef(filename_id),
            loudness: track_loudness,
        };
        let album = Album {
            artist_id: artist_id,
            title: StringRef(f_album),
            original_release_date: f_date,
            loudness: album_loudness,
        };
        let artist = Artist {
            name: StringRef(f_album_artist),
            name_for_sort: StringRef(f_album_artist_for_sort),
        };

        let mut add_album = true;
        let mut add_artist = true;

        // Check for consistency if duplicates occur.
        if self.tracks.get(&track_id).is_some() {
            // TODO: This should report an `Issue`, not panic.
            panic!("Duplicate track {}, file {}.", track_id, filename_string);
        }

        if let Some(existing_album) = self.albums.get(&album_id) {
            if let Some(detail) = albums_different(&self.strings, album_id, existing_album, &album) {
                let issue = detail.for_file(filename_string.clone());
                self.progress.as_mut().unwrap().send(Progress::Issue(issue)).unwrap();
            }
            add_album = false;
        }

        if let Some(existing_artist) = self.artists.get(&artist_id) {
            if let Some(detail) = artists_different(&self.strings, artist_id, existing_artist, &artist) {
                let issue = detail.for_file(filename_string.clone());
                self.progress.as_mut().unwrap().send(Progress::Issue(issue)).unwrap();
            }
            add_artist = false;
        }

        self.filenames.push(filename_string);
        self.tracks.insert(track_id, track);

        if add_album {
            self.albums.insert(album_id, album);
            self.album_sources.insert(album_id, FilenameRef(filename_id));
        }

        if add_artist {
            self.artists.insert(artist_id, artist);
            self.artist_sources.insert(artist_id, FilenameRef(filename_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Date};
    use super::{parse_date};

    #[test]
    fn parse_date_parses_year() {
        assert_eq!(parse_date("2018"), Some(Date::new(2018, 0, 0)));
        assert_eq!(parse_date("1970"), Some(Date::new(1970, 0, 0)));
        assert_eq!(parse_date("572"), None);
        assert_eq!(parse_date("-572"), None);
        assert_eq!(parse_date("MMXVIII"), None);
        assert_eq!(parse_date("2018a"), None);
    }

    #[test]
    fn parse_date_parses_month() {
        assert_eq!(parse_date("2018-01"), Some(Date::new(2018, 1, 0)));
        assert_eq!(parse_date("2018-12"), Some(Date::new(2018, 12, 0)));
        assert_eq!(parse_date("2018-42"), None);
        assert_eq!(parse_date("2018 12"), None);
        assert_eq!(parse_date("2018-3"), None);
        assert_eq!(parse_date("2018-03a"), None);
    }

    #[test]
    fn parse_date_parses_day() {
        assert_eq!(parse_date("2018-01-01"), Some(Date::new(2018, 1, 1)));
        assert_eq!(parse_date("2018-01-31"), Some(Date::new(2018, 1, 31)));
        assert_eq!(parse_date("2018-01-32"), None);
        assert_eq!(parse_date("2018-01 01"), None);
        assert_eq!(parse_date("2018-01-1"), None);
        assert_eq!(parse_date("2018-01-01a"), None);
    }

    #[test]
    fn format_date_formats_year_only() {
        assert_eq!(format!("{}", Date::new(2018, 0, 0)), "2018");
    }

    #[test]
    fn format_date_formats_year_and_month() {
        assert_eq!(format!("{}", Date::new(2018, 1, 0)), "2018-01");
    }

    #[test]
    fn format_date_formats_year_and_month_and_day() {
        assert_eq!(format!("{}", Date::new(2018, 1, 2)), "2018-01-02");
    }
}
