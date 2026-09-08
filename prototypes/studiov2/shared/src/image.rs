// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A built firmware image, once it has left the screen that built it.
//!
//! The builder's own `Built` carries `onerom-gen`'s idea of a build. This is
//! what survives the trip across the boundary: bytes, a suggested filename and
//! a line of description. A programmer screen needs the bytes, an analyser
//! needs the bytes, and neither needs `onerom-gen` to say so.

use std::sync::Arc;

/// An image somebody built.
#[derive(Debug, Clone)]
pub struct Image {
    /// What to call it when saving.
    pub name: String,
    /// One line saying what it holds.
    pub description: String,
    /// The bytes.  Behind an `Arc` because a message carrying one must not
    /// copy a megabyte to move between screens.
    pub bytes: Arc<Vec<u8>>,
}

impl Image {
    /// How big the image is.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the image holds nothing.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}
