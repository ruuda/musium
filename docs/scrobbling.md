# Scrobbling to Last.fm

Musium can be set up to scrobble plays to Last.fm. Musium logs plays to a SQLite
database in the data directory. An enclosed script can batch-submit those plays
to Last.fm. Running the script regularly ensures that all plays get scrobbled.
Musium does not currently offer immediate scrobbling or *now playing* updates.

## Authenticating

To scrobble to Last.fm, you need an <abbr>API</abbr> key and secret. Create those
at [last.fm/api/account/create](https://www.last.fm/api/account/create).
Ensure that the <abbr>API</abbr> key and secret are available in the environment
as `LAST_FM_API_KEY` and `LAST_FM_SECRET`.

Next, you need to authorize the script to submit scrobbles to your account. In
the repository, run

    tools/scrobble.py authenticate

This will print a `LAST_FM_SESSION_KEY`, which you also need to put in the
environment to be able to submit scrobbles.

## Running manually

With the environment variables set up, run the `scrobble` command in the
repository:

    tools/scrobble.py scrobble /data_path/musium.sqlite3

The data path is the [`data_path` as configured](configuration.md#data_path).
Musium stores `musium.sqlite3` in that directory.

The script only scrobbles listens that originated from Musium itself, it does
not scrobble imported listening history. Listens that were scrobbled
successfully get marked as such in the database, so they are only scrobbled
once.

## With systemd

Systemd timers can be useful for scrobbling periodically. First create a
one-shot service that runs the scrobble script:

```systemd
[Unit]
Description=Musium Scrobbler

[Service]
Type=oneshot
ExecStart=/checkouts/musium/tools/scrobble.py scrobble /var/lib/musium/musium.sqlite3

# The values below are randomly generated examples, they are not real secrets.
# Replace them with your personal secrets.
Environment=LAST_FM_API_KEY=5d41402abc4b2a76b9719d911017c592
Environment=LAST_FM_SECRET=f330c2f5a4e075a21593f477b9ee967a
Environment=LAST_FM_SESSION_KEY=gE7P1f444dLu6NbZeMs4wb9V4roITlAF

[Install]
WantedBy=default.target
```

Write it to `/etc/systemd/system/musium-scrobble.service`. Then add a timer to
start the script periodically:

```systemd
[Unit]
Description=Musium Scrobbler

[Timer]
# Run daily, between 08:00 and 08:15 local time.
OnCalendar=*-*-* 08:00:00
RandomizedDelaySec=900

# Run after boot if the system was powered off at the previous scheduled time.
Persistent=true

[Install]
WantedBy=timers.target
```

Write it to `/etc/systemd/system/musium-scrobble.timer`, then start the timer:

    systemctl daemon-reload
    systemctl enable musium-scrobble.timer
    systemctl start musium-scrobble.timer
