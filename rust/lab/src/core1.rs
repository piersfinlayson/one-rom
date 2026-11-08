// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - bus work on core 1
//!
//! Runs Lab's long bus work on core 1.  A whole-chip read lasts several
//! seconds and on core 0 it would stop Lab answering USB until it finished.
//! Core 1 runs nothing else so a read isn't interrupted.

use core::mem::{forget, transmute};

use embassy_executor::Executor;
use embassy_rp::Peri;
use embassy_rp::multicore::{Stack, spawn_core1};
use embassy_rp::peripherals::CORE1;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use static_cell::{ConstStaticCell, StaticCell};

static STACK: ConstStaticCell<Stack<16384>> = ConstStaticCell::new(Stack::new());
static EXECUTOR: StaticCell<Executor> = StaticCell::new();

struct Job(&'static mut (dyn FnMut() + Send));

static JOB: Signal<CriticalSectionRawMutex, Job> = Signal::new();
static DONE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// One job at a time.
static LOCK: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

pub fn start(core1: Peri<'static, CORE1>) {
    spawn_core1(core1, STACK.take(), || {
        EXECUTOR
            .init(Executor::new())
            .run(|spawner| spawner.spawn(worker().unwrap()))
    });
}

#[embassy_executor::task]
async fn worker() {
    loop {
        let Job(job) = JOB.wait().await;
        job();
        DONE.signal(());
    }
}

/// Runs `f` on core 1 and returns its result.
pub async fn run<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    let _lock = LOCK.lock().await;
    let mut f = Some(f);
    let mut result = None;
    let mut job = || result = f.take().map(|f| f());
    let job: &mut (dyn FnMut() + Send) = &mut job;
    // SAFETY: core 1 is done with `job` once DONE is signalled, and this
    // waits for that, in `Running`'s drop if this future is dropped first.
    let job = unsafe { transmute::<&mut (dyn FnMut() + Send), &'static mut _>(job) };
    let running = Running;
    JOB.signal(Job(job));
    DONE.wait().await;
    forget(running);
    result.expect("core 1 ran the job")
}

struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        while DONE.try_take().is_none() {}
    }
}
