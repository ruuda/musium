import Html exposing (Html)
import Html.Events as Html
import Html.Attributes as Html
import Http
import Json.Decode as Json

apiHost : String
apiHost = "http://localhost:8233"

main =
  Html.program
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

-- MODEL

type Model
  = Loading
  | AlbumList (List Album)
  | TrackList Album (List Track)
  | Failed String

init : (Model, Cmd Msg)
init =
  ( Loading
  , getAlbums
  )

-- UPDATE

type Msg
  = LoadAlbums (Result Http.Error (List Album))
  | LoadAlbum (Result Http.Error (Album, List Track))
  | OpenAlbum Album

update : Msg -> Model -> (Model, Cmd Msg)
update msg model =
  case msg of
    OpenAlbum album ->
      (model, getAlbum album)
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
    Html.a [ Html.href "#" ]
      [ Html.div
          [ Html.class "album"
          , Html.onClick (OpenAlbum album)
          ]
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
    , Html.h3 [] [Html.text track.title]
    , Html.p [] [Html.text track.artist]
    ]

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

getAlbum : Album -> Cmd Msg
getAlbum album =
  let
    (AlbumId id) = album.id
    url = apiHost ++ "/album/" ++ id
    prependAlbum ts = (album, ts)
    decodeTracks = Json.field "tracks" (Json.list decodeTrack)
  in
    Http.send LoadAlbum (Http.get url (Json.map prependAlbum decodeTracks))

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
