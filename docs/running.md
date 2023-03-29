# Running

Musium logs to stdout and runs until it is killed, which makes it easy to run in
a terminal for development, and it works well with systemd to run as a daemon.
Before we can start the server, we need to scan the library. After
[building](building.md):

    target/release/musium scan musium.conf

The first scan might take a few minutes, depending on the size of the library
and the speed of your disks. Generating thumbnails will take a long time, but we
do not need to wait for it, we can already start the server:

    target/release/musium serve musium.conf

After thumbnail generation is complete, we can either restart the server, or use
the _rescan library_ option on the _about_ page and then refresh the
webinterface to make the new thumbnails show up.

## With systemd

An example unit file:

    [Unit]
    Description=Musium Music Daemon

    [Service]
    # TODO: Currently the server loads static files from the repository,
    # so the working directory needs to be a checkout. We should embed the
    # static files in the binary instead.
    WorkingDirectory=/home/media/checkouts/musium
    ExecStart=/usr/local/bin/musium serve /etc/musium.conf

    # Musium supports reporting startup progress to systemd, set this to enable.
    Type=notify

    # When running as non-root user, CAP_SYS_NICE is needed to boost the
    # priority of the audio playback thread.
    AmbientCapabilities=CAP_SYS_NICE

    # Wen running as a non-root user, CAP_NET_BIND_SERVICE is needed to bind
    # to ports below 1024.
    AmbientCapabilities=CAP_NET_BIND_SERVICE

    [Install]
    WantedBy=default.target

This assumes that you have a [release binary](building.md) in `/usr/local/bin`,
and a [configuration file](configuration.md) at `/etc/musium.conf`. Write the
above file to `/etc/systemd/system/musium.service`, then start the service:

    systemctl daemon-reload
    systemctl start musium

## With systemd-user

It is also possible to run Musium using your systemd user instance. In that
case, place the unit at `~/.config/systemd/user/musium.service`, and use
`system --user` to start it. If you run the deamon under your own account on a
headless system, you may need to run

    loginctl enable-linger $USER

to allow the deamon to linger after you log out.

## Scanning the library

`musium serve` will serve the library as it was when it was last scanned. When
the library changes, you need to run `musium scan musium.conf` to pick up the
changes, and then restart the server. Alternatively, you can use the _rescan
library_ button on the _about_ page. In this case, no restart is needed to pick
up the changes, but you do need to refresh the webinterface.
