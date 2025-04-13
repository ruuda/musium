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
import Effect.Aff (Aff, launchAff)
import Prelude

import Dom (Element)
import Html (Html)
import Html as Html
import Model (QueuedTrack (..))
import Model as Model
import Event (Event)
import Event as Event

type QueueView =
  { queueView :: Element
  , queueList :: Element
  , postEvent :: Event -> Aff Unit
  }

new :: (Event -> Aff Unit) -> Html QueueView
new postEvent = Html.div $ do
  Html.setId "queue-view"
  Html.addClass "queue-empty"

  queueView <- ask

  queueList <- Html.div $ do
    Html.setId "queue-container"
    renderQueueActions postEvent
    Html.ul $ do
      Html.setId "queue-list"
      ask

  Html.p $ do
    Html.addClass "nothing-playing"
    Html.text "The play queue is empty"

  pure $
    { queueView
    , queueList
    , postEvent
    }

setQueue :: QueueView -> Array QueuedTrack -> Effect Unit
setQueue self queue = do
  case queue of
    [] ->
      Html.withElement self.queueView $ Html.addClass "queue-empty"

    _ -> do
      Html.withElement self.queueList $ do
        Html.clear
        for_ queue renderAlbum
      Html.withElement self.queueView $ Html.removeClass "queue-empty"

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

renderQueueActions :: (Event -> Aff Unit) -> Html Unit
renderQueueActions postEvent = Html.div $ do
  Html.addClass "list-config"
  let onClickPost event = Html.onClick $ void $ launchAff $ postEvent event
  Html.div $ do
    Html.addClass "config-option"
    Html.text "Shuffle"
    onClickPost Event.ShuffleQueue
  Html.div $ do
    Html.addClass "config-option"
    Html.text "Clear"
    onClickPost Event.ClearQueue
