# High-pass filter

Musium includes a built-in high-pass filter that can remove the low end from
bass-heavy music. A high-pass filter is useful in situations where your speakers
reproduce low frequencies — perhaps even louder than intended due to suboptimal
room acoustics — that are unpleasant or unwanted. Especially 2020s albums can be
bass-heavy, and taking off the low end can help to allow greater playback volume
without making the room sound saturated. Values around 50&nbsp;Hz are suitable
for this use case.

Like volume, the filter cutoff can be configured at runtime, and this functions
effectively like a _bass boost_ setting on players with a built-in equalizer,
except that the filter in Musium will only remove bass content, it will not
boost it. Setting the cutoff to 0&nbsp;Hz effectively disables the filter.

The high pass filter is not perfect. It has a gain of -3&nbsp;dB at the cutoff
frequency, and a rolloff of -12&nbsp;dB per octave. For example, at a cutoff
frequency of 50&nbsp;Hz, a 25&nbsp;Hz tone would be diminished by 15&nbsp;dB.
