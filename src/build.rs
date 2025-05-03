// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::str::FromStr;

use crate::database::{self as db, FileMetadata, Transaction};
use crate::prim::{
    Album, AlbumArtistsRef, AlbumId, Artist, ArtistId, Color, Date, FileId, FilenameRef, Instant,
    Lufs, StringRef, Track, TrackId,
};
use crate::string_utils::{normalize_words, StringDeduper};
use crate::word_index::WordMeta;

pub enum BuildError {
    /// Something went wrong interacting with the database.
    DbError(sqlite::Error),

    /// The file was not inserted.
    ///
    /// The actual error is reported separately as `Issue` in the
    /// `BuildMetaIndex`.
    FileFailed,
}

type Result<T> = std::result::Result<T, BuildError>;

impl From<sqlite::Error> for BuildError {
    fn from(err: sqlite::Error) -> BuildError {
        BuildError::DbError(err)
    }
}

#[derive(Clone, Debug)]
pub enum IssueDetail {
    /// A required metadata field is missing. Contains the field name.
    FieldMissingError(&'static str),

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
    AlbumArtistMismatch(AlbumId, Option<ArtistId>, Option<ArtistId>),

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
    pub fn for_file(self, filename: &str) -> Issue {
        Issue {
            filename: filename.to_string(),
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
    #[rustfmt::skip]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:\n  ", self.filename)?;
        match self.detail {
            IssueDetail::FieldMissingError(field) =>
                write!(f, "error: field '{}' missing.", field),
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
            IssueDetail::AlbumArtistMismatch(_id, Some(ref artist), Some(ref alt)) =>
                write!(f, "warning: discarded inconsistent album artist '{}' in favour of '{}'.", alt, artist),
            IssueDetail::AlbumArtistMismatch(_id, Some(ref artist), None) =>
                write!(f, "warning: album artist '{}' is not consistently present.", artist),
            IssueDetail::AlbumArtistMismatch(_id, None, Some(ref alt)) =>
                write!(f, "warning: discarded excess album artist '{}'.", alt),
            IssueDetail::AlbumArtistMismatch(_id, None, None) =>
                panic!("This error case should not be generated."),
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

fn parse_date(date_str: &str) -> Option<Date> {
    // We expect at least a year.
    if date_str.len() < 4 {
        return None;
    }

    let year = u16::from_str(&date_str[0..4]).ok()?;
    let mut month: u8 = 0;
    let mut day: u8 = 0;

    // If there is something following the year, it must be dash, and there must
    // be at least two digits for the month.
    if date_str.len() > 4 {
        if date_str.as_bytes()[4] != b'-' {
            return None;
        }
        if date_str.len() < 7 {
            return None;
        }
        month = u8::from_str(&date_str[5..7]).ok()?;
    }

    // If there is something following the month, it must be dash, and there
    // must be exactly two digits for the day.
    if date_str.len() > 7 {
        if date_str.as_bytes()[7] != b'-' {
            return None;
        }
        if date_str.len() != 10 {
            return None;
        }
        day = u8::from_str(&date_str[8..10]).ok()?;
    }

    // This is not the most strict date well-formedness check that we can do,
    // but it is something at least. Note that we do allow the month and day to
    // be zero, to indicate the entire month or entire year.
    if month > 12 || day > 31 {
        return None;
    }

    Some(Date::new(year, month, day))
}

/// Parse a part of a 128-bit hexadecimal UUID into a 64-bit unsigned integer.
fn parse_uuid(uuid: &str) -> Option<u64> {
    // Validate that the textual format of the UUID is as expected.
    // E.g. `1070cbb2-ad74-44ce-90a4-7fa1dfd8164e`.
    if uuid.len() != 36 {
        return None;
    }
    if uuid.as_bytes()[8] != b'-' {
        return None;
    }
    if uuid.as_bytes()[13] != b'-' {
        return None;
    }
    if uuid.as_bytes()[18] != b'-' {
        return None;
    }
    if uuid.as_bytes()[23] != b'-' {
        return None;
    }
    // We parse the first and last 4 bytes and use these as the 8-byte id.
    // See the comments above for the motivation for using only 64 of the 128
    // bits. We take the front and back of the string because it is easy, there
    // are no dashes to strip. Also, the non-random version bits are in the
    // middle, so this way we avoid using those.
    let high = u32::from_str_radix(&uuid[..8], 16).ok()? as u64;
    let low = u32::from_str_radix(&uuid[28..], 16).ok()? as u64;
    Some((high << 32) | low)
}

/// Like `parse_uuid`, but take only 52 bits. This is used for album ids.
///
/// On purpose, we still take the digits from the beginning and end of the
/// uuid, such that the uuid can easily be compared from begin or end to our
/// internal id. We don't just shift the `parse_uuid` result, because then
/// either the start or end of the hex-formatted id would no longer match the
/// hex-formatted uuid.
pub fn parse_uuid_52bits(uuid: &str) -> Option<u64> {
    // See also the comments in `parse_uuid`
    if uuid.len() != 36 {
        return None;
    }
    if uuid.as_bytes()[8] != b'-' {
        return None;
    }
    if uuid.as_bytes()[13] != b'-' {
        return None;
    }
    if uuid.as_bytes()[18] != b'-' {
        return None;
    }
    if uuid.as_bytes()[23] != b'-' {
        return None;
    }
    let high = u32::from_str_radix(&uuid[..8], 16).ok()? as u64;
    let low = u32::from_str_radix(&uuid[31..], 16).ok()? as u64;
    Some((high << 20) | low)
}

/// Return an issue if the two albums are not equal.
pub fn albums_different(
    strings: &StringDeduper,
    album_artists: &AlbumArtistsDeduper,
    id: AlbumId,
    a: &Album,
    b: &Album,
) -> Option<IssueDetail> {
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
            id, a.loudness, b.loudness,
        ));
    }

    let mut a_artists = album_artists.get(a.artist_ids).iter();
    let mut b_artists = album_artists.get(b.artist_ids).iter();

    loop {
        match (a_artists.next(), b_artists.next()) {
            (Some(a_aid), Some(b_aid)) if *a_aid == *b_aid => continue,
            (None, None) => break,
            (opt_a_id, opt_b_id) => {
                return Some(IssueDetail::AlbumArtistMismatch(
                    id,
                    opt_a_id.cloned(),
                    opt_b_id.cloned(),
                ))
            }
        }
    }

    None
}

/// Return an issue if the two artists are not equal.
pub fn artists_different(
    strings: &StringDeduper,
    id: ArtistId,
    a: &Artist,
    b: &Artist,
) -> Option<IssueDetail> {
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

pub struct AlbumArtistsDeduper {
    pub artists: Vec<ArtistId>,
    pub refs: HashMap<u64, AlbumArtistsRef>,
}

impl AlbumArtistsDeduper {
    pub fn new() -> Self {
        AlbumArtistsDeduper {
            artists: Vec::new(),
            refs: HashMap::new(),
        }
    }

    pub fn insert<I: IntoIterator<Item = ArtistId>>(&mut self, ids: I) -> AlbumArtistsRef {
        use std::collections::hash_map::Entry::{Occupied, Vacant};

        // Store all the artist ids in the backing vector and keep track of the
        // new range. Later we might undo this. Also while iterating, compute a
        // hash of all ids. Artist ids are already random (taken from a uuid)
        // and uniformly distributed, so a simple xor should suffice.
        let mut h = 0;
        let begin = self.artists.len();
        for id in ids {
            h ^= id.0;
            self.artists.push(id);
        }
        let end = self.artists.len();

        let range = AlbumArtistsRef {
            begin: begin as u32,
            end: end as u32,
        };

        match self.refs.entry(h) {
            Occupied(existing) => {
                let e = *existing.get();
                if self.artists[e.begin as usize..e.end as usize] == self.artists[begin..end] {
                    // We found an existing range, and it's the same. We don't need
                    // the one we just inserted, they were already there.
                    self.artists.truncate(begin);
                    *existing.get()
                } else {
                    // We have a collision, the wrong id is at this index. Then
                    // we'll just duplicate the entry, and have it backed twice.
                    range
                }
            }
            Vacant(vacancy) => *vacancy.insert(range),
        }
    }

    /// Return the underlying vector, destroying the deduplicator.
    pub fn into_vec(self) -> Vec<ArtistId> {
        self.artists
    }

    /// Return the artists with the given index.
    pub fn get(&self, ids: AlbumArtistsRef) -> &[ArtistId] {
        &self.artists[ids.begin as usize..ids.end as usize]
    }
}

pub struct BuildMetaIndex {
    pub artists: BTreeMap<ArtistId, Artist>,
    pub albums: BTreeMap<AlbumId, Album>,
    pub tracks: BTreeMap<TrackId, Track>,
    pub album_artists: AlbumArtistsDeduper,

    pub strings: StringDeduper,
    pub filenames: Vec<String>,

    pub words_artist: BTreeSet<(String, ArtistId, WordMeta)>,
    pub words_album: BTreeSet<(String, AlbumId, WordMeta)>,
    pub words_track: BTreeSet<(String, TrackId, WordMeta)>,

    /// The maximum file id of all files in the album.
    ///
    /// This is used to invalidate any existing album loudness, in case a
    /// file in the album gets updated.
    /// TODO: This is not actually used yet, invalidate the loudness based on
    /// it in the `loudness` module. Probably we could return the file id when
    /// we select the loudness.
    pub album_file_ids: HashMap<AlbumId, FileId>,

    /// The first (oldest) recorded listen for the albums in this map.
    pub album_first_listens: HashMap<AlbumId, Instant>,

    /// File name of the file currently being inserted.
    ///
    /// This is used to simplify helper methods for error reporting, to ensure
    /// that we don't have to pass the file name around everywhere.
    pub current_filename: FilenameRef,

    /// Issues collected while inserting into the builder.
    pub issues: Vec<Issue>,
}

pub struct FileTask {
    file_id: FileId,
    filename: FilenameRef,
    mtime: Instant,
    duration_seconds: u16,
}

impl BuildMetaIndex {
    pub fn new() -> BuildMetaIndex {
        BuildMetaIndex {
            artists: BTreeMap::new(),
            albums: BTreeMap::new(),
            tracks: BTreeMap::new(),
            album_artists: AlbumArtistsDeduper::new(),
            strings: StringDeduper::new(),
            filenames: Vec::new(),
            album_file_ids: HashMap::new(),
            album_first_listens: HashMap::new(),
            words_artist: BTreeSet::new(),
            words_album: BTreeSet::new(),
            words_track: BTreeSet::new(),
            // Initially we set this to a sentinel value even though we don't
            // have a backing file yet; dereferencing this should not happen.
            current_filename: FilenameRef(0),
            issues: Vec::new(),
        }
    }

    fn get_current_filename(&self) -> &str {
        &self.filenames[self.current_filename.0 as usize]
    }

    /// Push an issue, then return `Err`.
    ///
    /// Returning a `Result` is useful for chaining with ? so we can early out.
    fn issue<T>(&mut self, detail: IssueDetail) -> Result<T> {
        let issue = detail.for_file(self.get_current_filename());
        self.issues.push(issue);
        Err(BuildError::FileFailed)
    }

    fn error_missing_field<T>(&mut self, field: &'static str) -> Result<T> {
        self.issue(IssueDetail::FieldMissingError(field))
    }

    fn warning_track_title_contains_feat(&mut self) {
        let _ = self.issue::<()>(IssueDetail::TrackTitleContainsFeat);
    }

    fn error_parse_failed<T>(&mut self, field: &'static str) -> Result<T> {
        self.issue(IssueDetail::FieldParseFailedError(field))
    }

    fn error_not_stereo<T>(&mut self) -> Result<T> {
        self.issue(IssueDetail::NotStereo)
    }

    fn error_unsupported_bit_depth<T>(&mut self, bits: u32) -> Result<T> {
        self.issue(IssueDetail::UnsupportedBitDepth(bits))
    }

    /// Parse the value, report an issue if parse failed.
    ///
    /// When the outer option is none, there was a fatal parse error. When the
    /// inner option is none, the value was absent.
    #[inline(always)]
    fn parse<T, F: FnOnce(&String) -> Option<T>>(
        &mut self,
        field: &'static str,
        value: Option<&String>,
        parse: F,
    ) -> Result<Option<T>> {
        match value {
            Some(v) => match parse(v) {
                Some(result) => Ok(Some(result)),
                None => self.error_parse_failed(field),
            },
            None => Ok(None),
        }
    }

    /// Parse the value, report an issue if it is absent, or parse failed.
    #[inline(always)]
    fn require_and_parse<T, F: FnOnce(&String) -> Option<T>>(
        &mut self,
        field: &'static str,
        value: Option<&String>,
        parse: F,
    ) -> Result<T> {
        match self.parse(field, value, parse)? {
            Some(v) => Ok(v),
            None => self.error_missing_field(field),
        }
    }

    /// Deduplicate the string and get a string ref, if the value is present.
    #[inline(always)]
    fn require_and_insert_string(
        &mut self,
        field: &'static str,
        value: Option<String>,
    ) -> Result<u32> {
        match value {
            // TODO: We could potentially make `strings` take the `String`
            // rather than the ref, now that we have this owned data.
            Some(v) => Ok(self.strings.insert(&v)),
            None => self.error_missing_field(field),
        }
    }

    /// Perform the first step of insertion, based on only the information from
    /// the `files` table, not yet joined with other tables.
    pub fn insert_meta(&mut self, file: FileMetadata) -> Result<FileTask> {
        let filename_id = FilenameRef(self.filenames.len() as u32);
        self.filenames.push(file.filename);
        self.current_filename = filename_id;

        // It simplifies many things for playback if I can assume that all files
        // are stereo, so reject any non-stereo files. At the time of writing,
        // all 16k tracks in my library are stereo. The same holds for bit
        // depths, in practice 16 or 24 bits per sample are used, so for
        // playback I only support these.
        if file.streaminfo_channels != 2 {
            return self.error_not_stereo();
        }
        match file.streaminfo_bits_per_sample {
            16 => { /* Ok, supported. */ }
            24 => { /* Ok, supported. */ }
            n => return self.error_unsupported_bit_depth(n as u32),
        }

        let samples = match file.streaminfo_num_samples {
            Some(s) => s as u64,
            // TODO: Add a proper error for this, if it occurs in practice.
            None => panic!("Streaminfo does not contain duration."),
        };
        let samples_per_sec = file.streaminfo_sample_rate as u64;
        // Compute the duration in seconds. Add half the denominator in order to
        // round properly.
        let seconds = (samples + samples_per_sec / 2) / samples_per_sec;

        if seconds > u16::MAX as u64 {
            // TODO: Add a proper error for this, if it occurs in practice.
            panic!("Track is longer than {} seconds.", u16::MAX);
        }

        let result = FileTask {
            file_id: FileId(file.id),
            filename: filename_id,
            mtime: Instant {
                posix_seconds_utc: file.mtime,
            },
            duration_seconds: seconds as u16,
        };

        Ok(result)
    }

    /// Complete inserting a file, now consulting the additional tables to get
    /// tags and loudness information.
    #[rustfmt::skip]
    pub fn insert_full(&mut self, tx: &mut Transaction, file: FileTask) -> Result<()> {
        self.current_filename = file.filename;
        let file_id = file.file_id;

        let mut tag_date = None;
        let mut tag_discnumber = None;
        let mut tag_musicbrainz_albumid = None;
        let mut tag_musicbrainz_albumartistid = Vec::new();
        let mut tag_originaldate = None;
        let mut tag_tracknumber = None;
        let mut tag_title = None;
        let mut tag_artist = None;
        let mut tag_album = None;
        let mut tag_albumartist = None;
        let mut tag_albumartistsort = None;
        let mut tag_albumartists = Vec::new();
        let mut tag_albumartistssort = Vec::new();

        for opt_pair in db::iter_file_tags(tx, file.file_id.0)? {
            let (field_name, value) = opt_pair?;
            // Note, we lowercase field names when inserting into the database,
            // so we only have to match on the lowercase ones here.
            match &field_name[..] {
                "album" => tag_album = Some(value),
                "albumartist" => tag_albumartist = Some(value),
                "albumartists" => tag_albumartists.push(value),
                "albumartistsort" => tag_albumartistsort = Some(value),
                "albumartistssort" => tag_albumartistssort.push(value),
                "artist" => tag_artist = Some(value),
                "artists" => continue, // Currently unused.
                "date" => tag_date = Some(value),
                "discnumber" => tag_discnumber = Some(value),
                "musicbrainz_albumartistid" => tag_musicbrainz_albumartistid.push(value),
                "musicbrainz_albumid" => tag_musicbrainz_albumid = Some(value),
                "musicbrainz_trackid" => continue, // Currently unused.
                "originaldate" => tag_originaldate = Some(value),
                "title" => tag_title = Some(value),
                "tracknumber" => tag_tracknumber = Some(value),
                other => panic!("Found unsupported tag in database: {}", other),
            }
        }

        let track_number = self.require_and_parse(
            "tracknumber",
            tag_tracknumber.as_ref(),
            |v| u8::from_str(v).ok(),
        )?;
        let disc_number = self.parse(
            "discnumber",
            tag_discnumber.as_ref(),
            |v| u8::from_str(v).ok(),
        )?;
        // If the disc number is not set, assume disc 1.
        let disc_number = disc_number.unwrap_or(1);

        let mbid_album = self.require_and_parse(
            "musicbrainz_albumid",
            tag_musicbrainz_albumid.as_ref(),
            |v| parse_uuid_52bits(v),
        )?;

        let original_date = self.parse(
            "originaldate",
            tag_originaldate.as_ref(),
            |v| parse_date(v),
        )?;
        let date = self.parse(
            "date",
            tag_date.as_ref(),
            |v| parse_date(v),
        )?;

        // Use the 'originaldate' field, fall back to 'date' if it is not set.
        let release_date = match original_date.or(date) {
            Some(d) => d,
            None => return self.error_missing_field("originaldate"),
        };

        let title = self.require_and_insert_string("title", tag_title)?;
        let track_artist = self.require_and_insert_string("artist", tag_artist)?;
        let album = self.require_and_insert_string("album", tag_album)?;
        let album_artist = self.require_and_insert_string("albumartist", tag_albumartist)?;

        // The "albumartists" tag is optional, when it is not provided, we
        // default to the single album artist.
        if tag_albumartists.is_empty() {
            tag_albumartists.push(self.strings.get(album_artist).to_string());
        }

        // The album artist sort name can be omitted, in that case we default
        // to the album artist name. When it's not omitted, it must be provided
        // for every artist.
        let tag_albumartistssort = match tag_albumartistssort.len() {
            0 => match (tag_albumartists.len(), &tag_albumartistsort) {
                (1, Some(aa_sort)) => vec![aa_sort.clone()],
                _ => tag_albumartists.clone(),
            },
            _ => tag_albumartistssort,
        };

        // Album artist id, name, and sort name.
        let mut album_artists: Vec<(ArtistId, StringRef, StringRef)> = Vec::new();
        for ((tag_aa_mbid, tag_aa_name), tag_aa_name_sort) in tag_musicbrainz_albumartistid
            .iter()
            .zip(tag_albumartists)
            .zip(tag_albumartistssort) {
            let mbid_artist = self.require_and_parse(
                "musicbrainz_albumartistid",
                Some(tag_aa_mbid),
                |v| parse_uuid(v),
            )?;
            let aa_name = self.strings.insert(&tag_aa_name);
            let aa_name_sort = self.strings.insert(&tag_aa_name_sort);
            album_artists.push((
                ArtistId(mbid_artist),
                StringRef(aa_name),
                StringRef(aa_name_sort),
            ));
        }

        // Warn about track titles containing "(feat. ", these should probably
        // be in the artist metadata instead.
        {
            let track_title = &self.strings.get(title);
            if track_title.contains("(feat. ") {
                self.warning_track_title_contains_feat();
            }
        }

        let album_id = AlbumId(mbid_album);
        let track_id = TrackId::new(album_id, disc_number, track_number);

        // Record the maximum file id per album, so we can use it to invalidate
        // per-album data later.
        self.album_file_ids
            .entry(album_id)
            .and_modify(|v| *v = file_id.max(*v))
            .or_insert(file_id);

        // Split the title, album, and album artist, on words, and add those to
        // the indexes, to allow finding the track/album/artist later by word.
        let mut words = Vec::new();
        let mut words_album_artist = Vec::new();
        let mut all_words_album_artist = Vec::new();

        {
            // First we process all album artists individually.
            for &(artist_id, album_artist_i, _) in &album_artists {
                let album_artist_name = &self.strings.get(album_artist_i.0);
                // Fill the indexes with the words that occur in the name.
                // The artist is also present in the album and track indexes,
                // but with rank 0, such that including the artist in the search
                // terms would not make the intersection empty. For albums by
                // multiple artists, we make an exception and bump the rank,
                // such that you can still find the album by searching only for
                // the name of one of the artists.
                let k = if album_artists.len() == 1 { 0 } else { 1 };
                words_album_artist.clear();
                normalize_words(album_artist_name, &mut words_album_artist);
                for (i, w) in words_album_artist.drain(..).enumerate() {
                    let meta_rank_2 = WordMeta::new(w.len(), album_artist_name.len(), i, 2);
                    let meta_rank_k = WordMeta::new(w.len(), album_artist_name.len(), i, k);
                    let meta_rank_0 = WordMeta::new(w.len(), album_artist_name.len(), i, 0);
                    self.words_artist.insert((w.clone(), artist_id, meta_rank_2));
                    self.words_album.insert((w.clone(),  album_id,  meta_rank_k));
                    self.words_track.insert((w.clone(),  track_id,  meta_rank_0));
                    all_words_album_artist.push(w);
                }
            }

            // If the album has multiple artists, then the album artist as
            // credited may differ from the individual artists. For example,
            // the album artist can be "John Leged and The Roots" and the
            // individual album artists are "John Legend" and "The Roots". Then
            // the word "and" occurs in the album artist, but not in the
            // individual album artist names. We should insert the additional
            // word into the album and track list indexes, so that adding that
            // word to the search query does not exclude the album. And even
            // when there is a single artist, sometimes I prefer to to merge
            // multiple artists (e.g. "Robert Glasper" and "The Robert Glasper
            // Experiment") under the same mbid, but we can preserve the name as
            // credited on the album and make it searchable.
            let album_artist_full = self.strings.get(album_artist);
            normalize_words(album_artist_full, &mut words);
            for (i, w) in words.iter().enumerate() {
                if !all_words_album_artist.contains(w) {
                    let meta_rank_0 = WordMeta::new(w.len(), album_artist_full.len(), i, 0);
                    self.words_album.insert((w.clone(), album_id, meta_rank_0));
                    self.words_track.insert((w.clone(), track_id, meta_rank_0));
                }
            }
            // Add the words to the all collection only afterwards; if it
            // occurs twice in the album artist then it should be in the
            // index twice.
            all_words_album_artist.append(&mut words);

            let track_title = &self.strings.get(title);
            let album_title = &self.strings.get(album);
            let track_artist = &self.strings.get(track_artist);

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
            // artists, but also the full artist on compilation albums. These
            // get rank 1 to set them apart from album artist words (rank 0) and
            // title words (rank 2).
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
        for (_, _, ref mut aa_name_sort) in album_artists.iter_mut() {
            normalize_words(self.strings.get(aa_name_sort.0), &mut words);
            let sort_artist = words.join(" ");
            aa_name_sort.0 = self.strings.insert(&sort_artist);
            words.clear();
        }

        // TODO: It's inefficient to query the database once per track for the
        // album loudness.
        let track_loudness =
            db::select_track_loudness_lufs(tx, track_id.0 as i64)?.map(Lufs::from_f64);
        let album_loudness =
            db::select_album_loudness_lufs(tx, album_id.0 as i64)?.map(Lufs::from_f64);
        let album_color = db::select_album_color(tx, album_id.0 as i64)?
            .map(|c| Color::parse(&c).expect("Database should store only valid colors"));

        // Insert all the album artists if no artist with the given id existed
        // yet. If one did exist, verify consistency. Also fill the vector of
        // album artists so the album can refer to this.
        let album_artists_ref = self
            .album_artists
            .insert(album_artists.iter().map(|tuple| tuple.0));

        for (artist_id, aa_name, aa_name_sort) in album_artists {
            let artist = Artist {
                name: aa_name,
                name_for_sort: aa_name_sort,
            };
            match self.artists.get(&artist_id) {
                Some(existing_artist) => if let Some(detail) = artists_different(
                    &self.strings,
                    artist_id,
                    existing_artist,
                    &artist,
                ) {
                    let _ = self.issue::<()>(detail);
                }
                None => {
                    self.artists.insert(artist_id, artist);
                }
            }
        }

        let track = Track {
            file_id: file_id,
            title: StringRef(title),
            artist: StringRef(track_artist),
            duration_seconds: file.duration_seconds,
            filename: file.filename,
            loudness: track_loudness,
        };
        let mut album = Album {
            artist_ids: album_artists_ref,
            artist: StringRef(album_artist),
            title: StringRef(album),
            original_release_date: release_date,
            first_seen: file.mtime,
            loudness: album_loudness,
            color: album_color.unwrap_or_default(),
        };

        let mut add_album = true;

        // Check for consistency if duplicates occur.
        if self.tracks.get(&track_id).is_some() {
            // TODO: This should report an `Issue`, not panic.
            let filename = self.get_current_filename();
            panic!("Duplicate track {}, file {}.", track_id, filename);
        }

        if let Some(existing_album) = self.albums.get_mut(&album_id) {
            // If we have an existing album, take the max import date over all
            // files in that album. This is not a material difference for the
            // difference check below.
            let first_seen = album.first_seen.min(existing_album.first_seen);
            existing_album.first_seen = first_seen;
            album.first_seen = first_seen;

            if let Some(detail) = albums_different(
                &self.strings,
                &self.album_artists,
                album_id,
                existing_album,
                &album,
            ) {
                let _ = self.issue::<()>(detail);
            }
            add_album = false;
        }

        self.tracks.insert(track_id, track);

        if add_album {
            self.albums.insert(album_id, album);
        }

        Ok(())
    }

    /// Load the album's first listens from the `listens` table.
    pub fn insert_first_listens(&mut self, tx: &mut Transaction) -> db::Result<()> {
        // This does do a full table scan over all listens. But since I don't
        // import listening history yet, that's not so bad, on my laptop with a
        // cold cache it takes about 70ms to do 20k listens, on the Raspberry Pi
        // it will likely be slower, but still acceptable at startup.
        for row in db::iter_album_first_listens(tx)? {
            let (album_id_i64, started_at_iso8601) = row?;
            let album_id = AlbumId(album_id_i64 as u64);
            let started_at = match Instant::from_iso8601(&started_at_iso8601) {
                Some(t) => t,
                None => panic!(
                    "Encountered invalid started_at timestamp: {:?}",
                    started_at_iso8601
                ),
            };
            self.album_first_listens.insert(album_id, started_at);
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::{parse_date, Date};
    use super::{parse_uuid, parse_uuid_52bits};
    use super::{AlbumArtistsDeduper, ArtistId};

    #[test]
    fn parse_uuid_parses_uuid() {
        assert_eq!(
            parse_uuid("9c9f1380-2516-4fc9-a3e6-f9f61941d090"),
            Some(0x9c9f_1380_1941_d090)
        );
        assert_eq!(
            parse_uuid("056e4f3e-d505-4dad-8ec1-d04f521cbb56"),
            Some(0x056e_4f3e_521c_bb56)
        );
        assert_eq!(parse_uuid("nonsense"), None);
    }

    #[test]
    #[allow(clippy::unusual_byte_groupings)]
    fn parse_uuid_52bit_parses_uuid() {
        // Same as above, but note that we removed three hex digits in the middle.
        assert_eq!(
            parse_uuid_52bits("9c9f1380-2516-4fc9-a3e6-f9f61941d090"),
            Some(0x9c9f_1380_1_d090)
        );
        assert_eq!(
            parse_uuid_52bits("056e4f3e-d505-4dad-8ec1-d04f521cbb56"),
            Some(0x056e_4f3e_c_bb56)
        );
        assert_eq!(parse_uuid_52bits("nonsense"), None);
    }

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

    #[test]
    fn album_artists_deduper_works() {
        let mut dup = AlbumArtistsDeduper::new();
        let a = ArtistId(1);
        let b = ArtistId(2);
        let c = ArtistId(4);

        let a1 = dup.insert([a]);
        let a2 = dup.insert([a]);
        assert_eq!(a1, a2);

        let b1 = dup.insert([b]);
        assert_ne!(a1, b1);

        let ab1 = dup.insert([a, b]);
        let ac1 = dup.insert([a, c]);
        let ab2 = dup.insert([a, b]);
        let ac2 = dup.insert([a, c]);
        assert_eq!(ab1, ab2);
        assert_eq!(ac1, ac2);
    }
}
