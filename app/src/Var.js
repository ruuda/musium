// Mindec -- Music metadata indexer
// Copyright 2019 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.create = function(initialValue) {
  return function() {
    return { value: initialValue };
  }
}

exports.get = function(variable) {
  return function() {
    return variable.value;
  }
}

exports.set = function(variable) {
  return function(value) {
    return function() {
      variable.value = value;
    }
  }
}
