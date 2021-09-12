// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Holds a convenient synchronization primitive.

use std::sync::Mutex;

/// Pointer to a shared value that can be updated atomically.
///
/// Reads of the value return a clone. This can be used in two ways:
///
///  * If the value is `Copy`, then we can just copy the value on every read.
///  * Otherwise, wrap the inner value in an `Arc`, so reads clone the
///    reference, but not the inner value itself.
///
/// Inspired by Haskell's `TVar` and `MVar`.
///
/// For this primitive to be useful, you probably want to put it in an `Arc` and
/// share it between threads.
pub struct MVar<T: Clone> {
    inner: Mutex<T>,
}

impl<T: Clone> MVar<T> {
    /// Create a new `MVar` with the given initial value.
    pub fn new(initial_value: T) -> MVar<T> {
        MVar {
            inner: Mutex::new(initial_value),
        }
    }

    /// Return a clone of the current value.
    pub fn get(&self) -> T {
        self.inner.lock().unwrap().clone()
    }

    /// Replace the current value with a new value, return the old value.
    pub fn set(&self, new_value: T) -> T {
        std::mem::replace(&mut *self.inner.lock().unwrap(), new_value)
    }
}
