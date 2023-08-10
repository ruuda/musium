// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

export const pushStateImpl = function(data, title, url) {
  return function() {
    document.title = title;
    history.pushState(data, title, url);
  };
}

export const onPopStateImpl = function(nothing, just, callback) {
  return function() {
    // We manage scroll positions manually on naviagation,
    // the browser should not mess with it.
    history.scrollRestoration = 'manual';

    window.onpopstate = function(event) {
      if (event.state === null) {
        callback(nothing)();
      } else if (Object.keys(event.state).length === 0) {
        callback(nothing)();
      } else {
        callback(just(event.state))();
      }
    };
  };
}
