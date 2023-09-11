# Loudness normalization

Musium normalizes the perceptual playback loudness to make tracks sound equally
loud. It does this by estimating the loudness from audio data, and compensating
playback volume for that.

## Playback volume

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

## Track and album loudness

When two tracks from the same album play consecutively, Musium adjusts for the
album’s loudness rather than the track loudness. In other words, Musium does not
change the relative volume of tracks on the same album. You can listen to the
album the way it was mastered.

When tracks from _different_ albums play consecutively, Musium adjusts for track
loudness, because this allows slightly better matching than album loudness.

## Computing loudness

Musium automatically computes the loudness for any new tracks when they are
added to the library. This can take some time, especially on devices that do not
have a fast <abbr>CPU</abbr>, like a Raspberry Pi. The upside is that this also
instantly verifies that Musium can decode the file. Loudness is saved to [the
database](configuration.md#db_path).

## Loudness measurement

Musium computes the integrated loudness as defined in
[<abbr>ITU-R BS.1770-4</abbr>][bs1770-4].
For album loudness, it computes the integrated loudness over the concatenation
of all tracks on the album.

To reproduce the measurement externally, [the <abbr>BS1770</abbr> `flacgain`
utility][flacgain] can be used to analyze a collection of flac files, and to
write loudness to `BS17704_*` tags. In the past Musium relied on these tags for
loudness information, but since Musium 0.11.0 they are no longer used.

[bs1770-4]: https://www.itu.int/rec/R-REC-BS.1770-4-201510-I/en
[flacgain]: https://github.com/ruuda/bs1770#tagging-flac-files

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
Musium sidesteps this problem by computing the loudness itself rather than
depending on ambiguous tags.
