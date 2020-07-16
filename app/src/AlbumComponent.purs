-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumComponent
  ( renderAlbum
  ) where

import Control.Monad.Reader.Class (ask, local)
import Data.Maybe (Maybe (Just))
import Data.String.CodeUnits as CodeUnits
import Effect.Class (liftEffect)
import Prelude

import AlbumView as AlbumView
import Dom as Dom
import History as History
import Html (Html)
import Html as Html
import Model (Album (..))
import Model as Model
import Var as Var

renderAlbum :: Album -> Html Unit
renderAlbum (Album album) =
  Html.li $ do
    Html.addClass "album-container"
    header <- Html.div $ do
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
      ask

    isLoadedVar <- liftEffect $ Var.create false
    isOpenVar <- liftEffect $ Var.create false

    local (const header) $ do
      Html.onClick $ do
        Html.withElement Dom.body $ AlbumView.renderAlbum $ Album album
        History.pushState (Just album) (album.title <> " by " <> album.artist) ("/album/" <> show album.id)
