-- Mindec -- Music metadata indexer
-- Copyright 2018 Ruud van Asseldonk

-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

import Html exposing (Html)
import Html.Events as Html
import Html.Attributes as Html
import Http
import Json.Decode as Json
import Navigation exposing (Location)

apiHost : String
apiHost = "http://localhost:8233"

main =
  Navigation.program UrlChange
    { init = init
    , view = view
    , update = update
    , subscriptions = subscriptions
    }

-- DATA

type AlbumId = AlbumId String
type TrackId = TrackId String

type alias Album =
  { id : AlbumId
  , title : String
  , artist : String
  , sortArtist : String
  , date : String
  }

type alias Track =
  { id : TrackId
  , discNumber : Int
  , trackNumber : Int
  , title : String
  , artist : String
  , durationSeconds : Int
  }

-- Format total seconds in 'h:mm:ss' or 'm:ss' format.
formatDuration : Int -> String
formatDuration seconds =
  let
    h = seconds // 3600
    m = (seconds - h * 3600) // 60
    s = seconds - h * 3600 - m * 60
    mStr = String.padLeft 2 '0' (toString m)
    sStr = String.padLeft 2 '0' (toString s)
  in
     case h of
       0 -> (toString m) ++ ":" ++ sStr
       _ -> (toString h) ++ ":" ++ mStr ++ ":" ++ sStr

-- ROUTE

type Route
  = AtHome
  | AtAlbum (AlbumId)

parseRoute : String -> Maybe Route
parseRoute hash =
  -- /#
  if (hash == "") || (hash == "#") then
    Just AtHome
  -- /#album:f6758afbcfa3c6d3
  else if (String.startsWith "#album:" hash) && (String.length hash == 23) then
    Just (AtAlbum (AlbumId (String.dropLeft 7 hash)))
  else
    Nothing

routeAlbum : AlbumId -> String
routeAlbum (AlbumId id) = "#album:" ++ id

-- MODEL

type Model
  = Loading
  | AlbumList (List Album)
  | TrackList Album (List Track)
  | Failed String

init : Location -> (Model, Cmd Msg)
init location =
  handleUrl location Loading

-- UPDATE

type Msg
  = LoadAlbums (Result Http.Error (List Album))
  | LoadAlbum (Result Http.Error (Album, List Track))
  | UrlChange Location

handleUrl : Location -> Model -> (Model, Cmd Msg)
handleUrl location model =
   case parseRoute location.hash of
     Just AtHome -> (model, getAlbums)
     Just (AtAlbum aid) -> (model, getAlbum aid)
     Nothing -> (Failed "Invalid url.", Cmd.none)

update : Msg -> Model -> (Model, Cmd Msg)
update msg model =
  case msg of
    UrlChange location ->
      handleUrl location model
    LoadAlbums (Ok albums) ->
      (AlbumList (List.sortBy .date albums), Cmd.none)
    LoadAlbums (Err _) ->
      (Failed "Failed to retrieve album list.", Cmd.none)
    LoadAlbum (Ok (album, tracks)) ->
      (TrackList album tracks, Cmd.none)
    LoadAlbum (Err _) ->
      (Failed "Failed to retrieve album.", Cmd.none)

-- VIEW

view : Model -> Html Msg
view model =
  case model of
    Loading ->
      Html.text "loading ..."
    Failed message ->
      Html.text message
    AlbumList albums ->
      Html.div [Html.id "album-list"] (List.map viewAlbum albums)
    TrackList album tracks ->
      Html.div [Html.id "track-list"] (List.map viewTrack tracks)

viewAlbum : Album -> Html Msg
viewAlbum album =
  let
    (AlbumId id) = album.id
    firstTrack = (String.dropRight 3 id) ++ "101"
  in
    Html.a [ Html.href (routeAlbum album.id) ]
      [ Html.div
          [ Html.class "album" ]
          -- TODO: Serve album covers directly.
          [ Html.img [Html.src (apiHost ++ "/cover/" ++ firstTrack)] []
          , Html.h2 [] [Html.text album.title]
          , Html.p [] [Html.text album.artist]
          , Html.p [Html.class "date"] [Html.text (String.left 4 album.date)]
          ]
      ]

viewTrack : Track -> Html Msg
viewTrack track =
  Html.div [Html.class "track"]
    [ Html.span [Html.class "tracknumber"]
        [Html.text (toString track.trackNumber)]
    , viewTitle track.title
    , Html.p []
        [ Html.span [Html.class "duration"]
            [Html.text (formatDuration track.durationSeconds)]
        , Html.text track.artist
        ]
    ]

-- Format a track title inside a h3, but set parenthesized suffixes inside a
-- separate span, so they can be de-emphasized.
viewTitle : String -> Html Msg
viewTitle title =
  case List.maximum (String.indices "(" title) of
    Just n ->
      Html.h3 []
        [ Html.text (String.left n title)
        , Html.span [Html.class "parens"] [Html.text (String.dropLeft n title)]
        ]
    Nothing ->
      Html.h3 [] [Html.text title]

-- SUBSCRIPTIONS

subscriptions : Model -> Sub Msg
subscriptions model =
  Sub.none

-- HTTP

getAlbums : Cmd Msg
getAlbums =
  let
    url = apiHost ++ "/albums"
  in
    Http.send LoadAlbums (Http.get url (Json.list decodeAlbum))

getAlbum : AlbumId -> Cmd Msg
getAlbum albumId =
  let
    (AlbumId id) = albumId
    url = apiHost ++ "/album/" ++ id
    decodeAlbumInline =
      Json.map4 (Album albumId)
        (Json.field "title" Json.string)
        (Json.field "artist" Json.string)
        (Json.field "sort_artist" Json.string)
        (Json.field "date" Json.string)
    decodeTracks = Json.field "tracks" (Json.list decodeTrack)
    decode = Json.map2 (\x y -> (x, y)) decodeAlbumInline decodeTracks
  in
    Http.send LoadAlbum (Http.get url decode)

decodeAlbum : Json.Decoder Album
decodeAlbum =
  Json.map5 Album
    (Json.field "id" (Json.map AlbumId Json.string))
    (Json.field "title" Json.string)
    (Json.field "artist" Json.string)
    (Json.field "sort_artist" Json.string)
    (Json.field "date" Json.string)

decodeTrack : Json.Decoder Track
decodeTrack =
  Json.map6 Track
    (Json.field "id" (Json.map TrackId Json.string))
    (Json.field "disc_number" Json.int)
    (Json.field "track_number" Json.int)
    (Json.field "title" Json.string)
    (Json.field "artist" Json.string)
    (Json.field "duration_seconds" Json.int)
