// Musium -- Music playback daemon with web-based library browser
// Copyright 2019 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

"use strict";

export const eqElementImpl = function(x) {
  return function(y) {
    return x === y;
  }
}

export const createElement = function(tagName) {
  return function() {
    return document.createElement(tagName);
  }
}

export const appendChildImpl = function(child, container) {
  return function() {
    container.appendChild(child);
  }
}

export const renderHtml = function(html) {
  return function() {
    const range = document.createRange();
    range.selectNode(document.body);
    return range.createContextualFragment(html);
  }
}

export const removeChildImpl = function(child, container) {
  return function() {
    container.removeChild(child);
  }
}

export const clearElement = function(container) {
  return function() {
    while (container.hasChildNodes()) {
      container.removeChild(container.lastChild);
    }
  }
}

export const appendTextImpl = function(text, container) {
  return function() {
    container.insertAdjacentText('beforeend', text);
  }
}

export const getElementByIdImpl = function(id, just, nothing) {
  return function() {
    var element = document.getElementById(id);
    if (element === null) {
      return nothing;
    } else {
      return just(element);
    }
  }
}

export const getBoundingClientRect = function(element) {
  return function() {
    return element.getBoundingClientRect();
  }
}

export const assumeElementById = function(id) {
  return function() {
    var element = document.getElementById(id);
    window.console.assert(element !== null, 'No element with id: ' + id);
    return element;
  }
}

export const body = document.body;

export const getValue = function(element) {
  return function() {
    return element.value;
  }
}

export const setValueImpl = function(value, element) {
  return function() {
    element.value = value;
  }
}

export const getOffsetHeight = function(element) {
  return function() {
    return element.offsetHeight;
  }
}

export const getWindowHeight = function() {
  return window.innerHeight;
}

export const getScrollTop = function(element) {
  return function() {
    return element.scrollTop;
  }
}

export const setScrollTopImpl = function(off, element) {
  return function() {
    element.scrollTop = off;
  }
}

export const scrollIntoView = function(element) {
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

export const focusElement = function(element) {
  return function() {
    return element.focus();
  }
}

export const setAttributeImpl = function(attribute, value, element) {
  return function() {
    element.setAttribute(attribute, value);
  }
}

export const setImageImpl = function(src, alt, element) {
  return function() {
    element.src = src;
    element.alt = alt;
  }
}

export const getComplete = function(element) {
  return function() {
    return element.complete;
  }
}

export const waitCompleteImpl = function(unit, imgElement) {
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

export const addClassImpl = function(className, element) {
  return function() {
    element.classList.add(className);
  }
}

export const removeClassImpl = function(className, element) {
  return function() {
    element.classList.remove(className);
  }
}

export const setIdImpl = function(id, element) {
  return function() {
    element.id = id;
  }
}

export const setTransformImpl = function(transform, element) {
  return function() {
    element.style.transform = transform;
  }
}

export const setTransitionImpl = function(transition, element) {
  return function() {
    element.style.transition = transition;
  }
}

export const setMaskImageImpl = function(mask, element) {
  return function() {
    if ('webkitMaskImage' in element.style) {
      element.style.webkitMaskImage = mask;
    } else {
      element.style.maskImage = mask;
    }
  }
}

export const setBackgroundColorImpl = function(hexColor, element) {
  return function() {
    element.style.backgroundColor = hexColor;
  }
}

export const setWidthImpl = function(width, element) {
  return function() {
    element.style.width = width;
  }
}

export const setHeightImpl = function(height, element) {
  return function() {
    element.style.height = height;
  }
}

export const addEventListenerImpl = function(eventName, callback, element) {
  return function() {
    element.addEventListener(eventName, function(evt) {
      callback();
      evt.stopPropagation();
      return false;
    });
  }
}

export const onWindowKeyDown = function(callback) {
  return function() {
    window.addEventListener('keydown', function (evt) {
      callback(evt.key)();
    });
  }
}

export const onScrollImpl = function(callback, element) {
  return function() {
    // For some reason, addEventListener('scroll') does not work,
    // but this does.
    element.onscroll = function(evt) {
      callback();
    };
  }
}

export const onResizeWindow = function(callback) {
  return function() {
    window.onresize = function(evt) {
      callback();
    };
  }
}
