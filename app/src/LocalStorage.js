// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.setImpl = function(key, value) {
  return function() {
    return window.localStorage.setItem(key, value);
  }
}

exports.getImpl = function(nothing, just, key) {
  return function() {
    let value = window.localStorage.getItem(key);
    if (value === undefined) {
      return nothing;
    } else {
      return just(value);
    }
  }
}
