// Musium -- Music playback daemon with web-based library browser
// Copyright 2019 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

exports.eqElementImpl = function(x) {
  return function(y) {
    return x === y;
  }
}

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

exports.renderHtml = function(html) {
  return function() {
    const range = document.createRange();
    range.selectNode(document.body);
    return range.createContextualFragment(html);
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

exports.getBoundingClientRect = function(element) {
  return function() {
    return element.getBoundingClientRect();
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

exports.setValueImpl = function(value, element) {
  return function() {
    element.value = value;
  }
}

exports.getOffsetHeight = function(element) {
  return function() {
    return element.offsetHeight;
  }
}

exports.getWindowHeight = function() {
  return window.innerHeight;
}

exports.getScrollTop = function(element) {
  return function() {
    return element.scrollTop;
  }
}

exports.setScrollTopImpl = function(off, element) {
  return function() {
    element.scrollTop = off;
  }
}

exports.scrollIntoView = function(element) {
  return function() {
    element.scrollIntoView({
      // Page transitions are immediate, so scrolling should jump, it should not
      // be smooth.
      behavior: 'auto',
      block: 'start',
      inline: 'nearest',
    });
  }
}

exports.focusElement = function(element) {
  return function() {
    return element.focus();
  }
}

exports.setAttributeImpl = function(attribute, value, element) {
  return function() {
    element.setAttribute(attribute, value);
  }
}

exports.setImageImpl = function(src, alt, element) {
  return function() {
    element.src = src;
    element.alt = alt;
  }
}

exports.getComplete = function(element) {
  return function() {
    return element.complete;
  }
}

exports.waitCompleteImpl = function(unit, imgElement) {
  return function (onError, onSuccess) {
    let decodePromise = imgElement.decode();

    decodePromise.then(function() {
      onSuccess(unit);
    }, function() {
      onError(unit);
    });

    return function (cancelError, onCancelError, onCancelSuccess) {
      onCancelSuccess();
    };
  };
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

exports.setTransformImpl = function(transform, element) {
  return function() {
    element.style.transform = transform;
  }
}

exports.setTransitionImpl = function(transition, element) {
  return function() {
    element.style.transition = transition;
  }
}

exports.setWidthImpl = function(width, element) {
  return function() {
    element.style.width = width;
  }
}

exports.setHeightImpl = function(height, element) {
  return function() {
    element.style.height = height;
  }
}

exports.addEventListenerImpl = function(eventName, callback, element) {
  return function() {
    element.addEventListener(eventName, function(evt) {
      callback();
      evt.stopPropagation();
      return false;
    });
  }
}

exports.onWindowKeyDown = function(callback) {
  return function() {
    window.addEventListener('keydown', function (evt) {
      callback(evt.key)();
    });
  }
}

exports.onScrollImpl = function(callback, element) {
  return function() {
    // For some reason, addEventListener('scroll') does not work,
    // but this does.
    element.onscroll = function(evt) {
      callback();
    };
  }
}

exports.onResizeWindow = function(callback) {
  return function() {
    window.onresize = function(evt) {
      callback();
    };
  }
}
