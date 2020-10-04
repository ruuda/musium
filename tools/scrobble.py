#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2018 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
scrobble.py -- Scrobble listens to Last.fm.

Usage:

    scrobble.py authenticate
    scrobble.py scrobble musium.sqlite3

The following environment variables are expected to be set:

    LAST_FM_API_KEY       Last.fm API key.
    LAST_FM_SECRET        Associated shared secret.
    LAST_FM_SESSION_KEY   Printed by authenticate, only needed for scrobble.

You can create an API key and secret at https://www.last.fm/api/account/create.

"""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import sys
import urllib

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from enum import Enum
from urllib.request import Request, urlopen
from urllib.parse import urlencode
from typing import Dict, Union, Iterator


API_KEY = os.getenv('LAST_FM_API_KEY', '')
SECRET = os.getenv('LAST_FM_SECRET', '')
SESSION_KEY = os.getenv('LAST_FM_SESSION_KEY', '')


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

    def format_scrobble(self, index: int) -> Dict[str, str]:
        """
        Format as parameters for a form/url-encoded request to scrobble the track.
        """
        def indexed(key: str) -> str:
            return f'{key}[{index}]'

        result = {
            indexed('artist'): self.track_artist,
            indexed('track'): self.track_title,
            indexed('timestamp'): str(int(self.started_at.timestamp())),
            indexed('album'): self.album_title,
            indexed('trackNumber'): str(self.track_number),
            indexed('duration'): str(self.duration_seconds),
            # Last.fm says "The album artist - if this differs from the track artist."
            # But if we don't include it, it echos back empty string in the response.
            indexed('albumArtist'): self.album_artist,
        }

        return result


def get_listens_to_scrobble(
    connection: sqlite3.Connection,
    now: datetime,
) -> Iterator[Listen]:
    """
    Iterate unscrobbled listens that are eligible for scrobbling.
    """
    assert now.tzinfo == timezone.utc

    # Last.fm allows submitting scrobbles up to 14 days after their timestamp.
    # Any later, there is no point in submitting the scrobble any more.
    since = (now - timedelta(days=14)).timestamp()

    results = connection.cursor().execute(
        """
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

          -- But only those that Last.fm would accept. We have an index on the
          -- convert-to-seconds-since-epoch expression for uniqueness already,
          -- so this comparison can leverage that index.
          and cast(strftime('%s', started_at) as integer) > ?

          -- Last.fm guidelines say to only scrobble after playing for at least
          -- 30 seconds.
          and cast(strftime('%s', completed_at) as integer) -
              cast(strftime('%s', started_at) as integer) > 30;
        """,
        (since,)
    )

    for row in results:
        values = list(row)
        values[1] = datetime.fromisoformat(row[1].replace('Z', '+00:00'))
        values[2] = datetime.fromisoformat(row[2].replace('Z', '+00:00'))
        yield Listen(*values)


def set_scrobbled(
    connection: sqlite3.Connection,
    now: datetime,
    row_ids: List[int],
) -> Iterator[Listen]:
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


def iter_chunks(events: Iterator[Event], n: int) -> Iterator[List[Event]]:
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


def format_batch_request(events: List[Event]) -> Request:
    """
    Format a POST request to scrobble the given events.
    """
    assert len(events) <= 50, 'Last.fm allows at most 50 scrobbles per batch.'

    params = {
        'method': 'track.scrobble',
        'sk': SESSION_KEY,
    }

    for i, event in enumerate(events):
        params.update(event.format_scrobble(i))

    return format_signed_request(http_method='POST', data=params)


def format_signed_request(
    http_method: str,
    data: Dict[str, str],
) -> Request:
    """
    Format a signed request to
    """
    params = {
        **data,
        'api_key': API_KEY,
    }

    # Sort alphabetically by key, as required for the signature.
    params = {k: v for k, v in sorted(params.items())}

    sign_input = ''.join(f'{k}{v}' for k, v in params.items()) + SECRET
    params['api_sig'] = hashlib.md5(sign_input.encode('utf-8')).hexdigest()

    # The "format" key is not part of the signature input, we need to add it
    # later.
    params['format'] = 'json'

    # Encode the parameters as key=value separated by ampersands, percent-escape
    # characters where necessary. Encode space as %20, do escape the slash by
    # marking no character as safe.
    body_str = urlencode(params, quote_via=urllib.parse.quote, safe='')

    return Request(
        url='https://ws.audioscrobbler.com/2.0/',
        method=http_method,
        data=body_str.encode('utf-8'),
    )


def cmd_scrobble(db_file: str) -> None:
    now = datetime.now(tz=timezone.utc)

    if API_KEY == '':
        print('LAST_FM_API_KEY is not set, authentication will fail.')
    if SECRET == '':
        print('LAST_FM_SECRET is not set, authentication will fail.')
    if SESSION_KEY == '':
        print('LAST_FM_SESSION_KEY is not set, authentication will fail.')

    with sqlite3.connect(db_file) as connection:
        listens = get_listens_to_scrobble(connection, now)

        n_scrobbled = 0

        # Last.fm allows submitting batches of at most 50 scrobbles at once.
        for batch in iter_chunks(listens, n=50):
            req = format_batch_request(batch)
            response = json.load(urlopen(req))

            num_accepted = response['scrobbles']['@attr']['accepted']
            ids_accepted = []

            # The Last.fm API uses heuristics to convert their xml-oriented API
            # into a json API. When a tag occurs more than once it turns into a
            # list, but when there is a single one, the list is omitted. This
            # means that if the batch happened to contain a single listen, then
            # we now get an object instead of a list. Make that uniform again.
            scrobbles = response['scrobbles']['scrobble']
            if not isinstance(scrobbles, list):
                scrobbles = [scrobbles]

            # If Last.fm rejected a scrobble, the error code of "ignoredMessage"
            # is nonzero. In theory the error code tells us why the scrobble was
            # rejected, but in practice the API is buggy, so we don't bother to
            # figure out what was wrong. See also
            # https://support.last.fm/t/all-scrobbles-ignored-with-code-1-artist-ignored-why/6754
            for listen, scrobble in zip(batch, scrobbles):
                was_accepted = scrobble['ignoredMessage']['code'] == '0'
                if was_accepted:
                    ids_accepted.append(listen.id)
                else:
                    print(
                        f'ERROR: Last.fm rejected {listen}, response:',
                        json.dumps(scrobble),
                    )

            # Store that these listens have been scrobbled now.
            set_scrobbled(connection, now, ids_accepted)

            assert len(ids_accepted) == num_accepted
            print(f'Scrobbled {num_accepted} listens.')
            n_scrobbled += num_accepted


def cmd_authenticate() -> None:
    req = format_signed_request(
        http_method='GET',
        data={'method': 'auth.getToken'},
    )
    response = json.load(urlopen(req))
    token = response['token']

    print('Please authorize the application at the following page:\n')
    print(f'https://www.last.fm/api/auth/?api_key={API_KEY}&token={token}\n')
    input('Press Enter to continue.')

    req = format_signed_request(
        http_method='GET',
        data={'method': 'auth.getSession', 'token': token},
    )
    response = json.load(urlopen(req))
    username = response['session']['name']
    session_key = response['session']['key']
    print(f'\nScrobbling authorized by user {username}.')
    print('Please set the following environment variable when scrobbling:\n')
    print(f'LAST_FM_SESSION_KEY={session_key}')


if __name__ == '__main__':
    command = ''
    if len(sys.argv) > 1:
        command = sys.argv[1]

    if command == 'authenticate' and len(sys.argv) == 2:
        cmd_authenticate()

    elif command == 'scrobble' and len(sys.argv) == 3:
        cmd_scrobble(sys.argv[2])

    else:
        print(__doc__)
        sys.exit(1)
