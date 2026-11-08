// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - static buffers lent to one user at a time
//!
//! A buffer too large for the heap or a stack is a static.  A [`Lender`] lends
//! it as a [`Loan`], which returns it when dropped, so only one user at a time
//! holds it, on either core.

use core::cell::Cell;
use core::ops::{Deref, DerefMut};

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use static_cell::ConstStaticCell;

pub struct Lender<T: 'static> {
    value: ConstStaticCell<T>,
    returned: Mutex<CriticalSectionRawMutex, Cell<Option<&'static mut T>>>,
}

impl<T: Send> Lender<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: ConstStaticCell::new(value),
            returned: Mutex::new(Cell::new(None)),
        }
    }

    /// Lends the value, or returns `None` while it is lent.
    pub fn lend(&'static self) -> Option<Loan<T>> {
        let value = self
            .returned
            .lock(Cell::take)
            .or_else(|| self.value.try_take())?;
        Some(Loan {
            lender: self,
            value: Some(value),
        })
    }
}

pub struct Loan<T: 'static> {
    lender: &'static Lender<T>,
    /// `None` only while dropping.
    value: Option<&'static mut T>,
}

impl<T> Deref for Loan<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_deref().expect("a loan holds its value")
    }
}

impl<T> DerefMut for Loan<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_deref_mut().expect("a loan holds its value")
    }
}

impl<T> Drop for Loan<T> {
    fn drop(&mut self) {
        let value = self.value.take();
        self.lender.returned.lock(|returned| returned.set(value));
    }
}
