-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Html
  ( Html
  , addClass
  , button
  , clear
  , div
  , h1
  , h2
  , h3
  , hgroup
  , img
  , input
  , li
  , nav
  , node
  , onClick
  , onInput
  , onScroll
  , p
  , removeClass
  , scrollIntoView
  , setHeight
  , setId
  , setScrollTop
  , setTitle
  , setTransform
  , setTransition
  , span
  , text
  , ul
  , withElement
  ) where

import Control.Monad.Reader.Trans (ReaderT (..))
import Dom (Element)
import Dom as Dom
import Effect (Effect)
import Prelude

-- An effect that builds nodes and appends them to the parent.
type Html a = ReaderT Element Effect a

withElement :: forall a. Element -> Html a -> Effect a
withElement container (ReaderT f) = f container

clear :: Html Unit
clear = ReaderT $ \container -> Dom.clearElement container

node :: forall a. String -> Html a -> Html a
node tagName (ReaderT children) =
  ReaderT $ \container -> do
    self <- Dom.createElement tagName
    result <- children self
    Dom.appendChild self container
    pure result

text :: String -> Html Unit
text value = ReaderT $ \container -> Dom.appendText value container

setHeight :: String -> Html Unit
setHeight id = ReaderT $ \container -> Dom.setHeight id container

setId :: String -> Html Unit
setId id = ReaderT $ \container -> Dom.setId id container

setTransform :: String -> Html Unit
setTransform transform = ReaderT $ \container -> Dom.setTransform transform container

setTransition :: String -> Html Unit
setTransition transition = ReaderT $ \container -> Dom.setTransition transition container

setTitle :: String -> Html Unit
setTitle title = ReaderT $ \self -> Dom.setAttribute "title" title self

addClass :: String -> Html Unit
addClass className = ReaderT $ \container -> Dom.addClass className container

removeClass :: String -> Html Unit
removeClass className = ReaderT $ \container -> Dom.removeClass className container

onClick :: Effect Unit -> Html Unit
onClick callback = ReaderT $ \container ->
  Dom.addEventListener "click" callback container

onScroll :: Effect Unit -> Html Unit
onScroll callback = ReaderT $ \container ->
  Dom.onScroll callback container

onInput :: (String -> Effect Unit) -> Html Unit
onInput callback = ReaderT $ \container ->
  let
    getValueAndCall = do
      value <- Dom.getValue container
      callback value
  in
    Dom.addEventListener "input" getValueAndCall container

scrollIntoView :: Html Unit
scrollIntoView = ReaderT Dom.scrollIntoView

setScrollTop :: Number -> Html Unit
setScrollTop off = ReaderT $ \container -> Dom.setScrollTop off container

div :: forall a. Html a -> Html a
div children = node "div" children

h1 :: forall a. Html a -> Html a
h1 children = node "h1" children

h2 :: forall a. Html a -> Html a
h2 children = node "h2" children

h3 :: forall a. Html a -> Html a
h3 children = node "h3" children

p :: forall a. Html a -> Html a
p children = node "p" children

button :: forall a. Html a -> Html a
button children = node "button" children

hgroup :: forall a. Html a -> Html a
hgroup children = node "hgroup" children

nav :: forall a. Html a -> Html a
nav children = node "nav" children

span :: forall a. Html a -> Html a
span children = node "span" children

ul :: forall a. Html a -> Html a
ul children = node "ul" children

li :: forall a. Html a -> Html a
li children = node "li" children

img :: forall a. String -> String -> Html a -> Html a
img src alt (ReaderT children) = ReaderT $ \container -> do
  self <- Dom.createElement "img"
  Dom.setImage src alt self
  result <- children self
  Dom.appendChild self container
  pure result

input :: forall a. String -> Html a -> Html a
input placeholder (ReaderT children) =
  node "input" $ ReaderT $ \self -> do
    Dom.setAttribute "placeholder" placeholder self
    Dom.setAttribute "autocomplete" "off" self
    children self
