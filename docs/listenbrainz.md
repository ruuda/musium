# Submitting listens to Listenbrainz

Musium can be set up to submit plays to [Listenbrainz][lb]. Musium logs plays to
its SQLite database. An enclosed script can batch-submit those plays to
Listenbrainz. Running the script regularly ensures that all plays get submitted.
Musium does not currently offer immediate submission or *now playing* updates.

[lb]: https://listenbrainz.org

## Running manually

To submit listens to your profile, you need to obtain your *user token* from
[listenbrainz.org/profile](https://listenbrainz.org/profile/). Make this
available in the `LISTENBRAINZ_USER_TOKEN` environment variable, for example:

    export LISTENBRAINZ_USER_TOKEN=ab32823b-57e7-4953-80be-f10294b26058

With this set up, we can run the submission script located in the `tools`
directory of the repository:

    tools/scrobble.py submit-listens /db_path/musium.sqlite3

The file to pass is the [`db_path` as configured](configuration.md#db_path);
this is where Musium tracks listens.

The script only submits listens that originated from Musium itself, it does
not submit imported listening history. Listens that were submitted successfully
get marked as such in the database, so they are only submitted once.

## With systemd

Systemd timers can be useful for submitting listens periodically. This works the
same as [Last.fm scrobbling with systemd](scrobbling.md#with-systemd), with two
small differences:

 * The command is `scrobble.py submit-listens`, not `scrobble.py scrobble`.
 * The environment variable to set is `LISTENBRAINZ_USER_TOKEN`, the
   `LAST_FM_*` variables are not needed.

## On post-idle

It is possible to submit listens right after playback ends using a post-idle
command. This works the same as [for Last.fm
scrobbling](scrobbling.md#on-post-idle), see that page for more details.
