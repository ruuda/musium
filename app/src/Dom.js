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

exports.removeChildImpl = function(child, container) {
  return function() {
    container.removeChild(child);
  }
}

exports.clearElement = function(container) {
  return function() {
    while (container.hasChildNodes()) {
      container.removeChild(container.lastChild);
    }
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

exports.getValue = function(element) {
  return function() {
    return element.value;
  }
}

exports.setAttributeImpl = function(attribute, value, element) {
  return function() {
    element.setAttribute(attribute, value);
  }
}

exports.addClassImpl = function(className, element) {
  return function() {
    element.classList.add(className);
  }
}

exports.removeClassImpl = function(className, element) {
  return function() {
    element.classList.remove(className);
  }
}

exports.setIdImpl = function(id, element) {
  return function() {
    element.id = id;
  }
}

exports.addEventListenerImpl = function(eventName, callback, element) {
  return function() {
    element.addEventListener(eventName, function(evt) {
      callback();
      return false;
    });
  }
}
