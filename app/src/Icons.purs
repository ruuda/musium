-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2021 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Icons
  ( Icons
  , load
  ) where

import Effect.Aff (Aff)
import Effect.Class (liftEffect)
import Dom (Element)
import Prelude

import Dom as Dom
import Model as Model

type Icons =
  { iconLibrary :: Element
  , iconArtist :: Element
  , iconAlbum :: Element
  , iconQueue :: Element
  , iconCurrent :: Element
  , iconSearch :: Element
  }

loadIcon :: String -> Aff Element
loadIcon path = do
  -- We load the svg, and render it, so it can be embedded directly in the DOM.
  -- This is nice for styling it with CSS.
  svgStr <- Model.getString path
  liftEffect $ Dom.renderHtml svgStr

load :: Aff Icons
load = do
  iconLibrary <- loadIcon "/tab-library.svg"
  iconArtist  <- loadIcon "/tab-artist.svg"
  iconAlbum   <- loadIcon "/tab-album.svg"
  iconQueue   <- loadIcon "/tab-queue.svg"
  iconCurrent <- loadIcon "/tab-queue.svg"
  iconSearch  <- loadIcon "/tab-album.svg"
  pure
    { iconLibrary
    , iconArtist
    , iconAlbum
    , iconQueue
    , iconCurrent
    , iconSearch
    }
