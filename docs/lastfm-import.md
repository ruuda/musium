# Importing from Last.fm

In addition to [scrobbling plays to Last.fm](scrobbling.md), Musium can import
listens from Last.fm. These listens count towards playcounts which are used to
rank trending tracks and discoveries. This way, these features can be useful
even when you haven’t been using Musium for a long time, or when you listen with
other players in addition to Musium.

## Authenticating

Follow the [authentication steps](scrobbling.md#authenticating) as for
scrobbling. To import listens you do not need the `LAST_FM_SECRET` and
`LAST_FM_SESSION_KEY`, only the `LAST_FM_API_KEY`.

## Full import

The first time you import listens, you will want to import your full listening
history. This is done as follows:

    tools/scrobble.py lastfm import full /db_path/musium.sqlite3 «username»

The file path is [`db_path` as configured](configuration.md#db_path), and
`«username»` is your Last.fm username. This command will then fill the
`lastfm_listens` table in the database.

## Incremental import

After a full import, an incremental import is typically sufficient. An
incremental import will import all listens in the two weeks before the latest
listen in the database. If you run an incremental import regularly (e.g. daily
with a systemd timer) then this should be sufficient even for cached listens
that are submitted later. In case of missed listens, you can simply do a full
import again. An import will only ever add listens, it will never erase already
imported listens. To perform an incremental import:

    tools/scrobble.py lastfm import incremental \
      /db_path/musium.sqlite3 «username»

## Integrated syncing

The scrobble script has a subcommand `lastfm sync` which performs a `lastfm
scrobble` followed by a `lastfm import incremental`. It is useful for use with
systemd, as shown below.

## With systemd

As with scrobbling, it is possible to [run the import as a one-shot systemd
unit](scrobbling.md#with-systemd), which can then be triggered by a systemd
timer, or started as part of the [post-idle
script](configuration.md#exec_post_idle_path). The setup would be the same as for
scrobbling, except instead of calling `lastfm scrobble`, the unit’s
`ExecStart=` would use `lastfm sync`.
