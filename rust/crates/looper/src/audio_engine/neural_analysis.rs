//! Strict Beat This execution helpers on admitted analysis workers, never realtime.

pub(super) mod budget;
mod codec;
pub(super) mod descriptor;
pub(super) mod raw;
use pyo3::prelude::*;
use pyo3::types::PyString;
use raw::{BeatThisRawData, BeatThisSourceInput, pinned_cuda_model, pinned_model};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

const BRIDGE_LIMIT: usize = 16 * 1024 * 1024;

pub(in crate::audio_engine) struct MonoWriter<'a> {
    pub(in crate::audio_engine) file: &'a mut File,
    pub(in crate::audio_engine) digest: Sha256,
    pub(in crate::audio_engine) bytes: usize,
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

pub(in crate::audio_engine) fn verify_staged(
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

/// Borrow the installed worker within the caller's admitted analysis. The caller keeps
/// genuine source/request custody and all staging readers until this returns.
pub(in crate::audio_engine) fn invoke_worker(
    source: BeatThisSourceInput,
    identity: serde_json::Value,
    path: &str,
    installation_root: &str,
    probe: Py<PyAny>,
) -> Result<(BeatThisRawData, usize), String> {
    let mut expected = json!({"schema_version":1,"identity":identity,
        "pcm":{"path":path,"sample_rate_hz":source.source_sample_rate_hz,
            "frame_count":source.source_frame_count,"origin_seconds":0.0,
            "dtype":"float32-le","channels":1},"model":pinned_model()});
    if expected.to_string().len() > 32 * 1024 {
        return Err("Beat This request byte limit exceeded".into());
    }
    Python::attach(|py| {
        let module = py
            .import("flitzis_looper.analysis.neural_worker")
            .map_err(|e| e.to_string())?;
        let result = module
            .getattr("run_beat_this")
            .and_then(|entry| {
                entry.call1((
                    path,
                    source.source_sample_rate_hz,
                    source.source_frame_count,
                    &source.mono_sha256,
                    identity.to_string(),
                    installation_root,
                    probe,
                ))
            })
            .map_err(|e| e.to_string())?;
        let encoded = result
            .cast::<PyString>()
            .map_err(|e| e.to_string())?
            .to_str()
            .map_err(|e| e.to_string())?;
        if encoded.len() > BRIDGE_LIMIT {
            return Err("Beat This bridge response byte limit exceeded".into());
        }
        let length = encoded.len();
        // The bridge resolves its configuration once. Only its closed protocol
        // discriminator chooses a known profile; all expected request fields
        // below remain derived from this Native source and request owner.
        #[derive(serde::Deserialize)]
        struct BridgeVersion {
            schema_version: u32,
        }
        let version: BridgeVersion = serde_json::from_str(encoded).map_err(|e| e.to_string())?;
        if version.schema_version == 2 {
            expected["schema_version"] = 2.into();
            expected["model"] = pinned_cuda_model();
            expected["execution_policy"] = "auto".into();
        } else if version.schema_version != 1 {
            return Err("unknown Beat This worker profile".into());
        }
        BeatThisRawData::from_worker(source, &expected.to_string(), encoded)
            .map(|raw| (raw, length))
    })
}

#[cfg(test)]
#[path = "neural_analysis/tests.rs"]
mod tests;
