// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The pointer and cell types the generated device-side types are built
//! from, shared by every crate in the family.

use core::cell::UnsafeCell;
use core::ffi::{CStr, c_char};
use core::ptr::NonNull;

/// A pointer field of a device-side type. It is never null. A field that may
/// be null is `Option<Ptr<T>>`, which has the same layout.
#[repr(transparent)]
pub struct Ptr<T>(NonNull<T>);

// A Ptr stands for a `&'static T`, so it may be shared across threads when `T`
// may. It hands out only a raw pointer, so reading through it is `unsafe` anyway.
unsafe impl<T: Sync> Sync for Ptr<T> {}

impl<T> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Ptr<T> {}

impl<T> Ptr<T> {
    /// A pointer to `target`.
    pub const fn new(target: &'static T) -> Self {
        Self(NonNull::from_ref(target))
    }

    /// A pointer to the first element of `slice`, for a field that points at
    /// an array. The pointer may reach every element, where one made with
    /// [`Ptr::new`] from the first element may reach only that one.
    pub const fn from_slice(slice: &'static [T]) -> Self {
        Self(NonNull::from_ref(slice).cast())
    }

    /// The same address, pointing at a `U`. For a field typed `c_void`, and for
    /// a variable-length structure built as one value holding its parameters.
    pub const fn cast<U>(self) -> Ptr<U> {
        Ptr(self.0.cast())
    }

    /// The address held.
    pub const fn as_ptr(self) -> *const T {
        self.0.as_ptr()
    }
}

impl Ptr<c_char> {
    /// A pointer to the first byte of `s`, for a string field.
    pub const fn from_cstr(s: &'static CStr) -> Self {
        Self(NonNull::from_ref(s).cast())
    }
}

const _: () = assert!(size_of::<Option<Ptr<u8>>>() == size_of::<*const u8>());

/// A structure the firmware writes while it runs, such as the runtime info.
///
/// A pointer to one is `Ptr<RuntimeCell<T>>`, so it cannot point at a plain
/// static in flash. The `UnsafeCell` is what places the static in RAM.
#[repr(transparent)]
pub struct RuntimeCell<T>(UnsafeCell<T>);

// Every access goes through the raw pointer `as_mut_ptr` returns, so keeping
// them in order is the firmware's job, as with a `static mut`.
unsafe impl<T: Sync> Sync for RuntimeCell<T> {}

impl<T> RuntimeCell<T> {
    /// A cell holding `value`.
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// The address of the value, for the firmware to write through.
    pub const fn as_mut_ptr(&self) -> *mut T {
        self.0.get()
    }
}
