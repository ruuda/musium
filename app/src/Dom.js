// Mindec -- Music metadata indexer
// Copyright 2019 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.createElement = function(tagName) {
  return function() {
    return document.createElement(tagName);
  }
}

exports.appendChildImpl = function(child, container) {
  return function() {
    container.appendChild(child);
  }
}

exports.appendTextImpl = function(text, container) {
  return function() {
    container.insertAdjacentText('beforeend', text);
  }
}

exports.getElementByIdImpl = function(id, just, nothing) {
  return function() {
    var element = document.getElementById(id);
    if (element === null) {
      return nothing;
    } else {
      return just(element);
    }
  }
}

exports.assumeElementById = function(id) {
  return function() {
    var element = document.getElementById(id);
    window.console.assert(element !== null, 'No element with id: ' + id);
    return element;
  }
}

exports.body = document.body;

exports.setClassNameImpl = function(className, element) {
  return function() {
    element.className = className;
  }
}

exports.setIdImpl = function(id, element) {
  return function() {
    element.id = id;
  }
}
