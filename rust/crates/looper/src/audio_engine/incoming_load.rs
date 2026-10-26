//! Request-owned ordinary incoming demand. Python supplies resolved intent;
//! this worker/control state never becomes a second Global/PAD policy authority.

use super::cold_residency::ResidentLoadHint;
use super::source_grid::SourceGrid;
use crate::messages::{IncomingLoadDefaults, LoadedAnalysis, SampleBuffer};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

// Only off-realtime intent mutation owns this short state. Callback claim and
// mutation therefore have one atomic order without a callback lock.
pub(super) const INTENT_UPDATING: u8 = 4;

fn valid_source_period(period: f64, rate: u32) -> bool {
    let frames = period * f64::from(rate);
    let bpm = 60.0 / period;
    period.is_finite()
        && period > 0.0
        && frames.is_finite()
        && frames > 0.0
        && bpm.is_finite()
        && bpm > 0.0
}

#[cfg(test)]
thread_local! {
    static CLAIM_OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {
        std::cell::RefCell::new(None)
    };
    static UPDATE_RELEASE_OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub(super) fn set_claim_observer(observer: impl FnOnce() + 'static) {
    CLAIM_OBSERVER.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(observer));
    });
}

#[cfg(test)]
pub(super) fn observe_callback_claim() {
    let observer = CLAIM_OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer();
    }
}

#[cfg(test)]
pub(super) fn set_update_release_observer(observer: impl FnOnce() + 'static) {
    UPDATE_RELEASE_OBSERVER.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(observer));
    });
}

#[cfg(test)]
fn observe_update_release() {
    let observer = UPDATE_RELEASE_OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer();
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct IncomingIntent {
    pub revision: u64,
    pub key_lock: bool,
    pub loop_start_s: Option<f64>,
    pub loop_end_s: Option<f64>,
    pub loop_auto: bool,
    pub loop_bars: f64,
    pub timing_override: bool,
    pub period_seconds: Option<f64>,
    pub grid_origin_s: Option<f64>,
}

impl IncomingIntent {
    pub(super) fn initial(key_lock: bool) -> Self {
        Self {
            revision: 0,
            key_lock,
            loop_start_s: None,
            loop_end_s: None,
            loop_auto: true,
            loop_bars: 8.0,
            timing_override: false,
            period_seconds: None,
            grid_origin_s: None,
        }
    }

    pub(super) fn validate(self) -> Result<(), String> {
        if self
            .loop_start_s
            .is_some_and(|value| !value.is_finite() || value < 0.0)
            || self
                .loop_end_s
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || !self.loop_bars.is_finite()
            || self.loop_bars <= 0.0
            || self
                .period_seconds
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || self.grid_origin_s.is_some_and(|value| !value.is_finite())
            || (!self.loop_auto
                && self
                    .loop_end_s
                    .is_some_and(|end| end <= self.loop_start_s.unwrap_or(0.0)))
        {
            return Err("incoming loop/timing intent is invalid".into());
        }
        Ok(())
    }

    pub(super) fn validate_for_rate(self, rate: u32) -> Result<(), String> {
        self.validate()?;
        if rate == 0
            || self
                .period_seconds
                .is_some_and(|period| !valid_source_period(period, rate))
            || self
                .grid_origin_s
                .is_some_and(|origin| !(origin * f64::from(rate)).is_finite())
        {
            return Err("incoming source timing frames out of range".into());
        }
        Ok(())
    }

    pub(super) fn defaults(
        self,
        rate: u32,
        frames: usize,
        activity: Option<f64>,
        analysis: Option<&LoadedAnalysis>,
    ) -> Result<IncomingLoadDefaults, String> {
        self.validate_for_rate(rate)?;
        if rate == 0 || frames == 0 {
            return Err("incoming source geometry is unavailable".into());
        }
        let rate_f = f64::from(rate);
        let marker = |seconds: f64| {
            (seconds * rate_f)
                .round_ties_even()
                .clamp(0.0, frames as f64) as usize
        };
        let activity_start = activity
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or(0.0);
        let start_s =
            self.loop_start_s
                .unwrap_or(if self.loop_auto { activity_start } else { 0.0 });
        let requested_start = marker(start_s);
        let period_seconds = if self.timing_override {
            self.period_seconds
        } else {
            analysis.and_then(|result| {
                let bpm = result.bpm;
                (bpm.is_finite() && bpm > 0.0).then(|| 60.0 / bpm)
            })
        };
        if period_seconds.is_some_and(|period| !valid_source_period(period, rate)) {
            return Err("incoming source period frames out of range".into());
        }
        let origin_s = self.grid_origin_s.unwrap_or(activity_start);
        let end_s = if self.loop_auto {
            period_seconds.and_then(|period| {
                SourceGrid::from_period(period, origin_s)
                    .and_then(|grid| grid.source_after_beats(start_s, self.loop_bars * 4.0))
            })
        } else {
            self.loop_end_s
        };
        let requested_end = end_s.map(marker);
        let region =
            super::source_reader::effective_loop_region(requested_start, requested_end, frames)
                .ok_or("incoming source geometry is unavailable")?;
        let start_frame = region.start;
        let end_frame = requested_end.map(|_| region.end);
        let origin_frame = (origin_s * rate_f).round_ties_even();
        if !origin_frame.is_finite() {
            return Err("incoming grid origin overflows".into());
        }
        Ok(IncomingLoadDefaults {
            sample_rate_hz: rate,
            revision: self.revision,
            key_lock: self.key_lock,
            start_frame,
            end_frame,
            period_seconds,
            origin_frame,
            loop_auto: self.loop_auto,
            loop_bars: self.loop_bars,
        })
    }
}

impl IncomingLoadDefaults {
    pub(crate) fn valid_for_sample(self, sample: &SampleBuffer, rate: u32) -> bool {
        if self.sample_rate_hz != rate
            || !self.origin_frame.is_finite()
            || !self.loop_bars.is_finite()
            || self.loop_bars <= 0.0
            || self
                .period_seconds
                .is_some_and(|period| !valid_source_period(period, rate))
        {
            return false;
        }
        let Some(region) = super::source_reader::effective_loop_region(
            self.start_frame,
            self.end_frame,
            sample.frame_count(),
        ) else {
            return false;
        };
        self.start_frame == region.start
            && self.end_frame.is_none_or(|end| end == region.end)
            && ((sample.resident_start() == 0 && sample.resident_end() == sample.frame_count())
                || (self.end_frame.is_some()
                    && sample.resident_start() == region.start
                    && sample.resident_end() == region.end))
    }

    pub(super) fn resident_hint(self, rate: u32, frames: usize) -> Option<ResidentLoadHint> {
        // Unknown musical end remains physically complete; ON still records its
        // genuine explicit complete-track context rather than inventing BPM.
        self.end_frame
            .map(|end| ResidentLoadHint {
                start_s: self.start_frame as f64 / f64::from(rate),
                end_s: end as f64 / f64::from(rate),
                key_lock: self.key_lock,
            })
            .or_else(|| {
                self.key_lock.then_some(ResidentLoadHint {
                    start_s: 0.0,
                    end_s: frames as f64 / f64::from(rate),
                    key_lock: true,
                })
            })
    }
}

pub(super) struct IncomingLoad {
    pub request: u64,
    adoption: Arc<AtomicU8>,
    state: Mutex<(IncomingIntent, Arc<AtomicBool>)>,
}

impl IncomingLoad {
    pub(super) fn new(request: u64, intent: IncomingIntent, adoption: Arc<AtomicU8>) -> Self {
        Self {
            request,
            adoption,
            state: Mutex::new((intent, Arc::new(AtomicBool::new(false)))),
        }
    }

    pub(super) fn snapshot(&self) -> Result<(IncomingIntent, Arc<AtomicBool>), String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "incoming intent lock poisoned")?;
        Ok((state.0, state.1.clone()))
    }

    pub(super) fn update(&self, intent: IncomingIntent) -> Result<bool, String> {
        intent.validate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "incoming intent lock poisoned")?;
        if intent.revision <= state.0.revision {
            return Err("incoming intent revision is stale".into());
        }
        let pending = match self.adoption.compare_exchange(
            0,
            INTENT_UPDATING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => true,
            Err(1 | 2) => false,
            Err(_) => return Err("incoming source adoption is no longer available".into()),
        };
        if pending {
            state.1.store(true, Ordering::Release);
        }
        *state = (intent, Arc::new(AtomicBool::new(false)));
        #[cfg(test)]
        if pending {
            observe_update_release();
        }
        if pending
            && self
                .adoption
                .compare_exchange(INTENT_UPDATING, 0, Ordering::Release, Ordering::Acquire)
                .is_err()
        {
            return Err("incoming source was rejected during intent update".into());
        }
        // False truthfully means the callback already owns this source. Its
        // matching old receipt is not an ACK of the separately addressed edit;
        // the control layer must settle that edit on the adopted source.
        Ok(pending)
    }
}
