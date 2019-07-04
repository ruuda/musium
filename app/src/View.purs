-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module View (component) where

import Data.Array as Array
import Data.Maybe (Maybe (..))
import Data.String as String
import Data.Symbol (SProxy (..))
import Effect.Aff.Class (class MonadAff)
import Effect.Class.Console as Console
import Halogen as H
import Halogen.HTML as HH
import Halogen.HTML.Core (ClassName (..))
import Halogen.HTML.Events as HE
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
      HH.div
        [ HP.id_ "loader"
        , HP.class_ (ClassName "spinner")
        ]
        [ HH.div_ []
        , HH.div_ []
        , HH.p_ [ HH.text "Loading albums …" ]
        ]
    else
      let
        -- TODO: Turn into a component with onTouchEnter and onTouchLeave for style.
        yearButton year = HH.a
          [ HP.href $ "#" <> year ]
          [ HH.p
            [ HP.class_ $ ClassName "year-pointer" ]
            [ HH.text year ]
          ]
        years
          = Array.nub
          $ map Model.originalReleaseYear
          $ state.albums
        scrollbar = HH.div
          [ HP.id_ "album-list-scroll" ]
          ( map yearButton years )
      in
        HH.div_
          [ HH.ul
            [ HP.id_ "album-list" ]
            (map renderAlbum state.albums)
          , scrollbar
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
