module Main where

import Effect (Effect)
import Effect.Console as Console
import Prelude

greet :: String -> String
greet name = "Hello, " <> name <> "!"

main :: Effect Unit
main = Console.log (greet "World")
