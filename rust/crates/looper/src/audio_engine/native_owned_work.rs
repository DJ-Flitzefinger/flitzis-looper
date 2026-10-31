//! Immutable worker captures which never retain a Python Engine borrow.

use super::{AudioEngine, cold_store::CommittedColdLease, project_assets::ProjectAssets};
use crate::messages::SampleBuffer;
use std::sync::Arc;

pub(super) struct CompleteSampleWork {
    reference: SampleBuffer,
    lease: Option<CommittedColdLease>,
    assets: Arc<ProjectAssets>,
}

impl CompleteSampleWork {
    pub(super) fn capture(
        engine: &AudioEngine,
        id: usize,
        reference: &SampleBuffer,
    ) -> Result<Self, String> {
        let lease = if reference.resident_start() == 0
            && reference.resident_end() == reference.frame_count()
        {
            None
        } else {
            Some(
                engine
                    .cold_leases
                    .lock()
                    .map_err(|_| "cold lease lock poisoned")?
                    .get(id)
                    .and_then(Clone::clone)
                    .ok_or("complete source lease unavailable")?,
            )
        };
        Ok(Self {
            reference: reference.clone(),
            lease,
            assets: engine.project_assets.clone(),
        })
    }

    pub(super) fn materialize(&self, maximum: usize) -> Result<SampleBuffer, String> {
        let Some(lease) = &self.lease else {
            return Ok(self.reference.clone());
        };
        let complete = lease
            .read_complete(&self.reference, maximum)
            .map_err(|error| error.to_string())?;
        self.assets
            .retain_cold_reader(lease, &complete)
            .map_err(|error| error.to_string())?;
        Ok(complete)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkPoint {
    StemConversion,
    CommittedStemAdmission,
    CommittedStemPreflight,
    CommittedStemRegistration,
    BindingPreflight,
    WindowSourceRead,
    WindowSourceChunk,
    TimingPcm,
    SavedTimingSource,
    SavedTimingEncode,
    SavedTimingSerialize,
    AcceptedTiming,
    LegacyStemFiles,
    StemPcmFlush,
    MaterialRestoreIo,
}

#[cfg(test)]
type WorkObserver = Box<dyn Fn(WorkPoint)>;

/// Only this observer may cross the existing bounded preparation lane. Local
/// fault fixtures keep their non-Send observer and thread-local state.
#[cfg(test)]
pub(super) type PreparationObserver = Box<dyn Fn(WorkPoint) + Send>;

#[cfg(test)]
thread_local! {
    static OBSERVER: std::cell::RefCell<Option<WorkObserver>> = const { std::cell::RefCell::new(None) };
    static PREPARATION_OBSERVER: std::cell::RefCell<Option<PreparationObserver>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_observer(observer: Option<WorkObserver>) {
    OBSERVER.with(|slot| *slot.borrow_mut() = observer);
}

#[cfg(test)]
pub(super) fn set_preparation_observer(observer: Option<PreparationObserver>) {
    PREPARATION_OBSERVER.with(|slot| *slot.borrow_mut() = observer);
}

#[cfg(test)]
pub(super) fn take_preparation_observer() -> Option<PreparationObserver> {
    PREPARATION_OBSERVER.with(|slot| slot.borrow_mut().take())
}

#[cfg(test)]
pub(super) struct PreparationObservationScope(Option<PreparationObserver>);

#[cfg(test)]
impl PreparationObservationScope {
    pub(super) fn install(observer: Option<PreparationObserver>) -> Self {
        Self(PREPARATION_OBSERVER.with(|slot| slot.replace(observer)))
    }
}

#[cfg(test)]
impl Drop for PreparationObservationScope {
    fn drop(&mut self) {
        PREPARATION_OBSERVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[cfg(test)]
pub(super) fn observe_preparation(point: WorkPoint) {
    PREPARATION_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow().as_ref() {
            observer(point);
        }
    });
}

#[cfg(test)]
pub(super) fn observe(point: WorkPoint) {
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow().as_ref() {
            observer(point);
        }
    });
    observe_preparation(point);
}
