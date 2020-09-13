#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2018 Ruud van Asseldonk

# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
process_listens.py -- Convert Listenbrainz json into tab-separated values.

Streaming parsing with Serde is a lot of work for this format, and the Serde
deriving stuff is also not great for this use case. So instead we pre-process
the listens into a tsv file which is a lot easier to parse in Rust. This is not
streaming either, but for now it will do.

Usage:
    process_listens.py listens.json > listens.tsv
"""

import json
import sys

if len(sys.argv) != 2:
    print(__doc__)
    sys.exit(1)

print('seconds_since_epoch\ttrack\tartist\talbum')

for listen in json.load(open(sys.argv[1], 'r', encoding='utf-8')):
    print(listen['listened_at'], end='\t')
    print(listen['track_metadata']['track_name'], end='\t')
    print(listen['track_metadata']['artist_name'], end='\t')
    print(listen['track_metadata']['release_name'])
