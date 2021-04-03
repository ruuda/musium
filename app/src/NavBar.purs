-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2021 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module NavBar
  ( NavBarState
  , new
  , selectTab
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Foldable (traverse_)
import Effect (Effect)
import Effect.Class (liftEffect)
import Effect.Aff (Aff, launchAff_)
import Prelude

import Dom (Element)
import Event (Event)
import Event as Event
import Html (Html)
import Html as Html
import Navigation (Location)
import Navigation as Navigation

type NavBarState =
  { navBar :: Element
  , tabLibrary :: Element
  , tabArtist :: Element
  , tabAlbum :: Element
  , tabQueue :: Element
  , tabNowPlaying :: Element
  , tabSearch :: Element
  }

new :: (Event -> Aff Unit) -> Html NavBarState
new postEvent = Html.nav $ do
  Html.setId "navbar"

  let
    navTab :: String -> Event -> Html Element
    navTab title event = Html.div $ do
      Html.addClass "nav-tab"
      Html.text title
      Html.onClick $ launchAff_ $ postEvent event
      ask

    navEvent loc = Event.NavigateTo loc Event.RecordHistory

  tabLibrary    <- navTab "Library" $ navEvent Navigation.Library
  tabArtist     <- navTab "Artist"  $ Event.NavigateToArtist
  tabAlbum      <- navTab "Album"   $ Event.NavigateToAlbum
  tabQueue      <- navTab "Queue"   $ navEvent Navigation.Library -- TODO
  tabNowPlaying <- navTab "Current" $ navEvent Navigation.NowPlaying
  tabSearch     <- navTab "Search"  $ navEvent Navigation.Search

  -- The library is active at startup.
  liftEffect $ Html.withElement tabLibrary $ Html.addClass "active"

  navBar <- ask
  pure
    { navBar
    , tabLibrary
    , tabArtist
    , tabAlbum
    , tabQueue
    , tabNowPlaying
    , tabSearch
    }

tabs :: NavBarState -> Array Element
tabs state =
  [ state.tabLibrary
  , state.tabArtist
  , state.tabAlbum
  , state.tabQueue
  , state.tabNowPlaying
  , state.tabSearch
  ]

selectTab :: Location -> NavBarState -> Effect Unit
selectTab location state =
  let
    deactivate element = Html.withElement element $ Html.removeClass "active"
    activate element   = Html.withElement element $ Html.addClass "active"
  in do
    traverse_ deactivate $ tabs state
    activate $ case location of
      Navigation.Library    -> state.tabLibrary
      Navigation.Artist _   -> state.tabArtist
      Navigation.Album _    -> state.tabAlbum
      Navigation.NowPlaying -> state.tabNowPlaying
      Navigation.Search     -> state.tabSearch
