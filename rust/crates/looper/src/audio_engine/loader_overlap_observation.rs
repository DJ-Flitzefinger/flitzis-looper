//! Request-carried test observations at actual cold PCM producer callbacks.
//! No callback observer, sampler thread, gate or production API is installed.

use super::sample_loader::{SampleLoadProgress, SampleLoadSubtask};
use serde::Serialize;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

impl FileTime {
    fn ticks(self) -> u64 {
        (u64::from(self.high) << 32) | u64::from(self.low)
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThread() -> *mut std::ffi::c_void;
    fn GetCurrentThreadId() -> u32;
    fn GetThreadTimes(
        thread: *mut std::ffi::c_void,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Worker {
    pub(super) id: u32,
    pub(super) birth_100ns: u64,
    pub(super) cpu_ns: u64,
}

pub(super) fn current_worker() -> Worker {
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    // SAFETY: initialized FILETIME outputs and the current-thread pseudo handle
    // are valid for this synchronous non-realtime test observation. No owned
    // OS handle is created or retained.
    let (ok, id) = unsafe {
        (
            GetThreadTimes(
                GetCurrentThread(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            ),
            GetCurrentThreadId(),
        )
    };
    assert_ne!(
        ok,
        0,
        "thread CPU query: {}",
        std::io::Error::last_os_error()
    );
    Worker {
        id,
        birth_100ns: creation.ticks(),
        cpu_ns: (kernel.ticks() + user.ticks()) * 100,
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Point {
    pub(super) before_ns: u128,
    pub(super) after_ns: u128,
    pub(super) worker: Worker,
    pub(super) pad: usize,
    pub(super) request: u64,
    pub(super) stage: &'static str,
    pub(super) percent: f32,
}

pub(super) struct Observation {
    pub(super) epoch: Instant,
    points: Mutex<Vec<Point>>,
}

impl Observation {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            epoch: Instant::now(),
            points: Mutex::new(Vec::with_capacity(8192)),
        })
    }

    pub(super) fn points(&self) -> Vec<Point> {
        self.points.lock().unwrap().clone()
    }
}

thread_local! {
    static OBSERVER: RefCell<Option<Arc<Observation>>> = const { RefCell::new(None) };
}

/// Transfer the observer with exactly the admitted job, never by a global PAD key.
pub(super) fn take() -> Option<Arc<Observation>> {
    OBSERVER.with(|slot| slot.borrow_mut().take())
}

pub(super) struct Scope(Option<Arc<Observation>>);

impl Scope {
    pub(super) fn install(observer: Option<Arc<Observation>>) -> Self {
        Self(OBSERVER.with(|slot| slot.replace(observer)))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        OBSERVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

pub(super) fn observe(pad: usize, request: u64, progress: SampleLoadProgress) {
    OBSERVER.with(|slot| {
        let slot = slot.borrow();
        let Some(observer) = slot.as_ref() else {
            return;
        };
        // Both CPU queries of an interval are conservatively enclosed by its
        // outer wall stamps. Counter granularity and ordinary reporting/observer
        // overhead remain part of the whole Loader-worker observation.
        let before_ns = observer.epoch.elapsed().as_nanos();
        let worker = current_worker();
        let after_ns = observer.epoch.elapsed().as_nanos();
        let stage = match progress.subtask {
            SampleLoadSubtask::Decoding => "decoding",
            SampleLoadSubtask::Resampling => "resampling",
            SampleLoadSubtask::ChannelMapping => "channel-mapping",
        };
        let mut points = observer.points.lock().unwrap();
        assert!(points.len() < 8192, "bounded Loader observation exhausted");
        points.push(Point {
            before_ns,
            after_ns,
            worker,
            pad,
            request,
            stage,
            percent: progress.percent,
        });
    });
}
