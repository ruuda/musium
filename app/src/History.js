// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.pushStateImpl = function(data, title, url) {
  return function() {
    history.pushState(data, title, url);
  };
}

exports.onPopState = function(callback) {
  return function() {
    window.onpopstate = function(event) {
      callback(event.state)();
    };
  };
}
