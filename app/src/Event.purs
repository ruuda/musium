-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Event
  ( Event (..)
  , HistoryMode (..)
  , SortField (..)
  , SortDirection (..)
  , SortMode
  ) where

import Prelude
import Model (Album, QueuedTrack, ScanStatus)
import Navigation (Location)

data HistoryMode
  = RecordHistory
  | NoRecordHistory

data SortField
  = SortReleaseDate
  | SortFirstSeen
  | SortDiscover

derive instance sortFieldEq :: Eq SortField

data SortDirection
  = SortIncreasing
  | SortDecreasing

type SortMode = { field :: SortField, direction :: SortDirection }

data Event
  = Initialize (Array Album) (Array QueuedTrack)
  | UpdateQueue (Array QueuedTrack)
  | NavigateTo Location HistoryMode
  | NavigateToArtist
  | NavigateToAlbum
  | ChangeViewport
  -- Sort the album list on this particular field, toggling the direction if we
  -- are already sorting on that field.
  | SetSortField SortField
  -- This event is generated internally after enqueueing a track, to
  -- immediately bring the queue in sync without having to refresh it fully.
  -- It can kick off a full refresh in the usual way.
  | EnqueueTrack QueuedTrack
  -- Generated periodically when a track is playing to signal that we need to
  -- update the progress bar.
  | UpdateProgress
  -- The user typed the keyboard shortcut for 'search'.
  | SearchKeyPressed
  -- A new scan status was received.
  | UpdateScanStatus ScanStatus
