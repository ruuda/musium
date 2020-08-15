// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Adapted from Sempervivum (github.com/ruuda/sempervivum), which is copyright
// 2020 Ruud van Asseldonk, licensed Apache 2.0.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.eqInstantImpl = function(lhs, rhs) {
  return lhs === rhs;
}

exports.ordInstantImpl = function(lt, eq, gt, lhs, rhs) {
  return lhs < rhs ? lt : lhs === rhs ? eq : gt;
}

exports.getCurrentInstant = function() {
  return new Date(Date.now());
}

exports.addSecondsImpl = function(secs, instant) {
  return new Date(secs * 1000.0 + instant.getTime());
}

exports.diffSecondsImpl = function(t0, t1) {
  return (t0.getTime() - t1.getTime()) / 1000.0;
}
