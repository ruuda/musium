-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module AlbumListView
  ( AlbumListView
  , ScrollState
  , Slice
  , new
  , setAlbums
  , setSortMode
  , updateViewport
  ) where

import Control.Monad.Reader.Class (ask)
import Data.Array as Array
import Data.Int as Int
import Data.Maybe (Maybe (Just, Nothing))
import Data.String.CodeUnits as CodeUnits
import Data.Traversable (for_, sequence, sequence_)
import Effect (Effect)
import Effect.Aff (Aff, launchAff)
import Effect.Aff as Aff
import Prelude
import Test.Assert (assert', assertEqual')

import Dom (Element)
import Dom as Dom
import Event (Event, HistoryMode (RecordHistory), SortField (..), SortDirection (..), SortMode)
import Event as Event
import Html (Html)
import Html as Html
import Model (Album (..))
import Model as Model
import Navigation as Navigation

-- Render the "runway" in which albums can sroll, but put no contents in it.
-- The contents are added later by 'updateViewport'.
renderAlbumListRunway :: Int -> Html Unit
renderAlbumListRunway numAlbums = do
  Html.setId "album-list"
  -- An album entry is 4em tall.
  Html.setHeight $ (show $ 4 * numAlbums) <> "em"

-- A slice of the albums array, with inclusive begin and exclusive end indices.
type Slice =
  { begin :: Int
  , end :: Int
  }

-- The currently rendered albums, and which slice of the albums array that is.
type ScrollState =
  { elements :: Array Element
  , begin :: Int
  , end :: Int
  }

type Split =
  { shared :: ScrollState
  , residue :: Array Element
  }

-- Split the state into a shared part that intersects the target, and a residue
-- that can be reused.
split3 :: Slice -> ScrollState -> Split
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

assertOk :: ScrollState -> Effect Unit
assertOk state = assertEqual'
  "Elements array must contain as many elements as the covered range."
  { actual: Array.length state.elements, expected: state.end - state.begin }

-- Mutate the album list DOM nodes to ensure that the desired slice is rendered.
updateElements
  :: Array Album
  -> (Event -> Aff Unit)
  -> Element
  -> Slice
  -> ScrollState
  -> Effect ScrollState
updateElements albums postEvent albumList target state = do
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
        newElems <- sequence $ Array.replicate d $ Html.withElement albumList $ Html.li ask
        pure $ split.residue <> newElems
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
    Html.setBackgroundColor album.color
    Html.addClass "thumb"
  Html.span $ do
    Html.addClass "title"
    Html.text album.title
  Html.span $ do
    Html.addClass "artist"
    Html.text $ album.artist <> " "
    Html.span $ do
      Html.addClass "date"
      Html.setTitle album.releaseDate
      -- The date is of the form YYYY-MM-DD in ascii, so we can safely take
      -- the first 4 characters to get the year.
      Html.text (CodeUnits.take 4 album.releaseDate)

  Html.onClick $ void $ launchAff $ postEvent $
    Event.NavigateTo (Navigation.Album album.id) RecordHistory

type AlbumListView =
  { scrollState :: ScrollState
  , albums :: Array Album
  , albumListView :: Element
  , albumListRunway :: Element
  , postEvent :: Event -> Aff Unit
  , getOption :: SortField -> Element
  }

new :: (Event -> Aff Unit) -> Html AlbumListView
new postEvent = Html.div $ do
  Html.addClass "album-list-view"
  Html.onScroll $ Aff.launchAff_ $ postEvent $ Event.ChangeViewport

  getOption <- renderSortOptions postEvent

  albumListRunway <- Html.ul $ do
    renderAlbumListRunway 0
    ask

  albumListView <- ask
  pure
    { albumListView
    , albumListRunway
    , postEvent
    , getOption: getOption
    , albums: []
    , scrollState:
      { elements: []
      , begin: 0
      , end: 0
      }
    }

renderSortOptions :: (Event -> Aff Unit) -> Html (SortField -> Element)
renderSortOptions postEvent = Html.div $ do
  Html.addClass "list-config"
  let onClickPost field = Html.onClick $ void $ launchAff $ postEvent $ Event.SetSortField field
  optReleaseDate <- Html.div $ do
    Html.addClass "config-option"
    Html.text "Date"
    onClickPost SortReleaseDate
    ask
  optFirstSeen <- Html.div $ do
    Html.addClass "config-option"
    Html.text "First Seen"
    onClickPost SortFirstSeen
    ask
  optDiscover <- Html.div $ do
    Html.addClass "config-option"
    Html.text "Discover"
    onClickPost SortDiscover
    ask
  optTrending <- Html.div $ do
    Html.addClass "config-option"
    Html.text "Trending"
    onClickPost SortTrending
    ask
  optForNow <- Html.div $ do
    Html.addClass "config-option"
    Html.text "For Now"
    onClickPost SortForNow
    ask

  pure $ case _ of
    SortReleaseDate -> optReleaseDate
    SortFirstSeen   -> optFirstSeen
    SortDiscover    -> optDiscover
    SortTrending    -> optTrending
    SortForNow      -> optForNow

setSortMode :: SortMode -> AlbumListView -> Effect Unit
setSortMode { field, direction } state =
  let
    allFields = [SortReleaseDate, SortFirstSeen, SortDiscover, SortTrending, SortForNow]
    unsort = do
      Html.removeClass "increasing"
      Html.removeClass "decreasing"
  in
    for_ allFields $ \toInspect -> Html.withElement (state.getOption toInspect) $
      if toInspect == field
        then do
          Html.addClass "active"
          unsort
          Html.addClass $ case direction of
            SortIncreasing -> "increasing"
            SortDecreasing -> "decreasing"
        else do
          Html.removeClass "active"
          unsort

setAlbums :: Array Album -> AlbumListView -> Effect AlbumListView
setAlbums albums state = do
  -- TODO: Add a way to select a particular scroll position.
  Html.withElement state.albumListRunway $ do
    Html.clear
    renderAlbumListRunway $ Array.length albums

  updateViewport $ state
    { albums = albums
    , scrollState =
      { elements: []
      , begin: 0
      , end: 0
      }
    }

-- Bring the album list in sync with the viewport (the album list index and
-- the number of entries per viewport).
updateViewport :: AlbumListView -> Effect AlbumListView
updateViewport state = do
  -- To determine a good target, we need to know how tall an entry is, so we
  -- need to have at least one already. If we don't, then we take a slice of
  -- a single item to start with, and enqueue an event to update again after
  -- this update.
  { target, index: _, redo } <- case Array.head state.scrollState.elements of
    Nothing -> do
      pure
        { target: { begin: 0, end: min 1 (Array.length state.albums) }
        , index: 0
        , redo: Array.length state.albums > 0
        }
    Just elem -> do
      entryHeight <- Dom.getOffsetHeight elem
      viewportHeight <- Dom.getOffsetHeight state.albumListView
      y <- Dom.getScrollTop state.albumListView
      let
        headroom = 20
        i = Int.floor $ y / entryHeight
        -- This may be an overestimate due to the sort options, but that's okay.
        albumsVisible = Int.ceil $ viewportHeight / entryHeight
      pure
        { target:
          { begin: max 0 (i - headroom)
          , end: min (Array.length state.albums) (i + headroom + albumsVisible)
          }
        , index: i
        , redo: false
        }

  scrollState <- updateElements
    state.albums
    state.postEvent
    state.albumListRunway
    target
    state.scrollState

  -- If we needed to add a single element first, for measurement, then do a
  -- second iteration, ortherwise we are done now.
  (if redo then updateViewport else pure) state { scrollState = scrollState }
