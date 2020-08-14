-- Mindec -- Music metadata indexer
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Event
  ( Event (..)
  ) where

import Model (Album, QueuedTrack)

data Event
  = Initialize (Array Album)
  | UpdateQueue (Array QueuedTrack)
  | OpenLibrary
  | OpenAlbum Album
  | ChangeViewport
