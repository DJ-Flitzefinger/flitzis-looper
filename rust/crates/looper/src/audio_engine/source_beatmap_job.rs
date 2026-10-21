//! Source-bound map validation on the existing offline reservation and transitions.

use super::{JobState, OfflineInput, OfflinePurpose, RunningGuard};
use crate::audio_engine::source_beatmap::SourceBeatmap;
use crate::messages::LoaderEvent;
use std::sync::atomic::Ordering;

impl JobState {
    pub(super) fn prepare_source_beatmap(
        &self,
        encoded: &str,
        after_verify: impl FnOnce(),
    ) -> Result<(), String> {
        self.require_purpose(OfflinePurpose::SourceBeatmap)?;
        let transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if self.finished.load(Ordering::Acquire)
            || self.cancelled()
            || self.retirement_started.load(Ordering::Acquire)
            || self.pcm_retired.load(Ordering::Acquire)
            || self.preparing.swap(true, Ordering::AcqRel)
        {
            return Err("source beatmap preparation is unavailable".into());
        }
        let _running = RunningGuard(&self.preparing);
        drop(transition);
        let mut prepared = self
            .prepared_map
            .lock()
            .map_err(|_| "source beatmap result lock poisoned")?;
        if prepared.is_some() {
            return Err("source beatmap already prepared".into());
        }
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "offline snapshot lock poisoned")?;
        let Some(OfflineInput::Complete(reader)) = snapshot.as_ref() else {
            return Err("source beatmap original custody unavailable".into());
        };
        reader.require_map_custody()?;
        let map = SourceBeatmap::parse(encoded)?;
        reader.verify_source_beatmap(&map, &|| self.cancelled())?;
        // Tests block this exact admitted request while all real readers and the
        // running reservation remain held. Production supplies an empty closure.
        after_verify();
        if self.cancelled() {
            return Err("source beatmap preparation superseded or cancelled".into());
        }
        *prepared = Some(map);
        Ok(())
    }

    pub(super) fn finish_source_beatmap(&self) -> Result<bool, String> {
        self.require_purpose(OfflinePurpose::SourceBeatmap)?;
        let _transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if self.preparing.load(Ordering::Acquire) {
            return Err("offline native work is still retiring".into());
        }
        if self.finished.load(Ordering::Acquire) || self.retirement_started.load(Ordering::Acquire)
        {
            return Err("offline request already retired".into());
        }
        let map = self
            .prepared_map
            .lock()
            .map_err(|_| "source beatmap result lock poisoned")?
            .take()
            .ok_or("source beatmap has not been prepared")?;
        // Serialize owner validation and enqueue with the existing request boundary.
        // The captured snapshot still holds its actual original/assignment readers.
        let requests = self
            .request_ids
            .lock()
            .map_err(|_| "request lock poisoned")?;
        let source_current = {
            let snapshot = self
                .snapshot
                .lock()
                .map_err(|_| "offline snapshot lock poisoned")?;
            matches!(snapshot.as_ref(), Some(OfflineInput::Complete(reader))
                if reader.require_map_custody().is_ok())
        };
        let accepted = !self.cancelled.load(Ordering::Acquire)
            && source_current
            && self
                .native_owner
                .as_ref()
                .is_some_and(|owner| owner.current());
        let queued = if accepted {
            self.tx
                .send(self.owned_event(LoaderEvent::SourceBeatmapCompleted {
                    id: self.identity.pad_id,
                    request_id: self.identity.request_id,
                    map,
                }))
                .is_ok()
        } else {
            // Retire a rejected potentially large map on this worker before the
            // shared reservation admits subsequent work.
            drop(map);
            false
        };
        drop(requests);
        self.drop_pcm();
        self.finished.store(true, Ordering::Release);
        self.release_reservation();
        Ok(queued)
    }

    pub(super) fn discard_prepared_map(&self) {
        self.prepared_map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
