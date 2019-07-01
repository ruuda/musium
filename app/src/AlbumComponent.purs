-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumComponent
  ( Slot
  , component
  ) where

import Data.Maybe (Maybe (..))
import Effect (Effect)
import Effect.Aff.Class (class MonadAff)
import Data.Newtype (unwrap)
import Data.Const (Const)
import Halogen as H
import Halogen.HTML as HH
import Halogen.HTML.Core (ClassName (..))
import Halogen.HTML.Events as HE
import Halogen.HTML.Properties as HP
import Prelude

import Model (Album (..), Track (..))
import Model as Model
import Cast as Cast

data LazyData a
  = Uninitialized
  | Loading
  | Available a

type State =
  { album  :: Album
  , tracks :: LazyData (Array Track)
  , isOpen :: Boolean
  }

data Action
  = Toggle
  | BeginLoad
  | PlayTrack Track

type Slot = H.Slot (Const Void) Void

component :: forall f o m. MonadAff m => H.Component HH.HTML f Album o m
component =
  H.mkComponent
    { initialState
    , render
    , eval: H.mkEval $ H.defaultEval
      { handleAction = handleAction
      }
    }

initialState :: Album -> State
initialState album =
  { album: album
  , tracks: Uninitialized
  , isOpen: false
  }

render :: forall m. State -> H.ComponentHTML Action () m
render state =
  let
    album = unwrap state.album
    expandedClass = if state.isOpen
      then [(ClassName "expanded")]
      else [(ClassName "collapsed")]
  in
    HH.li_ $
      [ HH.div
        [ HE.onClick $ const $ Just Toggle
          -- Begin loading eagerly on touch or mouse down,
          -- don't wait for the click. TODO: We could even
          -- start loading as the element scrolls into view.
        , HE.onMouseDown $ const $ Just BeginLoad
        , HE.onTouchStart $ const $ Just BeginLoad
        ]
        [ HH.img
          [ HP.src (Model.thumbUrl album.id)
          , HP.alt $ album.title <> " by " <> album.artist
          ]
        , HH.div
          [ HP.class_ (ClassName "album-header") ]
          [ HH.span
            [ HP.class_ (ClassName "title") ]
            [ HH.text album.title ]
          , HH.span
            [ HP.class_ (ClassName "album-artist") ]
            [ HH.text album.artist ]
          ]
        ]
      ] <> case state.tracks of
        Available tracks ->
          [ HH.ul
            [ HP.classes $ [ClassName "track-list"] <> expandedClass ]
            ( map renderTrack tracks )
          ]
        _ ->
          [ HH.ul
            [ HP.classes $ [ClassName "track-list"] <> expandedClass ]
            [ HH.li_ [] ]
          ]

renderTrack :: forall m. Track -> H.ComponentHTML Action () m
renderTrack (Track track) =
  let
    span class_ content =
      HH.span [HP.class_ (ClassName class_)] [content]
  in
    HH.li
      [ HE.onClick \_ -> Just $ PlayTrack (Track track) ] $
      [ HH.div
        [ HP.class_ (ClassName "track-duration") ]
        [ span "track" $ HH.text $ show track.trackNumber
        , span "duration" $ HH.text $ Model.formatDurationSeconds track.durationSeconds
        ]
      , HH.div
        [ HP.class_ (ClassName "track-header") ]
        [ span "title" $ HH.text track.title
        , span "artist" $ HH.text track.artist
        ]
      ]

handleAction :: forall o m. MonadAff m => Action -> H.HalogenM State Action () o m Unit
handleAction = case _ of
  BeginLoad -> do
    { tracks, album } <- H.get
    case tracks of
      -- If we haven't started loading, start now.
      -- TODO: Reflect load error in load state, allow retry.
      Uninitialized -> do
        H.modify_ $ _ { tracks = Loading }
        tracks <- H.liftAff $ Model.getTracks (unwrap album).id
        H.modify_ $ _ { tracks = Available tracks }
      _ -> pure unit

  Toggle ->
    H.modify_ $ \state -> state { isOpen = not state.isOpen }

  PlayTrack track -> do
    album <- H.gets _.album
    H.liftEffect $ playTrack album track

playTrack :: Album -> Track -> Effect Unit
playTrack (Album album) (Track track) =
  Cast.playTrack
    { discNumber:  track.discNumber
    , trackNumber: track.trackNumber
    , title:       track.title
    , artist:      track.artist
    , albumTitle:  album.title
    , albumArtist: album.artist
    , releaseDate: album.date
                   -- TODO: Find a way to make urls work on the local network.
    , imageUrl:    "http://192.168.1.103:8233" <> Model.coverUrl track.id
    , trackUrl:    "http://192.168.1.103:8233" <> Model.trackUrl track.id
    }
