//! One preallocated SPSC handoff captures the actual effective voice off the callback.
use super::input_runtime_binding::InputPadBinding;
use super::prepared_source::PreparedSourcePermit;
use super::source_reader::FrameRange;
use super::voice_slot::VoiceSourceTiming;
use crate::messages::{PreparedStemSet, SampleBuffer};
use rtrb::{Producer, PushError};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub(crate) struct ResidentSeekPin {
    pub(crate) sample: SampleBuffer,
    pub(crate) loop_region: FrameRange,
    pub(crate) timing: VoiceSourceTiming,
    pub(crate) voice_index: usize,
    pub(crate) generation: u64,
    pub(crate) key_lock: bool,
    pub(crate) stems: Option<PreparedStemSet>,
}

pub(crate) struct ResidentSeekCapture {
    pub(crate) id: usize,
    pub(crate) binding: InputPadBinding,
    pub(crate) publication: PreparedSourcePermit,
    pub(crate) position_s: f64,
    sender: UnsafeCell<Producer<ResidentSeekPin>>,
    used: AtomicBool,
}

// Producer is used by exactly the winner of used.compare_exchange. A cloned
// command cannot become a second writer; the sole Consumer lives on the already
// reserved cold worker. Both owners are retired outside the audio callback.
unsafe impl Sync for ResidentSeekCapture {}

impl std::fmt::Debug for ResidentSeekCapture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResidentSeekCapture")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ResidentSeekCapture {
    pub(crate) fn new(
        id: usize,
        binding: InputPadBinding,
        publication: PreparedSourcePermit,
        position_s: f64,
        sender: Producer<ResidentSeekPin>,
    ) -> Self {
        Self {
            id,
            binding,
            publication,
            position_s,
            sender: UnsafeCell::new(sender),
            used: AtomicBool::new(false),
        }
    }

    /// None means the preallocated handoff accepted ownership. A returned pin
    /// remains with the callback for bounded off-thread retirement.
    pub(crate) fn publish(&self, pin: ResidentSeekPin) -> Option<ResidentSeekPin> {
        if self
            .used
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Some(pin);
        }
        // The atomic winner is the unique producer writer for the entire lifetime.
        match unsafe { &mut *self.sender.get() }.push(pin) {
            Ok(()) => None,
            Err(PushError::Full(pin)) => Some(pin),
        }
    }
}
