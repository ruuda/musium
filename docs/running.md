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

We do need to restart the server after thumbnail generation is complete to make
the thumbnails show up.

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

    [Install]
    WantedBy=default.target

    # TODO: Enable some hardening options.

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
changes, and then restart the server.
