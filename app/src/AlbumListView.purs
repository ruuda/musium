-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumListView
  ( AlbumListState
  , Slice
  , renderAlbumListRunway
  , updateAlbumList
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Maybe (Maybe (Just, Nothing))
import Data.String.CodeUnits as CodeUnits
import Effect (Effect)
import Effect.Aff (Aff, launchAff)
import Prelude

import Dom (Element)
import Dom as Dom
import Html (Html)
import Html as Html
import Model (Album (..))
import Model as Model
import Event (Event)
import Event as Event

-- Render the "runway" in which albums can sroll, but put no contents in it.
-- The contents are added later by 'updateAlbumList'.
renderAlbumListRunway :: Int -> Html Element
renderAlbumListRunway numAlbums = do
  Html.ul $ do
    Html.setId "album-list"
    -- An album entry is 4em tall.
    Html.setHeight $ (show $ 4 * numAlbums) <> "em"
    ask

-- A slice of the albums array, with inclusive begin and exclusive end indices.
type Slice =
  { begin :: Int
  , end :: Int
  }

-- The currently rendered albums, and which slice of the albums array that is.
type AlbumListState =
  { elements :: Array Element
  , begin :: Int
  , end :: Int
  }

-- Mutate the album list DOM nodes to ensure that the desired slice is rendered.
-- Note: This is not the most efficient update, sometimes it will recycle nodes
-- only to delete them later. That can happen when shrinking the window. I think
-- that is an acceptable cost to keep this function simple.
updateAlbumList
  :: Array Album
  -> (Event -> Aff Unit)
  -> Element
  -> Slice
  -> AlbumListState
  -> Effect AlbumListState
updateAlbumList albums postEvent albumList target =
  let
    setAlbum index = case Array.index albums index of
      Nothing    -> pure unit -- Logic error
      Just album -> do
        Html.clear
        Html.setTransform $ "translate(0em, " <> (show $ index * 4) <> "em)"
        renderAlbum postEvent album

    extend :: AlbumListState -> Effect AlbumListState
    extend state = do
      element <- Html.withElement albumList $ Html.li $ do
        setAlbum state.end
        ask
      step $ state { end = state.end + 1, elements = Array.snoc state.elements element }

    shrink :: AlbumListState -> Effect AlbumListState
    shrink state = case Array.unsnoc state.elements of
      Nothing -> pure state -- Logic error
      Just { init, last } -> do
        Dom.removeChild last albumList
        step $ state { end = state.end - 1, elements = init }

    moveBeginToEnd :: AlbumListState -> Effect AlbumListState
    moveBeginToEnd state = case Array.uncons state.elements of
      Nothing -> pure state -- Logic error
      Just { head, tail } -> do
        Html.withElement head $ setAlbum $ state.end
        step $ state
          { begin = state.begin + 1
          , end = state.end + 1
          , elements = Array.snoc tail head
          }

    moveEndToBegin :: AlbumListState -> Effect AlbumListState
    moveEndToBegin state = case Array.unsnoc state.elements of
      Nothing -> pure state -- Logic error
      Just { init, last } -> do
        Html.withElement last $ setAlbum $ state.begin - 1
        step $ state
          { begin = state.begin - 1
          , end = state.end - 1
          , elements = Array.cons last init
          }

    step :: AlbumListState -> Effect AlbumListState
    step state =
      let
        dBegin = target.begin - state.begin
        dEnd   = target.end - state.end
        result
          | dBegin == 0 && dEnd == 0  = pure state
          | dBegin == 0 && dEnd >  0  = extend state
          | dBegin == 0 && dEnd <  0  = shrink state
          | Array.null state.elements = extend $ state { begin = target.begin, end = target.begin }
          | dBegin > 0                = moveBeginToEnd state
          | dBegin < 0                = moveEndToBegin state
          | otherwise                 = pure state -- Unreachable
      in
        result
  in
    step

renderAlbum :: (Event -> Aff Unit) -> Album -> Html Unit
renderAlbum postEvent (Album album) = Html.div $ do
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
