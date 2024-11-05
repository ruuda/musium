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

    volume = -10 dB
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

The <abbr>Alsa</abbr> <abbr>PCM</abbr> used for playback. This is a string that
is interpreted by <abbr>Alsa</abbr>, and its format is unfortunately not well
documented, though [there is some information about <abbr>PCM</abbr> naming
conventions in the <abbr>Alsa</abbr> docs][alsa-names]. Values that should be
supported are those printed by `aplay --list-pcms`, but these are not the only
ones. For example, `front` might give you the default front speakers. Depending
on how your machine is configured, you may see `pulse` and `pipewire` as sound
servers.

Although it is possible to select a software output, Musium is built to use a
hardware device directly and exclusively (e.g. an external <abbr>USB</abbr>
audio interface). Values like `hw:0`, `hw:1`, etc. select a card by index,
corresponding to the output of `aplay --list-devices`. When there are multiple
devices and subdevices on the same card, they can be referenced by index with
commas, e.g. `hw:0,1,2` selects card 0, device 1, subdevice 2. Instead of a
numeric index, it is also possible to use named devices with key-value syntax,
e.g. `hw:CARD=U192k,DEV=0`.

The selected device may not support the bit depth and sample rate that Musium
tries to use. For example, most music is released as 16-bit 44.1 kHz audio, but
the Realtek ALC289 present on many laptops will only do 48 kHz. Musium does not
perform sample rate conversion, but <abbr>Alsa</abbr>’s _plug_ device can do this.
To use it, replace `hw:` with `plughw:`, or prepend `plug:` and then quote the
`hw:...` device in single quotes.

To verify whether a name is valid, we can use `aplay` to play `/dev/zero`.
E.g.

```
aplay \
  --device=hw:CARD=PCH,DEV=0 \
  --period-size=512 \
  --buffer-size=2048 \
  --channels=2 \
  --format=S16_LE \
  --dump-hw-params \
  /dev/zero
```

By adding `--dump-hw-params`, the program will print the parameter ranges that
the device supports.

[alsa-names]: https://www.alsa-project.org/alsa-doc/alsa-lib/pcm.html#pcm_dev_names

### audio_volume_control

The <abbr>Alsa</abbr> simple mixer control that controls playback volume. Often
there are controls named `Master`, `PCM`, and `Speakers`, but this differs from
card to card. Use `amixer scontrols` to list available controls. The name listed
between single quotes is the name that Musium expects. Be sure to run `amixer`
with the right privileges (possibly as superuser, or as a user in the `audio`
group) to reveal all available controls, and pass the numeric card index with
`--card`. You may need to pass `--device` as well.

Musium assumes exclusive control over this mixer control, so you should not
manipulate it manually with tools like Alsamixer after starting Musium. In
particular, Musium adjusts the volume to perform loudness normalization, so even
for a constant target playback volume, Musium will manipulate the mixer control.

### volume

Initial volume at startup. The value must be an integer and include the _dB_
unit as suffix. This setting is optional and defaults to `-10 dB`. See also
the [loudness normalization chapter](loudness.md) for the meaning of the value.
The volume can be changed at runtime in the webinterface.

### high_pass_cutoff

Initial filter cutoff frequency for the [high-pass filter](highpass.md). The
value must be an integer and include the _Hz_ unit as suffix. This setting is
optional and defaults to `0 Hz` (so no frequencies are filtered). The filter
cutoff can be changed at runtime in the webinterface.

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
can use this to submit listens to Last.fm, or to turn your speakers off when
there is no longer any music playing. See the page about [Trådfri
control](tradfri.md) for how to do this with Ikea Trådfri outlets.

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
