-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module View (component) where

import Data.Maybe (Maybe (..))
import Data.Symbol (SProxy (..))
import Effect.Aff.Class (class MonadAff)
import Effect.Class.Console as Console
import Halogen as H
import Halogen.HTML as HH
import Halogen.HTML.Properties as HP
import Prelude

import Model (Album (..), AlbumId)
import Model as Model

import AlbumComponent as AlbumComponent

type State =
  { isLoaded :: Boolean
  , albums :: Array Album
  }

data Action
  = BeginLoad
  | LoadAlbum AlbumId

type Slots =
  ( album :: AlbumComponent.Slot AlbumId
  )

component :: forall f i o m. MonadAff m => H.Component HH.HTML f i o m
component =
  H.mkComponent
    { initialState
    , render
    , eval: H.mkEval $ H.defaultEval
      { handleAction = handleAction
      , initialize = Just BeginLoad
      }
    }

initialState :: forall i. i -> State
initialState = const
  { isLoaded: false
  , albums: []
  }

render :: forall m. MonadAff m => State -> H.ComponentHTML Action Slots m
render state =
  if not state.isLoaded
    then
      HH.p_ [ HH.text "Loading albums ..." ]
    else
      HH.div_
        [ HH.ul
          [ HP.id_ "album-list" ]
          (map renderAlbum state.albums)
        ]

renderAlbum :: forall m. MonadAff m => Album -> H.ComponentHTML Action Slots m
renderAlbum album@(Album { id }) =
  HH.slot (SProxy :: SProxy "album") (id) AlbumComponent.component album absurd

handleAction :: forall o m. MonadAff m => Action -> H.HalogenM State Action Slots o m Unit
handleAction = case _ of
  BeginLoad -> do
    albums <- H.liftAff Model.getAlbums
    H.modify_ $ _ { isLoaded = true, albums = albums }
  LoadAlbum albumId -> do
    H.liftAff $ Console.log $ "load album" <> (show albumId)
