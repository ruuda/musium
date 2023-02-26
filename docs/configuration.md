# Configuration

Musium reads all settings from a configuration file. The location of the config
file is passed as an argument to the program. Config files consist of key-value
pairs with `=` separator, and support `#` for comments.

## Example

    # Note: listening on port 80 requires CAP_NET_BIND_SERVICE.
    # If you want to run as an unprivileged user, use a port beyond 1024.
    listen = 0.0.0.0:80

    library_path = /home/media/music
    db_path = /var/lib/musium/musium.sqlite3

    audio_device = UMC404HD 192k
    audio_volume_control = UMC404HD 192k Output

    high_pass_cutoff = 30 Hz

## Settings

The following settings are available. Unless noted otherwise, all options must
be specified exactly once.

### listen

The address and port to bind to, for example `0.0.0.0:80`. Use `0.0.0.0` as the
address to listen for external connections and make the player available to the
entire local network. Use `localhost` to listen only on loopback.

The listen address is optional and defaults to `0.0.0.0:8233`.

### library_path

The directory to recursively scan for flac files.

### db_path

Musium uses SQLite to store persistent state. The `db_path` setting specifies
where that database is stored. The directory where the database is to be stored
must exist.

The database contains the listens database, cached tag metadata, loudness
information, and cover art. Updating the listens database causes disk activity
for every track, so it is recommended to keep the data path on a silent storage
medium. See also [the section on disks](disks.md) for more details.

The size of the database is dominated by cover art and waveforms. On average an
album uses 62 kilobytes of disk space. For 1600 albums, that would be about 100
MB.

### audio_device

The <abbr>Alsa</abbr> card used for playback. When the configured card cannot
be found, Musium will list all of the cards that are available. You can also
list cards manually with `aplay --list-devices`. The name of the device is
listed between square brackets. Musium uses the <abbr>Alsa</abbr> hardware
device directly, there is no need nor support for PulseAudio.

### audio_volume_control

The <abbr>Alsa</abbr> simple mixer control that controls playback volume. Often
there are controls named `Master`, `PCM`, and `Speakers`, but this differs from
card to card. Use `amixer scontrols` to list available controls. Be sure to run
this with the right privileges (possibly as superuser, or as a user in the
`audio` group) to reveal all available controls.

Musium assumes exclusive control over this mixer control, so you should not
manipulate it manually with tools like Alsamixer after starting Musium. In
particular, Musium adjusts the volume to perform loudness normalization, so even
for a constant target playback volume, Musium will manipulate the mixer control.

### high_pass_cutoff

Apply a high-pass filter to the output, with the given cutoff frequency. The
value must be an integer and include the _Hz_ unit as suffix. This setting is
optional and defaults to 0&nbsp;Hz (so no frequencies are filtered).

A high-pass filter is useful in situations where your speakers reproduce low
frequencies — perhaps even louder than intended due to suboptimal room
acoustics — that are unpleasant or unwanted. Especially 2020s albums can be
bass-heavy, and taking off the low end can help to allow greater playback volume
without making the room sound saturated. Values around 50&nbsp;Hz are suitable
for this use case.

The high pass filter is not perfect. It has a gain of -3&nbsp;dB at the cutoff
frequency, and a rolloff of -12&nbsp;dB per octave. For example, at a cutoff
frequency of 50&nbsp;Hz, a 25&nbsp;Hz tone would be diminished by 15&nbsp;dB.

### exec_pre_playback_path

When Musium starts playback from an idle state, it can optionally execute a
program before continuing. For example, you can use this to ensure that your
speakers are powered on. See the page about [Trådfri control](tradfri.md) for
how to do this with Ikea Trådfri outlets.

The value is the path of a program to be executed. It is not possible to pass
arguments to the program. Instead, you can create a shell script that will call
the program with the required arguments. Musium waits for the program to exit
before starting playback. However, if the program does not finish within 10
seconds, Musium will continue playback anyway. After 20 more seconds, Musium
will kill the child process if it is still running.

This setting is optional. When it is not set, Musium starts playback instantly.

### exec_post_idle_path

After playback ends, Musium can optionally execute a program. For example, you
can use this to turn your speakers off when there is no longer any music
playing. See the page about [Trådfri control](tradfri.md) for how to do this
with Ikea Trådfri outlets.

The value is the path of a program to be executed. It is not possible to pass
arguments to the program. Instead, you can create a shell script that will call
the program with the required arguments. If the program does not finish within
30 seconds, Musium will kill the child process.

You can control the time between playback ending, and executing the program,
with the `idle_timeout_seconds` setting. If playback resumes within this time,
Musium does not execute the post-idle program. It _will_ execute the
pre-playback program when playback resumes, regardless of whether the post-idle
program was executed.

This setting is optional.

### idle_timeout_seconds

The time between playback ending, and executing the post-idle program, in
seconds. This setting is optional and defaults to three minutes. This setting
is only useful in combination with `exec_post_idle_path`.
