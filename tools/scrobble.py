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
    scrobble.py plays.log
"""

from __future__ import annotations

import sys
import json

from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from typing import Dict, Union, Iterator


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

    def __post_init__(self) -> None:
        assert self.time.tzinfo is not None

    @staticmethod
    def from_dict(data: Dict[str, Union[str, int]]) -> Event:
        event: str = data['event']
        time: str = data['time']
        args: Dict[str, Union[str, int, datetime, EventType]] = {
            **data,
            'time': datetime.fromisoformat(time.replace('Z', '+00:00')),
            'event': EventType(event),
        }
        return Event(**args)


def read_play_log(fname: str) -> Iterator[Event]:
    with open(fname, 'r', encoding='utf-8') as f:
        for line in f:
            yield Event.from_dict(json.loads(line))


def events_to_scrobble(events: Iterator[Event]) -> Iterator[Event]:
    """
    Return the completed event of listens to scrobble.
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
            yield event

        prev_event = event


def main(play_log: str) -> None:
    events = read_play_log(play_log)
    scrobble_events = events_to_scrobble(events)
    for event in scrobble_events:
        print(event)


if __name__ == '__main__':
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(1)
    else:
        main(sys.argv[1])
