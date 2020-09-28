#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2018 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
scrobble.py -- Scrobble the play log to Last.fm.

Usage:

    scrobble.py authenticate
    scrobble.py scrobble plays.log

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
import sys
import urllib

from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from urllib.request import Request, urlopen
from urllib.parse import urlencode
from typing import Dict, Union, Iterator


API_KEY = os.getenv('LAST_FM_API_KEY')
SECRET = os.getenv('LAST_FM_SECRET')
SESSION_KEY = os.getenv('LAST_FM_SESSION_KEY')


class EventType(Enum):
    started = 'started'
    completed = 'completed'


@dataclass(frozen=True)
class Event:
    time: datetime
    event: EventType
    queue_id: str
    track_id: str
    album_id: str
    album_artist_id: str
    title: str
    album: str
    artist: str
    album_artist: str
    duration_seconds: int
    track_number: int
    disc_number: int
    musicbrainz_trackid: Optional[str]

    def __post_init__(self) -> None:
        assert self.time.tzinfo is not None

    @staticmethod
    def from_dict(data: Dict[str, Union[str, int]]) -> Event:
        event: str = data['event']
        time: str = data['time']
        args: Dict[str, Union[str, int, datetime, EventType]] = {
            # Ensure the argument is present, even when the value is not.
            'musicbrainz_trackid': None,
            **data,
            'time': datetime.fromisoformat(time.replace('Z', '+00:00')),
            'event': EventType(event),
        }
        return Event(**args)

    def format_scrobble(self, index: int) -> Dict[str, str]:
        """
        Format a completed event as parameters for a form/url-encoded request
        to scrobble the track.
        """
        assert self.event == EventType.started

        def indexed(key: str) -> str:
            return f'{key}[{index}]'

        result = {
            indexed('artist'): self.artist,
            indexed('track'): self.title,
            indexed('timestamp'): str(int(self.time.timestamp())),
            indexed('album'): self.album,
            indexed('trackNumber'): str(self.track_number),
            indexed('duration'): str(self.duration_seconds),
            # Last.fm says "The album artist - if this differs from the track artist."
            # But if we don't include it, it echos back empty string in the response.
            indexed('albumArtist'): self.album_artist,
        }

        if self.musicbrainz_trackid is not None:
            result[indexed('mbid')] = self.musicbrainz_trackid

        return result


def read_play_log(fname: str) -> Iterator[Event]:
    with open(fname, 'r', encoding='utf-8') as f:
        for line in f:
            yield Event.from_dict(json.loads(line))


def events_to_scrobble(events: Iterator[Event]) -> Iterator[Event]:
    """
    Return the started event of listens to scrobble.
    * Match up started and completed events.
    * Apply Last.fm requirements for when to scrobble.
    """
    try:
        prev_event = next(events)
    except StopIteration:
        return

    for event in events:
        duration = event.time - prev_event.time
        if (
            # Check that we have the end of a started-completed pair.
            event.event == EventType.completed
            and prev_event.event == EventType.started
            # Confirm that the pair matches.
            and event.queue_id == prev_event.queue_id
            and event.track_id == prev_event.track_id
            # Sanity check: confirm that the time between start and completed
            # agrees with the alleged duration of the track.
            and abs(duration.total_seconds() - event.duration_seconds) < 10.0
            # Last.fm requirement: The track must at least be 30 seconds long.
            # The playtime requirement is implied by the above check.
            and event.duration_seconds > 30
        ):
            # We yield the started event, not the completion event, because for
            # a scrobble we need the time at which the track started playing.
            yield prev_event

        prev_event = event


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


def cmd_scrobble(play_log: str) -> None:
    events = read_play_log(play_log)
    scrobble_events = events_to_scrobble(events)

    n_scrobbled = 0

    # Last.fm allows submitting batches of at most 50 scrobbles at once.
    for batch in iter_chunks(scrobble_events, n=50):
        req = format_batch_request(batch)
        response = json.load(urlopen(req))

        num_accepted = response['scrobbles']['@attr']['accepted']

        if num_accepted != len(batch):
            print(f'Error after {n_scrobbled} submissions:')
            print(json.dumps(response, indent=2))
            break

        else:
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
