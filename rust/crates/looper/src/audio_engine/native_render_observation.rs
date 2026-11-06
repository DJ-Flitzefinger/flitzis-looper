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
    pub first_shift_dependency: Option<ShiftDependency>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct SourceObservation {
    pub coverage: SourceTiming,
    pub checked_fill: SourceTiming,
    pub unchecked_fill: SourceTiming,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct SourceTiming {
    pub calls: usize,
    pub filled_frames: usize,
    pub ns: u128,
    pub max_ns: u128,
}

thread_local! {
    static CURRENT: Cell<Option<Observation>> = const { Cell::new(None) };
    static SOURCE: Cell<Option<SourceObservation>> = const { Cell::new(None) };
    static PENDING_DEPENDENCY: Cell<Option<PendingDependency>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct ShiftDependency {
    pub ready_prefix: usize,
    pub final_input_bits: [u32; 2],
    pub post_shift_to_mix_ns: u128,
}

#[derive(Clone, Copy)]
pub(super) struct PendingDependency {
    pub ready_prefix: usize,
    final_input_bits: [u32; 2],
    shifted_at: Instant,
}

impl PendingDependency {
    pub(super) fn mixed(self) {
        update(|row| {
            row.first_shift_dependency.get_or_insert(ShiftDependency {
                ready_prefix: self.ready_prefix,
                final_input_bits: self.final_input_bits,
                post_shift_to_mix_ns: self.shifted_at.elapsed().as_nanos(),
            });
        });
    }
}

pub(super) fn shifted_before_mix(ready_prefix: usize, final_input_bits: [u32; 2]) {
    if CURRENT.with(|current| current.get().is_some()) {
        PENDING_DEPENDENCY.with(|pending| {
            pending.set(Some(PendingDependency {
                ready_prefix,
                final_input_bits,
                shifted_at: Instant::now(),
            }));
        });
    }
}

pub(super) fn take_pending_dependency() -> Option<PendingDependency> {
    PENDING_DEPENDENCY.with(Cell::take)
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

    #[cfg(windows)]
    pub(crate) fn begin_source() -> Self {
        let scope = Self::begin();
        SOURCE.set(Some(SourceObservation::default()));
        scope
    }

    #[cfg(windows)]
    pub(crate) fn finish_source(self) -> (Observation, SourceObservation) {
        let source = SOURCE.take().expect("active source timing");
        (self.finish(), source)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(None));
        SOURCE.set(None);
        PENDING_DEPENDENCY.with(|pending| pending.set(None));
    }
}

pub(crate) struct SourceTimer {
    begin: Option<Instant>,
    bucket: fn(&mut SourceObservation) -> &mut SourceTiming,
    filled_frames: usize,
}

impl SourceTimer {
    pub(crate) fn begin(bucket: fn(&mut SourceObservation) -> &mut SourceTiming) -> Self {
        Self {
            begin: SOURCE.with(|source| source.get().map(|_| Instant::now())),
            bucket,
            filled_frames: 0,
        }
    }

    pub(crate) fn filled(mut self, frames: usize) {
        self.filled_frames = frames;
    }
}

impl Drop for SourceTimer {
    fn drop(&mut self) {
        if let Some(begin) = self.begin {
            let ns = begin.elapsed().as_nanos();
            if let Some(mut observation) = SOURCE.get() {
                let timing = (self.bucket)(&mut observation);
                timing.calls += 1;
                timing.filled_frames += self.filled_frames;
                timing.ns += ns;
                timing.max_ns = timing.max_ns.max(ns);
                SOURCE.set(Some(observation));
            }
        }
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
