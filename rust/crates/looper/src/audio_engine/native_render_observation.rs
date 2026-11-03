//! Scoped test-only counters on the rendering thread; workers are not included.

use std::cell::Cell;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct Observation {
    pub shifts: u64,
    pub shift_ns: u128,
    pub max_shift_ns: u128,
    pub adapter_shortfall_channel_frames: usize,
    pub adapter_resets: u64,
    pub source_refusals: u64,
}

thread_local! {
    static CURRENT: Cell<Option<Observation>> = const { Cell::new(None) };
}

pub(crate) struct Scope;

impl Scope {
    pub(crate) fn begin() -> Self {
        CURRENT.with(|current| assert!(current.replace(Some(Observation::default())).is_none()));
        Self
    }

    pub(crate) fn finish(self) -> Observation {
        CURRENT.with(|current| current.take().expect("active render observation"))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(None));
    }
}

pub(crate) fn shift_begin() -> Option<Instant> {
    CURRENT.with(|current| current.get().map(|_| Instant::now()))
}

pub(crate) fn shift_end(begin: Option<Instant>) {
    if let Some(begin) = begin {
        let elapsed = begin.elapsed().as_nanos();
        update(|row| {
            row.shifts += 1;
            row.shift_ns += elapsed;
            row.max_shift_ns = row.max_shift_ns.max(elapsed);
        });
    }
}

pub(crate) fn shortfall(frames: usize) {
    update(|row| row.adapter_shortfall_channel_frames += frames);
}

pub(crate) fn reset() {
    update(|row| row.adapter_resets += 1);
}

pub(crate) fn source_result(success: bool) {
    if !success {
        update(|row| row.source_refusals += 1);
    }
}

fn update(change: impl FnOnce(&mut Observation)) {
    CURRENT.with(|current| {
        if let Some(mut row) = current.get() {
            change(&mut row);
            current.set(Some(row));
        }
    });
}
