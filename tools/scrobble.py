#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2018 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
scrobble.py -- Scrobble listens to Last.fm or Listenbrainz.

Last.fm Usage
-------------

    scrobble.py lastfm authenticate
    scrobble.py lastfm scrobble musium.sqlite3
    scrobble.py lastfm import <full|incremental> musium.sqlite3 <username>
    scrobble.py lastfm sync musium.sqlite3 <username>

The following environment variables are expected to be set:

    LAST_FM_API_KEY       Last.fm API key.
    LAST_FM_SECRET        Associated shared secret.
    LAST_FM_SESSION_KEY   Printed by authenticate, only needed for scrobble.

You can create an API key and secret at https://www.last.fm/api/account/create.


Listenbrainz Usage
------------------

    scrobble.py listenbrainz submit-listens musium.sqlite

The following environment variables are expected to be set:

    LISTENBRAINZ_USER_TOKEN  Listenbrainz user token

You can obtain a user token at https://listenbrainz.org/profile/.

"""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import sys
import time
import urllib

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from enum import Enum
from http.client import HTTPSConnection
from typing import Any, Dict, Iterator, List, Optional, Union, Tuple, TypeVar
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


LAST_FM_API_KEY = os.getenv("LAST_FM_API_KEY", "")
LAST_FM_SECRET = os.getenv("LAST_FM_SECRET", "")
LAST_FM_SESSION_KEY = os.getenv("LAST_FM_SESSION_KEY", "")
LISTENBRAINZ_USER_TOKEN = os.getenv("LISTENBRAINZ_USER_TOKEN", "")

# Listenbrainz enforces a max request body size.
# See https://listenbrainz.readthedocs.io/en/production/dev/api/,
# anchor #listenbrainz.webserver.views.api_tools.MAX_LISTEN_SIZE.
LISTENBRAINZ_MAX_BODY_BYTES = 10240


@dataclass(frozen=True)
class Listen:
    id: int
    started_at: datetime
    completed_at: datetime
    track_title: str
    album_title: str
    track_artist: str
    album_artist: str
    duration_seconds: int
    track_number: int
    disc_number: int

    def __post_init__(self) -> None:
        assert self.started_at.tzinfo is not None
        assert self.completed_at.tzinfo is not None

    def format_lastfm_scrobble(self, index: int) -> Dict[str, str]:
        """
        Format as parameters for a form/url-encoded request to scrobble the
        track to the Last.fm API.
        """

        def indexed(key: str) -> str:
            return f"{key}[{index}]"

        result = {
            indexed("artist"): self.track_artist,
            indexed("track"): self.track_title,
            indexed("timestamp"): str(int(self.started_at.timestamp())),
            indexed("album"): self.album_title,
            indexed("trackNumber"): str(self.track_number),
            indexed("duration"): str(self.duration_seconds),
            # Last.fm says "The album artist - if this differs from the track artist."
            # But if we don't include it, it echos back empty string in the response.
            indexed("albumArtist"): self.album_artist,
        }

        return result

    def format_listenbrainz_listen(self) -> Dict[str, Any]:
        """
        Format as a dict that can be submitted as json to the Listenbrainz API.
        See also https://listenbrainz.readthedocs.io/en/production/dev/json/#json-doc.
        """
        return {
            "listened_at": int(self.started_at.timestamp()),
            "track_metadata": {
                "additional_info": {
                    "listening_from": "Musium",
                    "tracknumber": self.track_number,
                    # TODO: Include Musicbrainz ids, once we track those in the
                    # listens database. In particular:
                    # * artist_mbids
                    # * release_mbid
                    # * recording_mbid
                    # * track_mbid
                    # * ISRC
                },
                "artist_name": self.track_artist,
                "track_name": self.track_title,
                "release_name": self.album_title,
            },
        }


def get_listens_to_scrobble(
    connection: sqlite3.Connection,
    *,
    since: Optional[datetime] = None,
) -> Iterator[Listen]:
    """
    Iterate unscrobbled listens that are eligible for scrobbling. When 'since'
    is set, we select only listens that happened within after that instant.
    This is needed for Last.fm, which does not allow backdating scrobbles further.
    """
    assert since is None or since.tzinfo is not None, "since must have tzinfo"

    common = """
        select
          id,
          started_at,
          completed_at,
          track_title,
          album_title,
          track_artist,
          album_artist,
          duration_seconds,
          track_number,
          disc_number
        from
          listens
        where
          -- Select all listens originating from us that still need to be scrobbled.
          scrobbled_at is null
          and source = 'musium'

          -- Last.fm guidelines say to only scrobble after playing for at least
          -- 30 seconds. Listenbrainz guidelines say to only scroble full tracks
          -- or at least 4 minutes, but Musium only plays full tracks (when
          -- completed_at is not null), so that is implied by the query.
          and cast(strftime('%s', completed_at) as integer) -
              cast(strftime('%s', started_at) as integer) > 30
        """

    if since is not None:
        results = connection.cursor().execute(
            f"""
            {common}
            -- We have an index on the convert-to-seconds-since-epoch expression
            -- for uniqueness already, so this comparison can leverage that index.
            and cast(strftime('%s', started_at) as integer) > ?;
            """,
            (since.timestamp(),),
        )

    else:
        results = connection.cursor().execute(f"{common};")

    for row in results:
        values = list(row)
        values[1] = datetime.fromisoformat(row[1].replace("Z", "+00:00"))
        values[2] = datetime.fromisoformat(row[2].replace("Z", "+00:00"))
        yield Listen(*values)


def set_scrobbled(
    connection: sqlite3.Connection,
    now: datetime,
    row_ids: List[int],
) -> None:
    """
    Update the rows to set scrobbled_at.
    """
    assert now.tzinfo is not None
    now_str = now.isoformat()
    params = [(now_str, row_id) for row_id in row_ids]
    connection.executemany(
        """
        update listens set scrobbled_at = ? where id = ?;
        """,
        params,
    )
    connection.commit()


T = TypeVar("T")


def iter_chunks(events: Iterator[T], n: int) -> Iterator[List[T]]:
    """
    Yield chunks of n items from the original iterator.
    The last chunk may be smaller.
    """
    result = []
    for event in events:
        result.append(event)

        if len(result) == n:
            yield result
            result = []

    if len(result) > 0:
        yield result


def format_batch_request_last_fm(listens: List[Listen]) -> Request:
    """
    Format a POST request to scrobble the given listens to Last.fm.
    """
    assert len(listens) <= 50, "Last.fm allows at most 50 scrobbles per batch."

    params = {
        "method": "track.scrobble",
        "sk": LAST_FM_SESSION_KEY,
    }

    for i, listen in enumerate(listens):
        params.update(listen.format_lastfm_scrobble(i))

    return format_signed_request(http_method="POST", data=params)


def format_signed_request(
    http_method: str,
    data: Dict[str, str],
) -> Request:
    """
    Format a signed request with the data encoded in query params.
    """
    params = {
        **data,
        "api_key": LAST_FM_API_KEY,
    }

    # Sort alphabetically by key, as required for the signature.
    params = {k: v for k, v in sorted(params.items())}

    sign_input = "".join(f"{k}{v}" for k, v in params.items()) + LAST_FM_SECRET
    params["api_sig"] = hashlib.md5(sign_input.encode("utf-8")).hexdigest()

    # The "format" key is not part of the signature input, we need to add it
    # later.
    params["format"] = "json"

    # Encode the parameters as key=value separated by ampersands, percent-escape
    # characters where necessary. Encode space as %20, do escape the slash by
    # marking no character as safe.
    body_str = urlencode(params, quote_via=urllib.parse.quote, safe="")

    return Request(
        url="https://ws.audioscrobbler.com/2.0/",
        method=http_method,
        data=body_str.encode("utf-8"),
    )


def cmd_scrobble(db_file: str) -> None:
    now = datetime.now(tz=timezone.utc)

    if LAST_FM_API_KEY == "":
        print("LAST_FM_API_KEY is not set, authentication will fail.")
    if LAST_FM_SECRET == "":
        print("LAST_FM_SECRET is not set, authentication will fail.")
    if LAST_FM_SESSION_KEY == "":
        print("LAST_FM_SESSION_KEY is not set, authentication will fail.")

    with sqlite3.connect(db_file) as connection:
        # Last.fm allows submitting scrobbles up to 14 days after their timestamp.
        # Any later, there is no point in submitting the scrobble any more.
        listens = get_listens_to_scrobble(connection, since=now - timedelta(days=14))

        # Last.fm allows submitting batches of at most 50 scrobbles at once.
        for batch in iter_chunks(listens, n=50):
            req = format_batch_request_last_fm(batch)
            response = json.load(urlopen(req))

            num_accepted = response["scrobbles"]["@attr"]["accepted"]
            ids_accepted = []

            # The Last.fm API uses heuristics to convert their xml-oriented API
            # into a json API. When a tag occurs more than once it turns into a
            # list, but when there is a single one, the list is omitted. This
            # means that if the batch happened to contain a single listen, then
            # we now get an object instead of a list. Make that uniform again.
            scrobbles = response["scrobbles"]["scrobble"]
            if not isinstance(scrobbles, list):
                scrobbles = [scrobbles]

            # If Last.fm rejected a scrobble, the error code of "ignoredMessage"
            # is nonzero. In theory the error code tells us why the scrobble was
            # rejected, but in practice the API is buggy, so we don't bother to
            # figure out what was wrong. See also
            # https://support.last.fm/t/all-scrobbles-ignored-with-code-1-artist-ignored-why/6754
            for listen, scrobble in zip(batch, scrobbles):
                was_accepted = scrobble["ignoredMessage"]["code"] == "0"
                if was_accepted:
                    ids_accepted.append(listen.id)
                else:
                    print(
                        f"ERROR: Last.fm rejected {listen}, response:",
                        json.dumps(scrobble),
                    )

            # Store that these listens have been scrobbled now.
            set_scrobbled(connection, now, ids_accepted)

            assert len(ids_accepted) == num_accepted
            # Flush, even when stdout is not a terminal, such as when running
            # under systemd, so we get accurate timestamps in the journal.
            print(f"Scrobbled {num_accepted} listens.", flush=True)


def cmd_authenticate() -> None:
    req = format_signed_request(
        http_method="GET",
        data={"method": "auth.getToken"},
    )
    response = json.load(urlopen(req))
    token = response["token"]

    print("Please authorize the application at the following page:\n")
    print(f"https://www.last.fm/api/auth/?api_key={LAST_FM_API_KEY}&token={token}\n")
    input("Press Enter to continue.")

    req = format_signed_request(
        http_method="GET",
        data={"method": "auth.getSession", "token": token},
    )
    response = json.load(urlopen(req))
    username = response["session"]["name"]
    session_key = response["session"]["key"]
    print(f"\nScrobbling authorized by user {username}.")
    print("Please set the following environment variable when scrobbling:\n")
    print(f"LAST_FM_SESSION_KEY={session_key}")


@dataclass(frozen=True)
class LastfmPageImport:
    total_pages: int
    total_listens: int
    min_started_at: datetime


def fix_misencodings(s: str) -> str:
    """
    Bad data ended up in Last.fm. But we can fix some known cases.
    """
    if "Ã©" in s:
        return s.encode("latin-1").decode("utf-8")
    else:
        return s


def import_lastfm_page(
    client: HTTPSConnection,
    tx: sqlite3.Cursor,
    username: str,
    page: int,
    after_timestamp: Optional[datetime],
) -> Optional[LastfmPageImport]:
    """
    Import one page of listens from Last.fm. Returns the timestamp of the oldest
    listen, and the number of pages in total.
    """
    params = {
        "method": "user.getRecentTracks",
        "user": username,
        "page": page,
        "limit": 200,
        "api_key": LAST_FM_API_KEY,
        "format": "json",
    }

    if after_timestamp is not None:
        assert after_timestamp.tzinfo is not None
        params["from"] = int(after_timestamp.timestamp())

    params_str = urlencode(params, quote_via=urllib.parse.quote, safe="")
    client.request("GET", "/2.0/?" + params_str)
    response = client.getresponse()

    if response.status != 200:
        print(f"Got {response.status}:", response.read())
        return None

    data = json.load(response)["recenttracks"]
    attrs = data["@attr"]
    started_at: int

    for track in data["track"]:
        started_at = int(track["date"]["uts"])
        tx.execute(
            """
            insert into lastfm_listens
              (started_at, title, track_artist, album, album_mbid)
            values
              (?, ?, ?, ?, ?)
            on conflict
              do nothing;
            """,
            (
                started_at,
                fix_misencodings(track["name"]),
                fix_misencodings(track["artist"]["#text"]),
                fix_misencodings(track["album"]["#text"]),
                track["album"]["mbid"],
            ),
        )

    return LastfmPageImport(
        total_pages=int(attrs["totalPages"]),
        total_listens=int(attrs["total"]),
        min_started_at=datetime.fromtimestamp(started_at, tz=timezone.utc),
    )


def cmd_lastfm_import(is_full: bool, db_file: str, username: str) -> None:
    now = datetime.now(tz=timezone.utc)

    if LAST_FM_API_KEY == "":
        print("LAST_FM_API_KEY is not set, authentication will fail.")

    timeout_seconds = 5.5
    client = HTTPSConnection("ws.audioscrobbler.com", 443, timeout=timeout_seconds)
    page = 1
    n_errors = 0
    n_have = 0

    with sqlite3.connect(db_file) as db_conn:
        # If we do a full import, then there is no lower bound on the scrobble
        # timestamp. For some reason though, the number that Last.fm reports and
        # the count we get in the database is different, so it looks like we are
        # never done if we always did a full import. (Maybe Last.fm allows
        # duplicates on a timestamp?) For an incremental import, we take the
        # past 2 weeks (because that is how long you can still submit to Last.fm
        # plus some slack), or if the most recent listen we have is older, then
        # we use that as the lower bound.
        after_timestamp: Optional[datetime]
        if is_full:
            after_timestamp = None
            print("Doing a full import.")
        else:
            after_timestamp = now - timedelta(days=16)
            tx = db_conn.cursor()
            (newest_listen,) = tx.execute(
                "select max(started_at) from lastfm_listens;"
            ).fetchone()
            db_conn.commit()
            if newest_listen is not None:
                after_timestamp = min(
                    after_timestamp,
                    datetime.fromtimestamp(newest_listen, tz=timezone.utc),
                )
                print(
                    f"Doing an incremental import of listens as of {after_timestamp}."
                )

        # Import one page at a time, starting with the most recent listens.
        # We know we are done when we have the same number of listens in the
        # database as Last.fm claims to have. This assumes that scrobbles don't
        # happen while the import is running.
        while True:
            tx = db_conn.cursor()
            result = import_lastfm_page(client, tx, username, page, after_timestamp)

            if result is None:
                db_conn.rollback()
                n_errors += 1
                assert n_errors < 10, "Bailing after 10 consecutive errors."
                sleep_seconds = 5.0
                time.sleep(sleep_seconds)
                continue
            else:
                n_errors = 0
                since = (
                    after_timestamp.timestamp() if after_timestamp is not None else 0
                )
                (n_have,) = tx.execute(
                    """
                    select count(1)
                    from lastfm_listens
                    where (started_at > ?);
                    """,
                    (since,),
                ).fetchone()
                db_conn.commit()

            print(
                f"[{page} / {result.total_pages}] "
                f"Have {n_have:,} of {result.total_listens:,} listens."
            )

            if n_have == result.total_listens:
                print(
                    f"Database has {n_have} listens, same as Last.fm, import is complete."
                )
                break

            if page == result.total_pages:
                print(f"We imported page {page} of {page}, import is complete.")
                break

            page += 1


def format_batch_request_listenbrainz(listens: List[Listen]) -> Optional[Request]:
    """
    Format a POST request to submit the given listens to Listenbrainz.

    Returns None when trying to submit too many listens in one request.
    """

    body_dict = {
        "listen_type": "import",
        "payload": [listen.format_listenbrainz_listen() for listen in listens],
    }
    body_bytes = json.dumps(body_dict).encode("utf-8")

    if len(body_bytes) > LISTENBRAINZ_MAX_BODY_BYTES:
        return None

    return Request(
        url="https://api.listenbrainz.org/1/submit-listens",
        method="POST",
        headers={
            "Authorization": f"Token {LISTENBRAINZ_USER_TOKEN}",
            "Content-Type": "application/json; charset=utf-8",
        },
        data=body_bytes,
    )


@dataclass(frozen=True)
class ListenbrainzBatch:
    listens: List[Listen]
    request: Request


def iter_requests_listenbrainz(
    listens: Iterator[Listen],
) -> Iterator[ListenbrainzBatch]:
    """
    Break up the stream of listens into submission requests.
    """
    # At the time of writing (when listens do not include Musicbrainz
    # identifiers), sizes of individual listens are around 190-240 bytes.
    # So as a first guess, we are going to create batches that are expected to
    # fit in one request, assuming 215 bytes per listen.
    listens_per_batch = LISTENBRAINZ_MAX_BODY_BYTES // 215

    batches = iter_chunks(listens, n=listens_per_batch)
    listens_remaining: List[Listen] = []

    # We start out with this batch size, but refine it while trying to create
    # batches, reduce n step by step if the batch is too large, and increase it
    # again if we did have a batch that fit.
    n = listens_per_batch

    while True:
        # Replenish the buffer of listens when it runs low.
        while len(listens_remaining) < 2 * listens_per_batch:
            try:
                listens_remaining.extend(next(batches))

            except StopIteration:
                break

        # If after that the buffer is still empty, there was no new batch,
        # and we are done.
        if len(listens_remaining) == 0:
            break

        # Slice out a batch of size n from the buffer.
        batch = listens_remaining[:n]
        listens_remaining = listens_remaining[n:]
        request = format_batch_request_listenbrainz(batch)

        if request is not None:
            assert len(batch) > 0
            yield ListenbrainzBatch(batch, request)
            n = n + 5

        else:
            # The batch is too big, reduce the size and try again.
            listens_remaining = batch + listens_remaining
            assert n > 1, "A listen is too big to submit"
            n = n - 1


def cmd_submit_listens(db_file: str) -> None:
    now = datetime.now(tz=timezone.utc)

    if LISTENBRAINZ_USER_TOKEN == "":
        print("LISTENBRAINZ_USER_TOKEN is not set, authorization will fail.")

    with sqlite3.connect(db_file) as connection:
        listens = get_listens_to_scrobble(connection)

        for batch in iter_requests_listenbrainz(listens):
            try:
                response = urlopen(batch.request)
                assert response.status == 200
                ids_accepted = [listen.id for listen in batch.listens]
                set_scrobbled(connection, now, ids_accepted)
                # Flush, even when stdout is not a terminal, such as when running
                # under systemd, so we get accurate timestamps in the journal.
                print(f"Submitted {len(batch.listens)} listens.", flush=True)

                # Try to avoid exceeding the rate limit, never let the number of
                # calls remaining go to 0. See also
                # https://listenbrainz.readthedocs.io/en/production/dev/api/#rate-limiting.
                if int(response.headers.get("X-RateLimit-Remaining", "10")) <= 1:
                    sleep_seconds = float(
                        response.headers.get("X-RateLimit-Reset-In", "1")
                    )
                    time.sleep(sleep_seconds)

            except HTTPError as err:
                print(f"Unexpected response, status {err.status}.")
                # Re-format the body for easier debugging. If the body is not
                # json this fails, but we already printed the error. If the
                # error is temporary, like a 503, then we just fail, and the
                # next time that this script runs we hope for better luck.
                print(json.dumps(json.load(err), indent=2))
                sys.exit(1)


if __name__ == "__main__":
    command = []
    if len(sys.argv) > 2:
        command = sys.argv[1:3]

    if command == ["lastfm", "authenticate"] and len(sys.argv) == 3:
        cmd_authenticate()

    elif command == ["lastfm", "scrobble"] and len(sys.argv) == 4:
        cmd_scrobble(sys.argv[3])

    elif command == ["lastfm", "import"] and len(sys.argv) == 6:
        assert sys.argv[3] in ("full", "incremental")
        is_full = sys.argv[3] == "full"
        cmd_lastfm_import(is_full=is_full, db_file=sys.argv[4], username=sys.argv[5])

    elif command == ["lastfm", "sync"] and len(sys.argv) == 5:
        # Sync is a shorthand for scrobble followed by import.
        cmd_scrobble(sys.argv[3])
        cmd_lastfm_import(is_full=False, db_file=sys.argv[3], username=sys.argv[4])

    elif command == ["listenbrainz", "submit-listens"] and len(sys.argv) == 4:
        cmd_submit_listens(sys.argv[3])

    else:
        print(__doc__)
        sys.exit(1)
