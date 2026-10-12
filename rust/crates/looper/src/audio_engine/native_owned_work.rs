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
    TimingPcm,
    SavedTimingSource,
    SavedTimingEncode,
    SavedTimingSerialize,
    AcceptedTiming,
    LegacyStemFiles,
    MaterialRestoreIo,
}

#[cfg(test)]
type WorkObserver = Box<dyn Fn(WorkPoint)>;

#[cfg(test)]
thread_local! {
    static OBSERVER: std::cell::RefCell<Option<WorkObserver>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_observer(observer: Option<WorkObserver>) {
    OBSERVER.with(|slot| *slot.borrow_mut() = observer);
}

#[cfg(test)]
pub(super) fn observe(point: WorkPoint) {
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow().as_ref() {
            observer(point);
        }
    });
}
