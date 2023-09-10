-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2020 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module NowPlaying
  ( NowPlayingState (..)
  , nothingPlayingInfo
  , nowPlayingInfo
  , volumeControls
  , updateProgressBar
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Array.NonEmpty as NonEmptyArray
import Effect.Aff (Aff, launchAff_)
import Effect (Effect)
import Effect.Class (liftEffect)
import Prelude
import Dom (Element)
import Time (Duration)

import Dom as Dom
import Event (Event)
import Event as Event
import Html (Html)
import Html as Html
import Model (Decibel (Decibel), QueuedTrack (QueuedTrack), Rating (Rating), Volume (Volume))
import Model as Model
import Navigation as Navigation
import StatusBar as StatusBar
import Time as Time

volumeControls :: Html Unit
volumeControls = Html.div $ do
  Html.addClass "volume-controls"
  { volumeBar, label } <- Html.div $ do
    Html.addClass "indicator"
    Html.div $ do
      label <- Html.div $ do
        Html.addClass "volume-label"
        ask
      volumeBar <- ask
      pure $ { volumeBar, label }

  -- Now that we have the elements that display the current volume,
  -- define a function that alters those elements to display a certain volume.
  let
    setVolume (Decibel v) = do
      -- Use -20 dB as the minimum of the bar and 10 dB as the maximum.
      -- Set the minimum width to 1% so you can see that there is something that
      -- fills the volume bar.
      let percentage = max 1.0 $ min 100.0 $ (v + 20.0) / 0.3
      Dom.setWidth (show percentage <> "%") volumeBar
      Html.withElement label $ do
        Html.clear
        Html.text $ show v <> " dB"

    changeVolume dir = liftEffect $ launchAff_ $ do
      Volume newVolume <- Model.changeVolume dir
      liftEffect $ setVolume newVolume.volume

  -- Fetch the initial volume.
  liftEffect $ launchAff_ $ do
    Volume v <- Model.getVolume
    liftEffect $ setVolume v.volume

  Html.button $ do
    Html.addClass "volume-down"
    Html.text "V-"
    Html.onClick $ changeVolume Model.VolumeDown
  Html.button $ do
    Html.addClass "volume-up"
    Html.text "V+"
    Html.onClick $ changeVolume Model.VolumeUp

data NowPlayingState
  = StatePlaying { progressBar :: Element }
  | StateNotPlaying

nowPlayingInfo :: (Event -> Aff Unit) -> QueuedTrack -> Html NowPlayingState
nowPlayingInfo postEvent (QueuedTrack track) = Html.div $ do
  let
    onClickGoTo target = Html.onClick $ launchAff_ $
      postEvent $ Event.NavigateTo target Event.RecordHistory

  -- This structure roughly follows that of the album view.
  Html.addClass "album-info"
  Html.div $ do
    Html.addClass "cover-area"
    Html.div $ do
      Html.addClass "cover"
      let alt = track.title <> " by " <> track.artist
      Html.img (Model.thumbUrl track.albumId) alt $ Html.addClass "backdrop"
      Html.img (Model.thumbUrl track.albumId) alt $ Html.addClass "lowres"
      Html.img (Model.coverUrl track.albumId) alt $ pure unit
      onClickGoTo $ Navigation.Album track.albumId
    ratingButtons $ track.rating

  Html.div $ do
    Html.addClass "current-info"

    Html.hgroup $ do
      Html.h1 $ Html.text track.title
      Html.h2 $ do
        Html.addClass "artist"
        Html.text track.artist
        -- TODO: Figure out a way to navigate in case of multiple album artists.
        onClickGoTo $ Navigation.Artist $ NonEmptyArray.head track.albumArtistIds
      Html.h2 $ do
        Html.span $ do
          Html.addClass "album-title"
          Html.text track.album
          onClickGoTo $ Navigation.Album track.albumId
        Html.text " ⋅ "
        Html.span $ do
          Html.addClass "date"
          Html.text track.releaseDate

    Html.div $ do
      Html.addClass "waveform"
      Html.setMaskImage $ "url(" <> (Model.waveformUrl track.trackId) <> ")"
      Html.div $ do
        Html.addClass "progress"
        StatusBar.setInitialProgress (QueuedTrack track)
        self <- ask
        pure $ StatePlaying { progressBar: self }

ratingButtons :: Rating -> Html Unit
ratingButtons (Rating rating) = Html.div $ do
  Html.addClass "rating-buttons"
  Html.button $ do
    Html.text "✖"
    Html.setTitle "Rate as 'dislike'"
    when (rating == (-1)) $ Html.addClass "active"
    Html.onClick $ pure unit
  Html.button $ do
    Html.text "•"
    Html.setTitle "Rate as neutral (clear rating)"
    when (rating == 0) $ Html.addClass "active"
    Html.onClick $ pure unit
  Html.button $ do
    Html.text "★"
    Html.setTitle "Rate as 'like'"
    when (rating == 1) $ Html.addClass "active"
    Html.onClick $ pure unit
  Html.button $ do
    Html.text "❤"
    when (rating == 2) $ Html.addClass "active"
    Html.setTitle "Rate as 'love'"
    Html.onClick $ pure unit

nothingPlayingInfo :: Html NowPlayingState
nothingPlayingInfo = Html.div $ do
  Html.addClass "nothing-playing"
  Html.text "Nothing playing right now"
  pure StateNotPlaying

updateProgressBar :: QueuedTrack -> NowPlayingState -> Effect Duration
updateProgressBar currentTrack state = case state of
  StateNotPlaying -> pure $ Time.fromSeconds 0.1
  StatePlaying { progressBar } -> do
    StatusBar.updateProgressElement progressBar currentTrack
