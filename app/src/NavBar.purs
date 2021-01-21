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
import Effect.Aff (Aff, launchAff_)
import Prelude

import Dom (Element)
import Event (Event)
import Event as Event
import Html (Html)
import Html as Html
import Icons (Icons)
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

new :: Icons -> (Event -> Aff Unit) -> Html NavBarState
new icons postEvent = Html.nav $ do
  Html.setId "navbar"
  Html.onClick $ launchAff_ $ postEvent $ Event.ClickStatusBar

  let
    navTab :: String -> String -> Html Element
    navTab title id = Html.div $ do
      Html.addClass "nav-tab"
      Html.span $ Html.pureElement icons.iconLibrary
      Html.span $ do
        Html.setId id
        Html.text title
      ask

  tabLibrary    <- navTab "Library" "tab-library"
  tabArtist     <- navTab "Artist"  "tab-artist"
  tabAlbum      <- navTab "Album"   "tab-album"
  tabQueue      <- navTab "Queue"   "tab-queue"
  tabNowPlaying <- navTab "Current" "tab-current"
  tabSearch     <- navTab "Search"  "tab-search"

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
      Navigation.NowPlaying -> state.tabNowPlaying
      Navigation.Search     -> state.tabSearch
      Navigation.Album _    -> state.tabAlbum
