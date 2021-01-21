-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2021 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module NavBar
  ( NavBarState
  , new
  ) where

import Control.Monad.Reader.Class (ask)
import Effect.Aff (Aff, launchAff_)
import Prelude

import Dom (Element)
import Event (Event)
import Event as Event
import Html (Html)
import Html as Html

type NavBarState =
  { navBar :: Element
  }

new :: (Event -> Aff Unit) -> Html NavBarState
new postEvent = Html.nav $ do
  Html.setId "navbar"
  Html.onClick $ launchAff_ $ postEvent $ Event.ClickStatusBar

  let
    navTab title = Html.div $ do
      Html.addClass "nav-tab"
      Html.text title

  navTab "Library"
  navTab "Artist"
  navTab "Album"
  navTab "Queue"
  navTab "Now Playing"
  navTab "Search"

  navBar <- ask
  pure { navBar }
