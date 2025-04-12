-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2025 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module QueueView
  ( QueueView
  , new
  , setQueue
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Traversable (for_)
import Effect (Effect)
import Effect.Aff (Aff)
import Prelude

import Dom (Element)
import Html (Html)
import Html as Html
import Model (QueuedTrack (..))
import Model as Model
import Event (Event)

type QueueView =
  { queueView :: Element
  , queueList :: Element
  , postEvent :: Event -> Aff Unit
  }

new :: (Event -> Aff Unit) -> Html QueueView
new postEvent = Html.div $ do
  Html.setId "queue-view"
  queueView <- ask

  Html.div $ do
    Html.addClass "list-config"
    Html.text "TODO: Queue config"

  queueList <- Html.ul $ do
    Html.setId "queue-list"
    ask

  pure $
    { queueView
    , queueList
    , postEvent
    }

setQueue :: QueueView -> Array QueuedTrack -> Effect Unit
setQueue self queue = Html.withElement self.queueList $ do
  Html.clear
  case queue of
    [] -> Html.p $ do
      Html.text "The play queue is empty"
    _ -> for_ queue renderAlbum

renderAlbum :: QueuedTrack -> Html Unit
renderAlbum (QueuedTrack track) = Html.li $ do
  Html.addClass "track"
  Html.img (Model.thumbUrl track.albumId) (track.album <> " by " <> track.artist) $ do
    Html.addClass "thumb"
  Html.span $ do
    Html.addClass "title"
    Html.text track.title
  Html.span $ do
    Html.addClass "artist"
    Html.text $ track.artist
