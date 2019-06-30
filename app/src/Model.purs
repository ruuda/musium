module Model
  ( Album (..)
  , AlbumId (..)
  , getAlbums
  , thumbUrl
  ) where

import Prelude

import Affjax as Http
import Affjax.ResponseFormat as Http.ResponseFormat
import Data.Array (sortWith)
import Data.Argonaut.Decode (decodeJson, getField) as Json
import Data.Argonaut.Decode.Class (class DecodeJson)
import Data.Either (Either (..))
import Effect.Aff (Aff)
import Effect.Exception (Error, error)
import Control.Monad.Error.Class (class MonadThrow, throwError)

fatal :: forall m a. MonadThrow Error m => String -> m a
fatal = error >>> throwError

newtype AlbumId = AlbumId String

instance showAlbumId :: Show AlbumId where
  show (AlbumId id) = id

thumbUrl :: AlbumId -> String
thumbUrl (AlbumId id) = "/thumb/" <> id

newtype Album = Album
  { id :: AlbumId
  , title :: String
  , artist :: String
  , sortArtist :: String
  , date :: String
  }

instance decodeJsonAlbum :: DecodeJson Album where
  decodeJson json = do
    obj        <- Json.decodeJson json
    id         <- map AlbumId $ Json.getField obj "id"
    title      <- Json.getField obj "title"
    artist     <- Json.getField obj "artist"
    sortArtist <- Json.getField obj "sort_artist"
    date       <- Json.getField obj "date"
    pure $ Album { id, title, artist, sortArtist, date }

getAlbums :: Aff (Array Album)
getAlbums = do
  response <- Http.get Http.ResponseFormat.json "/albums"
  case response.body of
    Left err -> fatal $ "Failed to retrieve albums: " <> Http.printResponseFormatError err
    Right json -> case Json.decodeJson json of
      Left err -> fatal $ "Failed to parse albums: " <> err
      Right albums -> pure $ sortWith (\(Album a) -> a.date) albums
