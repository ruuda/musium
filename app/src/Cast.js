// Mindec -- Music metadata indexer
// Copyright 2019 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.makeQueueItem = function(track) {
  // TODO: Better handling of Cast being unavailable.
  if (!window.hasCast) return null;

  var meta = new chrome.cast.media.MusicTrackMediaMetadata();
  meta.discNumber  = track.discNumber;
  meta.trackNumber = track.trackNumber;
  meta.title       = track.title;
  meta.artist      = track.artist;
  meta.albumName   = track.albumTitle;
  meta.albumArtist = track.albumArtist;
  meta.releaseDate = track.releaseDate;
  meta.images = [new chrome.cast.Image(track.imageUrl)];

  var mediaInfo = new chrome.cast.media.MediaInfo(track.trackUrl, 'audio/flac');
  mediaInfo.metadata = meta;

  var preloadSeconds = 10.0;
  var queueItem = new chrome.cast.media.QueueItem(mediaInfo);
  queueItem.preloadTime = preloadSeconds;

  return queueItem;
};

exports.getCastSessionImpl = function(onError, onSuccess) {
  if (!window.hasCast) onError("unavailable");

  var context = cast.framework.CastContext.getInstance();
  var castSession = context.getCurrentSession();

  if (castSession) {
    onSuccess(castSession);
  } else {
    context.requestSession().then(
      function() { onSuccess(context.getCurrentSession()); },
      onError
    );
  };

  return function(cancelError, cancelerError, cancelerSuccess) {
    console.error('TODO: How to mplement cancellation?');
  };
};

exports.getMediaSessionImpl = function(just, nothing, castSession) {
  var mediaSession = castSession.getMediaSession();
  if (mediaSession) {
    return just(mediaSession);
  } else {
    return nothing;
  }
}

exports.queueTrackImpl = function(unit, mediaSession, queueItem) {
  return function(onError, onSuccess) {
    mediaSession.queueAppendItem(
      queueItem,
      function() { onSuccess(unit) },
      onError
    );

    return function(cancelError, cancelerError, cancelerSuccess) {
      console.error('TODO: How to mplement cancellation?');
    };
  };
};

exports.playTrackImpl = function(unit, castSession, queueItem) {
  return function(onError, onSuccess) {
    var request = new chrome.cast.media.LoadRequest(queueItem.media);

    castSession.loadMedia(request).then(
      function() { onSuccess(unit); },
      onError
    );

    return function(cancelError, cancelerError, cancelerSuccess) {
      console.error('TODO: How to mplement cancellation?');
    };
  };
}
