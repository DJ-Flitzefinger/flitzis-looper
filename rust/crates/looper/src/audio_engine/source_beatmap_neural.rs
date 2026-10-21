//! Selected Beat This acquisition on the existing cold map lane, never realtime.

use super::{JobState, create_staged_pcm};
use crate::audio_engine::analysis_pcm::MONO_RULE;
use crate::audio_engine::complete_context::{CompleteSourceReader, NativeAnalysisOwner};
use crate::audio_engine::source_beatmap_capture::{
    RawCapture,
    beat_this::{BeatThisRawData, BeatThisSourceInput, pinned_model},
};
use pyo3::prelude::*;
use pyo3::types::PyString;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const JOB_LIMIT: usize = 512 * 1024 * 1024;
const BRIDGE_LIMIT: usize = 16 * 1024 * 1024;
// Simultaneous Python wire/decoded tuples/packed output allowance (256 MiB),
// Native capture/codec/reparse allowance (64 MiB), fixed metadata/I/O (16 MiB).
// The model subprocess RSS is separate; this does not claim a whole-tree cap.
const WORK_ALLOWANCE: usize = 336 * 1024 * 1024;

#[cfg(all(test, windows))]
#[path = "source_beatmap_neural_tests.rs"]
mod tests;

#[pyclass(frozen)]
struct SourceMapCancellation {
    cancelled: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    owner: NativeAnalysisOwner,
}

#[pymethods]
impl SourceMapCancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self.shutdown.load(Ordering::Acquire)
            || !self.owner.current()
    }
}

struct MonoWriter<'a> {
    file: &'a mut File,
    digest: Sha256,
    bytes: usize,
}

impl Write for MonoWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.digest.update(&bytes[..written]);
        self.bytes = self
            .bytes
            .checked_add(written)
            .ok_or_else(|| io::Error::other("Beat This staging byte overflow"))?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn verify_staged(
    file: &mut File,
    expected_bytes: usize,
    expected_digest: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut bytes = 0_usize;
    let mut scratch = [0_u8; 16 * 1024];
    loop {
        if cancelled() {
            return Err("Beat This original acquisition cancelled".into());
        }
        let read = file.read(&mut scratch).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read)
            .ok_or("Beat This staged byte overflow")?;
        if bytes > expected_bytes {
            return Err("Beat This staged extent changed".into());
        }
        digest.update(&scratch[..read]);
    }
    if bytes != expected_bytes || format!("{:x}", digest.finalize()) != expected_digest {
        return Err("Beat This staged input identity changed".into());
    }
    Ok(())
}

impl JobState {
    pub(super) fn acquire_beat_this_raw(
        &self,
        reader: &CompleteSourceReader,
        expected_generation: u64,
        installation_root: &str,
    ) -> Result<RawCapture, String> {
        self.require_purpose(super::OfflinePurpose::SourceBeatmap)?;
        let cancelled = || self.cancelled();
        if cancelled() {
            return Err("Beat This acquisition cancelled".into());
        }
        reader.require_map_custody()?;
        let guard = reader
            .guard
            .as_ref()
            .ok_or("Beat This original source owner unavailable")?;
        if expected_generation == 0
            || guard.generation != expected_generation
            || !Path::new(installation_root).is_absolute()
        {
            return Err("Beat This original source generation or installation is invalid".into());
        }
        let held = reader.held_bytes()?;
        if held
            .checked_add(WORK_ALLOWANCE)
            .is_none_or(|peak| peak > JOB_LIMIT)
        {
            return Err("Beat This source-map simultaneous job peak exceeds 512 MiB".into());
        }
        let frames = reader.reference.frame_count();
        let pcm_bytes = frames
            .checked_mul(4)
            .ok_or("Beat This complete input extent overflow")?;
        if pcm_bytes == 0 || pcm_bytes > JOB_LIMIT || !(8_000..=384_000).contains(&guard.rate) {
            return Err("Beat This complete input extent or rate unsupported".into());
        }
        let (source_descriptor, decoder_identity) =
            reader.raw_source_descriptor(expected_generation, &cancelled)?;
        let (root, _) = reader.map_artifact_paths()?;
        let temporary = tempfile::Builder::new()
            .prefix(".beat-this-source-map-")
            .tempdir_in(root)
            .map_err(|error| error.to_string())?;
        // All inner file/worker owners settle before explicit directory cleanup,
        // including errors. Never silently rely on TempDir::drop for settlement.
        let acquisition = (|| -> Result<RawCapture, String> {
            let path = temporary.path().join("mono.f32le");
            let path_text = path
                .to_str()
                .ok_or("Beat This staging path is not representable")?;
            // The existing Windows share-read handle prevents writers/path replacement
            // while the worker borrows this actual Native-produced complete input.
            let mut file = create_staged_pcm(path_text).map_err(|error| error.to_string())?;
            let mut writer = MonoWriter {
                file: &mut file,
                digest: Sha256::new(),
                bytes: 0,
            };
            reader
                .stream_mono(&mut writer, &cancelled)
                .map_err(|error| error.to_string())?;
            writer.flush().map_err(|error| error.to_string())?;
            if writer.bytes != pcm_bytes {
                return Err("Beat This complete mono extent differs".into());
            }
            let mono_sha256 = format!("{:x}", writer.digest.finalize());
            file.sync_all().map_err(|error| error.to_string())?;
            let identity = json!({"pad_id":self.identity.pad_id,"request_id":self.identity.request_id,
            "source_id":self.identity.source_id,"source_generation":self.identity.source_generation});
            let identity_json = identity.to_string();
            let expected_request = json!({"schema_version":1,"identity":identity,
            "pcm":{"path":path_text,"sample_rate_hz":guard.rate,"frame_count":frames,
                "origin_seconds":0.0,"dtype":"float32-le","channels":1},"model":pinned_model()})
            .to_string();
            if expected_request.len() > 32 * 1024 {
                return Err("Beat This request byte limit exceeded".into());
            }
            let probe = SourceMapCancellation {
                cancelled: self.cancelled.clone(),
                shutdown: self
                    .async_map
                    .get()
                    .ok_or("Beat This acquisition requires existing async map owner")?
                    .shutdown_token(),
                owner: self
                    .native_owner
                    .as_ref()
                    .ok_or("Beat This original Native owner unavailable")?
                    .clone(),
            };
            let source = BeatThisSourceInput {
                decoder_identity,
                source_descriptor,
                source_sample_rate_hz: guard.rate,
                source_frame_count: frames,
                source_zero_frame: 0,
                mono_sha256: mono_sha256.clone(),
                mono_rule: MONO_RULE.into(),
            };
            // Python orchestration runs only on this already-admitted cold worker.
            // The bridge returns only after its actual process/readers have retired.
            let raw = Python::attach(|py| -> Result<RawCapture, String> {
                let module = py
                    .import("flitzis_looper.analysis.source_map_worker")
                    .map_err(|error| error.to_string())?;
                let probe = Py::new(py, probe).map_err(|error| error.to_string())?;
                let result = module
                    .getattr("run_source_map_beat_this")
                    .and_then(|entry| {
                        entry.call1((
                            path_text,
                            guard.rate,
                            frames,
                            &mono_sha256,
                            identity_json,
                            installation_root,
                            probe,
                        ))
                    })
                    .map_err(|error| error.to_string())?;
                let result = result
                    .cast::<PyString>()
                    .map_err(|error| error.to_string())?;
                let encoded = result.to_str().map_err(|error| error.to_string())?;
                if encoded.len() > BRIDGE_LIMIT {
                    return Err("Beat This bridge response byte limit exceeded".into());
                }
                let raw = RawCapture::from(BeatThisRawData::from_worker(
                    source,
                    &expected_request,
                    encoded,
                )?);
                raw.publication_peak_bytes(held, encoded.len())?;
                Ok(raw)
            })?;
            if cancelled() {
                return Err("Beat This original acquisition cancelled".into());
            }
            reader.require_map_custody()?;
            verify_staged(&mut file, pcm_bytes, &mono_sha256, &cancelled)?;
            drop(file);
            Ok(raw)
        })();
        let staging_path = temporary.path().to_owned();
        match (acquisition, temporary.close()) {
            (result, Ok(())) => result,
            (Ok(_), Err(error)) => Err(format!(
                "Beat This staging cleanup failed at {}: {error}",
                staging_path.display()
            )),
            (Err(primary), Err(error)) => Err(format!(
                "Beat This staging cleanup failed at {}: {error}; acquisition failed: {primary}",
                staging_path.display()
            )),
        }
    }
}
