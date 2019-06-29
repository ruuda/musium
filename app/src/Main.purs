module Main where

import Effect (Effect)
import Effect.Console (log)
import Prelude

greet :: String -> String
greet name = "Hello, " <> name <> "!"

main :: Effect Unit
main = log (greet "World")
