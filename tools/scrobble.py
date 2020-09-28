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
from typing import Dict, Union


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
        args = {
            **data,
            'time': datetime.fromisoformat(data['time'].replace('Z', '+00:00')),
            'event': EventType(data['event']),
        }
        return Event(**args)


def read_play_log(fname: str) -> Iterable[Event]:
    with open(fname, 'r', encoding='utf-8') as f:
        for line in f:
            yield Event.from_dict(json.loads(line))


def main(play_log: str) -> None:
    for event in read_play_log(play_log):
        print(event)


if __name__ == '__main__':
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(1)
    else:
        main(sys.argv[1])
