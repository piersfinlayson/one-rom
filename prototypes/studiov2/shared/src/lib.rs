// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! State more than one screen reads, and the look they share.
//!
//! Every screen crate depends on this one and nothing depends on a screen but
//! the shell, so the dependency graph is a fan and the compiler rejects a
//! screen that reaches sideways into another.
//!
//! [`Shared`] is the single source of truth for anything two screens both
//! want.  A screen takes it by reference in `update` and `view` and keeps no
//! copy of it, which is what stops a second version of the truth appearing.
//! Screen-local state — a scroll offset, what is typed in a box, a widget's
//! own buffer — stays in the screen and is not here.

pub mod device;
pub mod image;
pub mod store;
pub mod style;

pub use device::Device;
pub use image::Image;
pub use store::{Store, StoreError};

/// Everything more than one screen reads.
///
/// Held once, by the shell.  A standalone screen binary holds one too, built
/// by [`Shared::stub`], so a screen never learns whether it is running inside
/// the shell or on its own.
pub struct Shared {
    /// The device the user is working with, if one is selected.
    ///
    /// Which devices are attached is the shell's business.  Which one is
    /// selected is everybody's.
    pub device: Option<Device>,

    /// The whole session log, in a file.
    ///
    /// Every screen appends to this one. A screen showing it holds its own
    /// window over it, which is a cache and not a second copy — see
    /// [`Store::revision`] for how a screen learns its cache is stale.
    pub log: Store,

    /// The last image built, if any.
    ///
    /// Produced by the builder, wanted by a programmer and by an analyser, so
    /// it cannot live in the builder.
    pub image: Option<Image>,
}

impl Shared {
    /// Wraps a fresh log store.
    pub fn new(log: Store) -> Self {
        Self {
            device: None,
            log,
            image: None,
        }
    }

    /// Shared state for a screen running on its own: a temporary log, one
    /// made-up device selected, and no image until something builds one.
    pub fn stub() -> Result<Self, StoreError> {
        let mut shared = Self::new(Store::temporary()?);
        shared.device = device::attached().into_iter().next();
        Ok(shared)
    }

    /// Throws the log away and starts a new one.
    ///
    /// The new store's revision starts at zero, so every screen holding a
    /// cached window sees a number it does not recognise and rebuilds.
    pub fn clear_log(&mut self) -> Result<(), StoreError> {
        self.log = Store::temporary()?;
        Ok(())
    }
}
