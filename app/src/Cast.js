// Mindec -- Music metadata indexer
// Copyright 2019 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.playTrack = function(track) {
  // Pure part: set up the track metadata and load request.
  var meta = new chrome.cast.media.MusicTrackMediaMetadata();
  meta.discNumber = track.discNumber;
  meta.trackNumber = track.trackNumber;
  meta.title = track.title;
  meta.artist = track.artist;
  meta.albumName = track.albumTitle;
  meta.albumArtist = track.albumArtist;
  meta.releaseDate = track.releaseDate;
  meta.images = [new chrome.cast.Image(track.imageUrl)];

  var mediaInfo = new chrome.cast.media.MediaInfo(track.trackUrl, 'audio/flac');
  mediaInfo.metadata = meta;

  var request = new chrome.cast.media.LoadRequest(mediaInfo);

  function doPlay(castSession) {
    castSession.loadMedia(request).then(
      function() { console.log('Load succeed'); },
      function(errorCode) { console.log('Error code: ' + errorCode); }
    );
  };

  // Effectful part: actually send the load request.
  return function() {
    var context = cast.framework.CastContext.getInstance();
    var castSession = context.getCurrentSession();
    if (castSession) {
      doPlay(castSession);
    } else {
      context.requestSession().then(
        function() { doPlay(context.getCurrentSession()); },
        function(errorCode) { console.log('Error code: ' + errorCode); }
      );
    }
  };
};
