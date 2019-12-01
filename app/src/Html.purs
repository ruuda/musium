-- Mindec -- Music metadata indexer
-- Copyright 2019 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

module Html
  ( Html
  , addClass
  , appendTo
  , div
  , img
  , input
  , li
  , node
  , onClick
  , onInput
  , removeClass
  , setId
  , span
  , text
  , ul
  ) where

import Control.Monad.Reader.Trans (ReaderT (..))
import Dom (Element)
import Dom as Dom
import Effect (Effect)
import Prelude

-- An effect that builds nodes and appends them to the parent.
type Html a = ReaderT Element Effect a

appendTo :: Element -> Html Unit -> Effect Unit
appendTo container (ReaderT f) = f container

node :: forall a. String -> Html a -> Html a
node tagName (ReaderT children) =
  ReaderT $ \container -> do
    self <- Dom.createElement tagName
    result <- children self
    Dom.appendChild self container
    pure result

text :: String -> Html Unit
text value = ReaderT $ \container -> Dom.appendText value container

setId :: String -> Html Unit
setId id = ReaderT $ \container -> Dom.setId id container

addClass :: String -> Html Unit
addClass className = ReaderT $ \container -> Dom.addClass className container

removeClass :: String -> Html Unit
removeClass className = ReaderT $ \container -> Dom.removeClass className container

onClick :: Effect Unit -> Html Unit
onClick callback = ReaderT $ \container ->
  Dom.addEventListener "click" callback container

onInput :: (String -> Effect Unit) -> Html Unit
onInput callback = ReaderT $ \container ->
  let
    getValueAndCall = do
      value <- Dom.getValue container
      callback value
  in
    Dom.addEventListener "input" getValueAndCall container

div :: forall a. Html a -> Html a
div children = node "div" children

span :: forall a. Html a -> Html a
span children = node "span" children

ul :: forall a. Html a -> Html a
ul children = node "ul" children

li :: forall a. Html a -> Html a
li children = node "li" children

img :: String -> String -> Html Unit
img src alt = ReaderT $ \container -> do
  self <- Dom.createElement "img"
  Dom.setAttribute "src" src self
  Dom.setAttribute "alt" alt self
  Dom.appendChild self container

input :: forall a. String -> Html a -> Html a
input placeholder (ReaderT children) =
  node "input" $ ReaderT $ \self -> do
    Dom.setAttribute "placeholder" placeholder self
    children self
