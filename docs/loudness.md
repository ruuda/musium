# Loudness normalization

Musium can normalize the perceptual playback loudness based on loudness
information in flac tags.

The volume slider in Musium treats 0 dB volume as 0 dB gain for the output
device, but it does compensate for track or album loudness relative to a target
loudness of -23 <abbr>LUFS</abbr>. When the Musium volume slider is set to 0 dB,
this means that a track with a loudness of -23 <abbr>LUFS</abbr> will play back
without volume adjustment (at maximum volume of the device), and a track with a
loudness of -10 <abbr>LUFS</abbr> will play back with device volume at -13 dB.
When the Musium volume slider is set to -5 dB, a track with a loudness of -10
<abbr>LUFS</abbr> will play back with device volume at -18 dB.

It is possible to set volumes beyond 0 dB in Musium, but this is only effective
for inherently loud tracks: a track with a loudness of -23 <abbr>LUFS</abbr> is
already playing at the maximum device volume, but a track with a loudness of
-13 <abbr>LUFS</abbr> has enough headroom to allow the volume to be set to 10 dB.

## Tags

Two tags affect loudness normalization:

 * `BS17704_TRACK_LOUDNESS`
 * `BS17704_ALBUM_LOUDNESS`

The tags must store the integrated loudness as defined in
[<abbr>ITU-R BS.1770-4</abbr>][bs1770-4], for the track, and the concatenation
of all tracks respectively. The value must be a decimal number, followed by the
suffix “<abbr>LUFS</abbr>” for Loudness Units Full Scale. For example,
`-9.317 LUFS`. The value for album loudness must be consistent across all tracks
in the album.

[bs1770-4]: https://www.itu.int/rec/R-REC-BS.1770-4-201510-I/en

## ReplayGain

ReplayGain has been very influential for loudness normalization, and it is
widely supported by both taggers and players, but unfortunately it has become
ambiguous over time. ReplayGain does not store the loudness of the track
directly, instead it stores the *gain* that is needed to bring the track to
target loudness. The target loudness was initially well-defined, but tools
started offering different options to accomodate to the reference loudness of
different standards such as <abbr>EBU R-128</abbr>, <abbr>ATSC A/85</abbr>, and
ReplayGain 2.0. This means that ReplayGain tags from different sources do not
necessarily have the same meaning, which defeats the purpose of normalization.
In practice this means that ReplayGain is only really useful if you can be
certain that all tags were produced by the same program with the same settings.

The `BS17704_*` tags used by Musium aim to sidestep this problem by storing the
observed loudness, instead of the gain. The gain can easily be computed by the
player, and the particular target loudness that is used is not important anyway
for normalizing loudness in a collection of music. (It does matter when you want
to match the loudness of your music to e.g. external streaming services.)
Furthermore, by naming the tag after the revision of the standard, future
revisions to <abbr>BS.1770</abbr> will not create ambiguities in the meaning of
existing tags.

## Writing BS.1770-4 tags

[The <abbr>BS1770</abbr> `flacgain` utility][flacgain] can be used to analyze a
collection of flac files, and to add `BS17704_*` tags.

[flacgain]: https://github.com/ruuda/bs1770#tagging-flac-files
