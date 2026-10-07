//! Finite saved-loop selection and complete immutable content descriptors.
//! All selection, allocation and file work runs outside the audio callback.

use super::cold_store::ColdManifest;
use super::source_reader::{FrameRange, effective_loop_region};
use crate::messages::{CompleteSourceIdentity, ResidentContext, ResidentSourceView, SampleBuffer};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct ResidentLoadGuard {
    pub request: u64,
    pub hint: ResidentLoadHint,
    pub rate: u32,
    pub cancelled: Arc<AtomicBool>,
}

impl ResidentLoadGuard {
    pub(super) fn loop_intent(&self, start_s: f64, end_s: Option<f64>) {
        let same = (start_s * f64::from(self.rate)).round()
            == (self.hint.start_s * f64::from(self.rate)).round()
            && end_s.is_some_and(|end| {
                (end * f64::from(self.rate)).round()
                    == (self.hint.end_s * f64::from(self.rate)).round()
            });
        if !same {
            self.cancelled.store(true, Ordering::Release);
        }
    }
    pub(super) fn key_lock_intent(&self, enabled: bool) {
        if enabled != self.hint.key_lock {
            self.cancelled.store(true, Ordering::Release);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ResidentLoadHint {
    pub start_s: f64,
    pub end_s: f64,
    pub key_lock: bool,
}

impl ResidentLoadHint {
    pub(super) fn parse(
        start: Option<f64>,
        end: Option<f64>,
        key_lock: bool,
    ) -> Result<Option<Self>, String> {
        match (start, end) {
            (None, None) if !key_lock => Ok(None),
            (Some(start_s), Some(end_s))
                if start_s.is_finite()
                    && end_s.is_finite()
                    && start_s >= 0.0
                    && end_s > start_s =>
            {
                Ok(Some(Self {
                    start_s,
                    end_s,
                    key_lock,
                }))
            }
            _ => Err("resident loop requires finite ordered source-second bounds".into()),
        }
    }

    pub(super) fn region(self, rate: u32, frames: usize) -> Result<FrameRange, String> {
        if rate == 0 || frames == 0 {
            return Err("resident source geometry is unavailable".into());
        }
        let start = (self.start_s * f64::from(rate))
            .round()
            .clamp(0.0, frames as f64) as usize;
        let end = (self.end_s * f64::from(rate))
            .round()
            .clamp(0.0, frames as f64) as usize;
        // The normal source reader's existing physical-boundary policy is the authority.
        effective_loop_region(start, Some(end), frames)
            .ok_or_else(|| "empty resident source".into())
    }

    pub(super) fn context(self) -> ResidentContext {
        if self.key_lock {
            ResidentContext::KeyLockFullTrack
        } else {
            ResidentContext::FiniteLoop
        }
    }
}

fn digest(value: &Value) -> Result<[u8; 32], String> {
    let text = value.as_str().ok_or("complete source digest missing")?;
    if text.len() != 64 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("complete source digest malformed".into());
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| "complete source digest malformed")?;
    }
    Ok(bytes)
}

pub(super) fn identity(manifest: &ColdManifest) -> Result<Arc<CompleteSourceIdentity>, String> {
    let pcm = &manifest.descriptor["playback"]["pcm"];
    let number = |key: &str| pcm[key].as_u64().ok_or("complete source dimension missing");
    let frame_count =
        usize::try_from(number("full_frames")?).map_err(|_| "source frame overflow")?;
    let channels = usize::try_from(number("channels")?).map_err(|_| "source channels overflow")?;
    let sample_rate_hz = u32::try_from(number("rate_hz")?).map_err(|_| "source rate overflow")?;
    if frame_count == 0
        || channels == 0
        || sample_rate_hz == 0
        || pcm["source_zero_bits"] != "0000000000000000"
        || frame_count
            .checked_mul(channels)
            .and_then(|n| n.checked_mul(4))
            != usize::try_from(number("full_bytes")?).ok()
    {
        return Err("complete source descriptor dimensions disagree".into());
    }
    Ok(Arc::new(CompleteSourceIdentity {
        frame_count,
        channels,
        sample_rate_hz,
        original_sha256: digest(&manifest.descriptor["decoder"]["original"]["sha256"])?,
        playback_sha256: digest(&pcm["interleaved_sha256"])?,
        mono_sha256: digest(&pcm["mono_sha256"])?,
        // Canonical full identity includes every original/decoder/playback transform field.
        transform_sha256: digest(&Value::String(manifest.identity.clone()))?,
        source_zero_frame: 0,
    }))
}

pub(super) fn attach(sample: &mut SampleBuffer, manifest: &ColdManifest) -> Result<(), String> {
    if sample.residency.is_none() {
        let source = identity(manifest)?;
        if sample.channels != source.channels
            || sample.samples.len() != source.frame_count * source.channels
        {
            return Err("complete source PCM extent disagrees with manifest".into());
        }
        sample.residency = Some(Arc::new(ResidentSourceView {
            source,
            start_frame: 0,
            window_revision: 1,
            context: ResidentContext::FullTrack,
        }));
    }
    Ok(())
}

pub(super) fn select(
    sample: SampleBuffer,
    hint: Option<ResidentLoadHint>,
    maximum: usize,
) -> Result<SampleBuffer, String> {
    let Some(hint) = hint else {
        return Ok(sample);
    };
    let view = sample
        .residency
        .as_ref()
        .ok_or("resident selection lacks complete identity")?;
    let region = hint.region(view.source.sample_rate_hz, sample.frame_count())?;
    let (start, end) = if hint.key_lock {
        (0, sample.frame_count())
    } else {
        (region.start, region.end)
    };
    if sample.resident_start() == start && sample.resident_end() == end {
        let mut sample = sample;
        Arc::make_mut(sample.residency.as_mut().expect("checked descriptor")).context =
            hint.context();
        return Ok(sample);
    }
    let overlap = sample
        .samples
        .len()
        .checked_mul(4)
        .and_then(|old| {
            (end - start)
                .checked_mul(sample.channels)
                .and_then(|n| n.checked_mul(8))
                .and_then(|new| old.checked_add(new))
        })
        .ok_or("resident PCM overlap overflow")?;
    if overlap > maximum {
        return Err("resident selection exceeds transient PCM limit".into());
    }
    sample.window(start, end, 1, hint.context())
}
