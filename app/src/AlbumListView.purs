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
import Data.Traversable (for_, sequence, sequence_)
import Effect (Effect)
import Effect.Aff (Aff, launchAff)
import Prelude
import Test.Assert (assert', assertEqual')

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

type Split =
  { shared :: AlbumListState
  , residue :: Array Element
  }

-- An empty album list state with 'begin' set to the given index.
emptyAt :: Int -> AlbumListState
emptyAt i = { elements: [], begin: i, end: i }

-- Split the state into a shared part that intersects the target, and a residue
-- that can be reused.
split3 :: Slice -> AlbumListState -> Split
split3 target state =
  let
    begin   = min state.end $ max state.begin target.begin
    end     = max begin $ min state.end target.end
    k1      = begin - state.begin
    k2      = end - state.begin
    shared  = { elements: Array.slice k1 k2 state.elements, begin: begin, end: end }
    residue = (Array.take k1 state.elements) <> (Array.drop k2 state.elements)
  in
    { shared, residue }

assertOk :: AlbumListState -> Effect Unit
assertOk state = assertEqual'
  "Elements array must contain as many elements as the covered range."
  { actual: Array.length state.elements, expected: state.end - state.begin }

-- Mutate the album list DOM nodes to ensure that the desired slice is rendered.
updateAlbumList
  :: Array Album
  -> (Event -> Aff Unit)
  -> Element
  -> Slice
  -> AlbumListState
  -> Effect AlbumListState
updateAlbumList albums postEvent albumList target state = do
  let
    split = split3 target state

    setAlbum index element = case Array.index albums index of
      Nothing    -> pure unit -- Logic error
      Just album -> do
        assert'
          "Elements in the shared slice should not be rewritten"
          (index < split.shared.begin || index >= split.shared.end)
        Html.withElement element $ do
          Html.clear
          Html.setTransform $ "translate(0em, " <> (show $ index * 4) <> "em)"
          renderAlbum postEvent album

  -- Ensure that we have precisely enough elements in the pool of <li>'s to
  -- recycle, destroying or creating them as needed.
  let
    nTotal = target.end - target.begin
    nShared = Array.length split.shared.elements
    nChange = nTotal - nShared
  residue <- case nChange - Array.length split.residue of
      d | d < 0 -> do
        for_ (Array.take (-d) split.residue) $ \elem -> Dom.removeChild elem albumList
        pure (Array.drop (-d) split.residue)
      d | d > 0 -> do
        new <- sequence $ Array.replicate d $ Html.withElement albumList $ Html.li ask
        pure $ split.residue <> new
      _ -> pure split.residue

  let
    n = split.shared.begin - target.begin
    prefix = Array.take n residue
    suffix = Array.drop n residue
    m = Array.length suffix

  sequence_ $ Array.mapWithIndex (\i -> setAlbum $ target.begin + i) prefix
  sequence_ $ Array.mapWithIndex (\i -> setAlbum $ target.end - m + i) suffix
  let
    result =
      { begin: target.begin
      , end: target.end
      , elements: prefix <> split.shared.elements <> suffix
      }
  assertOk result
  pure result

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
