//! Source-bound map validation on the existing offline reservation and transitions.

use super::{JobState, OfflineInput, OfflinePurpose, RunningGuard};
use crate::audio_engine::source_beatmap::SourceBeatmap;
use crate::audio_engine::source_beatmap_artifact::{SourceBeatmapArtifact, StorePoint};
use crate::messages::LoaderEvent;
use std::path::Path;
use std::sync::atomic::Ordering;

pub(super) enum PreparedMap {
    Accepted {
        map: SourceBeatmap,
        artifact: Option<SourceBeatmapArtifact>,
    },
    Raw(crate::audio_engine::source_beatmap_artifact::SourceBeatmapRawArtifact),
}

pub(super) enum MapInput<'a> {
    AcquireRaw {
        expected_generation: u64,
    },
    Validate(&'a str),
    Store(&'a str),
    Restore(&'a str),
    Create {
        origin: f64,
        period: f64,
        expected_generation: Option<u64>,
    },
    Edit {
        receipt: &'a str,
        operation: &'a str,
    },
}

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
        if self.async_map.get().is_some() {
            return Err("asynchronous source beatmap owns its preparation".into());
        }
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
        self.prepare_source_beatmap_body(encoded, after_verify)
    }

    /// The caller already owns direct preparation or the original armed async guard.
    pub(super) fn prepare_source_beatmap_body(
        &self,
        encoded: &str,
        after_verify: impl FnOnce(),
    ) -> Result<(), String> {
        self.prepare_source_beatmap_input_body(
            MapInput::Validate(encoded),
            after_verify,
            &mut |_, _| Ok(()),
        )
    }

    pub(super) fn prepare_source_beatmap_input_body(
        &self,
        input: MapInput<'_>,
        after_verify: impl FnOnce(),
        observe: &mut impl FnMut(StorePoint, &Path) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.cancelled() {
            return Err("source beatmap preparation superseded or cancelled".into());
        }
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
        let cancelled = || self.cancelled();
        if let MapInput::AcquireRaw {
            expected_generation,
        } = input
        {
            let lease = self
                .raw_review
                .lock()
                .map_err(|_| "raw review admission lock poisoned")?
                .as_ref()
                .cloned()
                .ok_or("raw acquisition requires its original review admission")?;
            let raw = crate::audio_engine::source_beatmap_raw_data::RawData::acquire(
                reader,
                expected_generation,
                &cancelled,
            )?;
            let (root, original) = reader.map_artifact_paths()?;
            let artifact =
                crate::audio_engine::source_beatmap_artifact::SourceBeatmapRawArtifact::store(
                    root, original, raw, lease, &cancelled, observe,
                )?;
            reader.require_map_custody()?;
            after_verify();
            if self.cancelled() {
                return Err("raw source acquisition superseded or cancelled".into());
            }
            *prepared = Some(PreparedMap::Raw(artifact));
            return Ok(());
        }
        let (map, artifact) = match input {
            MapInput::AcquireRaw { .. } => {
                unreachable!("raw preparation returned through its typed branch")
            }
            MapInput::Validate(encoded) => {
                let map = SourceBeatmap::parse(encoded)?;
                reader.verify_source_beatmap(&map, &cancelled)?;
                (map, None)
            }
            MapInput::Store(encoded) => {
                let map = SourceBeatmap::parse(encoded)?;
                // Verify the held actual original once, then keep this source
                // custody throughout immutable artifact publication.
                reader.verify_source_beatmap(&map, &cancelled)?;
                let (root, original) = reader.map_artifact_paths()?;
                let artifact =
                    SourceBeatmapArtifact::store(root, original, map, &cancelled, observe)?;
                reader.require_map_custody()?;
                (artifact.map(), Some(artifact))
            }
            MapInput::Restore(receipt) => {
                let (root, _) = reader.map_artifact_paths()?;
                let artifact = SourceBeatmapArtifact::restore(root, receipt, &cancelled)?;
                let map = artifact.map();
                reader.verify_source_beatmap(&map, &cancelled)?;
                (map, Some(artifact))
            }
            MapInput::Create {
                origin,
                period,
                expected_generation,
            } => {
                let map = reader.manual_source_beatmap(
                    origin,
                    period,
                    expected_generation,
                    &cancelled,
                )?;
                let (root, original) = reader.map_artifact_paths()?;
                let artifact =
                    SourceBeatmapArtifact::store(root, original, map, &cancelled, observe)?;
                reader.require_map_custody()?;
                (artifact.map(), Some(artifact))
            }
            MapInput::Edit { receipt, operation } => {
                let (root, original) = reader.map_artifact_paths()?;
                let base = SourceBeatmapArtifact::restore(root, receipt, &cancelled)?;
                let map = base.map();
                // The base receipt supplies exact revision/content bytes; actual
                // held source verification hashes the original only once.
                reader.verify_source_beatmap(&map, &cancelled)?;
                let edited = map.edited(operation)?;
                if cancelled() {
                    return Err("source beatmap edit cancelled".into());
                }
                let artifact =
                    SourceBeatmapArtifact::store(root, original, edited, &cancelled, observe)?;
                reader.require_map_custody()?;
                (artifact.map(), Some(artifact))
            }
        };
        // Tests block this exact admitted request while all real readers and the
        // running reservation remain held. Production supplies an empty closure.
        after_verify();
        if self.cancelled() {
            return Err("source beatmap preparation superseded or cancelled".into());
        }
        *prepared = Some(PreparedMap::Accepted { map, artifact });
        Ok(())
    }

    pub(super) fn finish_source_beatmap(&self) -> Result<bool, String> {
        self.finish_source_beatmap_body(true)
    }

    /// Async cleanup retains both admissions after publication until actual retirement.
    pub(super) fn finish_source_beatmap_body(&self, retire: bool) -> Result<bool, String> {
        self.require_purpose(OfflinePurpose::SourceBeatmap)?;
        let _transition = self
            .transitions
            .lock()
            .map_err(|_| "offline transition lock poisoned")?;
        if retire && self.async_map.get().is_some() {
            return Err("asynchronous source beatmap owns its completion".into());
        }
        if self.preparing.load(Ordering::Acquire) {
            return Err("offline native work is still retiring".into());
        }
        if self.finished.load(Ordering::Acquire) || self.retirement_started.load(Ordering::Acquire)
        {
            return Err("offline request already retired".into());
        }
        let prepared = self
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
        let accepted = !self.cancelled()
            && source_current
            && self
                .native_owner
                .as_ref()
                .is_some_and(|owner| owner.current());
        let queued = if accepted {
            let event = match prepared {
                PreparedMap::Accepted { map, artifact } => LoaderEvent::SourceBeatmapCompleted {
                    id: self.identity.pad_id,
                    request_id: self.identity.request_id,
                    map,
                    artifact,
                },
                PreparedMap::Raw(artifact) => LoaderEvent::SourceBeatmapRawCompleted {
                    id: self.identity.pad_id,
                    request_id: self.identity.request_id,
                    artifact,
                },
            };
            self.tx.send(self.owned_event(event)).is_ok()
        } else {
            // Retire a rejected potentially large map on this worker before the
            // shared reservation admits subsequent work.
            drop(prepared);
            false
        };
        drop(requests);
        if retire {
            self.drop_pcm();
            self.finished.store(true, Ordering::Release);
            self.release_reservation();
        }
        Ok(queued)
    }

    pub(super) fn discard_prepared_map(&self) {
        self.prepared_map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
