-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Event
  ( Event (..)
  , HistoryMode (..)
  ) where

import Model (Album, QueuedTrack)
import Navigation (Location)

data HistoryMode
  = RecordHistory
  | NoRecordHistory

data Event
  = Initialize (Array Album)
  | UpdateQueue (Array QueuedTrack)
  | NavigateTo Location HistoryMode
  | NavigateToArtist
  | NavigateToAlbum
  | ChangeViewport
    -- This event is generated internally after enqueueing a track, to
    -- immediately bring the queue in sync without having to refresh it fully.
    -- It can kick off a full refresh in the usual way.
  | EnqueueTrack QueuedTrack
    -- Generated periodically when a track is playing to signal that we need to
    -- update the progress bar.
  | UpdateProgress
