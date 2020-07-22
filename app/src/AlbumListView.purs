-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumListView
  ( renderAlbumList
  ) where

import Data.Array as Array
import Data.Foldable (traverse_)
import Data.String.CodeUnits as CodeUnits
import Effect.Aff (Aff, launchAff)
import Prelude

import Html (Html)
import Html as Html
import Model (Album (..))
import Model as Model
import Event (Event)
import Event as Event

renderAlbumList :: (Event -> Aff Unit) -> Array Album -> Html Unit
renderAlbumList postEvent albums = do
  -- A sentinel element to grow the album list to the right size so the scroll
  -- bar is correct, even though not all entries are present yet.
  Html.div $ do
    -- An album entry is 4em tall.
    let height = 4 * Array.length albums
    Html.setId "runway-sentinel"
    Html.setTransform $ "translate(0em, " <> (show height) <> "em)"

  Html.ul $ do
    Html.setId "album-list"
    traverse_ (renderAlbum postEvent) albums

renderAlbum :: (Event -> Aff Unit) -> Album -> Html Unit
renderAlbum postEvent (Album album) =
  Html.li $ do
    Html.addClass "album-container"
    Html.div $ do
      Html.addClass "album"
      Html.img (Model.thumbUrl album.id) (album.title <> " by " <> album.artist) $ do
        Html.addClass "thumb"
      Html.span $ do
        Html.addClass "title"
        Html.text album.title
      Html.span $ do
        Html.addClass "artist"
        Html.text $ album.artist <> " "
        Html.span $ do
          Html.addClass "date"
          Html.setTitle album.date
          -- The date is of the form YYYY-MM-DD in ascii, so we can safely take
          -- the first 4 characters to get the year.
          Html.text (CodeUnits.take 4 album.date)

      Html.onClick $ void $ launchAff $ postEvent $ Event.OpenAlbum $ Album album
