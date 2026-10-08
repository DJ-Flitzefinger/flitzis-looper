//! Complete corrected QM diagnostics on the existing offline reservation.
//! Conversion/tracking, optional retained input and validation are never realtime.

use super::{MAX_PCM_BYTES, MAX_RESULT_BYTES};
use crate::audio_engine::analysis_pcm::{
    KEY_PREPROCESSING, MONO_RULE, PcmIdentity, resample_mono_cancellable,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flitzis_looper_analysis::{AnalysisConfig, analyze_bpm_raw};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub(super) const BACKEND: &str = "corrected-qm-native-v1";
const ARRAY_LIMIT: usize = 250_000;
const CHUNK_BYTES: usize = 16 * 1024;

fn cancellation(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("corrected legacy analysis cancelled".into())
    } else {
        Ok(())
    }
}

fn read_mono(
    file: &mut File,
    frames: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<(Vec<f32>, String), String> {
    let expected = frames
        .checked_mul(4)
        .ok_or("corrected legacy input size overflow")?;
    if frames == 0
        || expected > MAX_PCM_BYTES
        || file.metadata().map_err(|error| error.to_string())?.len() != expected as u64
    {
        return Err("corrected legacy complete PCM geometry mismatch".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut mono = Vec::with_capacity(frames);
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; CHUNK_BYTES];
    let mut remaining = expected;
    while remaining > 0 {
        cancellation(cancelled)?;
        let count = remaining.min(CHUNK_BYTES);
        file.read_exact(&mut bytes[..count])
            .map_err(|error| error.to_string())?;
        digest.update(&bytes[..count]);
        for chunk in bytes[..count].chunks_exact(4) {
            let value = f32::from_le_bytes(chunk.try_into().expect("complete float32 chunk"));
            if !value.is_finite() {
                return Err("corrected legacy nonfinite PCM".into());
            }
            mono.push(value);
        }
        remaining -= count;
    }
    cancellation(cancelled)?;
    Ok((mono, format!("{:x}", digest.finalize())))
}

fn retain_analyzer(
    analyzer: &[f64],
    path: Option<&str>,
    cancelled: &impl Fn() -> bool,
) -> Result<String, String> {
    let mut file = if let Some(path) = path {
        if !Path::new(path).is_absolute() {
            return Err("corrected legacy analyzer export path must be absolute".into());
        }
        Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; CHUNK_BYTES];
    for chunk in analyzer.chunks(CHUNK_BYTES / 8) {
        cancellation(cancelled)?;
        for (value, destination) in chunk.iter().zip(bytes.chunks_exact_mut(8)) {
            destination.copy_from_slice(&value.to_le_bytes());
        }
        let count = std::mem::size_of_val(chunk);
        digest.update(&bytes[..count]);
        if let Some(file) = file.as_mut() {
            file.write_all(&bytes[..count])
                .map_err(|error| error.to_string())?;
        }
    }
    if let Some(file) = file.as_mut() {
        file.sync_all().map_err(|error| error.to_string())?;
    }
    cancellation(cancelled)?;
    Ok(format!("{:x}", digest.finalize()))
}

fn packed_f64(values: impl IntoIterator<Item = f64>) -> String {
    STANDARD.encode(
        values
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

fn packed_f32(values: impl IntoIterator<Item = f32>) -> String {
    STANDARD.encode(
        values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

pub(super) fn analyze(
    file: &mut File,
    identity: &PcmIdentity,
    rate: u32,
    frames: usize,
    analyzer_output: Option<&str>,
    cancelled: &impl Fn() -> bool,
) -> Result<Value, String> {
    cancellation(cancelled)?;
    let (mono, mono_sha256) = read_mono(file, frames, cancelled)?;
    // This is the production normal-analysis converter, not a separate numerical
    // approximation. Native diagnostic mono uses the already-exported f64 mean.
    let converted = resample_mono_cancellable(mono, rate, 44_100, MAX_PCM_BYTES, cancelled)
        .map_err(|error| error.to_string())?;
    let promoted_peak = converted
        .capacity()
        .checked_mul(4)
        .and_then(|bytes| {
            converted
                .len()
                .checked_mul(8)
                .and_then(|extra| bytes.checked_add(extra))
        })
        .ok_or("corrected legacy analyzer allocation overflow")?;
    if promoted_peak > MAX_PCM_BYTES {
        return Err("corrected legacy analyzer PCM byte limit exceeded".into());
    }
    let analyzer: Vec<f64> = converted.into_iter().map(f64::from).collect();
    cancellation(cancelled)?;
    let input_sha256 = retain_analyzer(&analyzer, analyzer_output, cancelled)?;
    let config = AnalysisConfig::default();
    let raw = analyze_bpm_raw(&analyzer, 44_100, &config)?;
    cancellation(cancelled)?;
    if raw.beat_frames().len() > ARRAY_LIMIT || raw.downbeat_raw_indices().len() > ARRAY_LIMIT {
        return Err("corrected legacy raw position limit exceeded".into());
    }
    let (bpm, grid) = raw.legacy_result();
    Ok(json!({
        "schema_version":1,"backend":BACKEND,"diagnostic_only":true,
        "identity":{"pad_id":identity.pad_id,"request_id":identity.request_id,
            "source_id":identity.source_id,"source_generation":identity.source_generation},
        "loaded":{"sample_rate_hz":rate,"frame_count":frames,"origin_seconds":0.0,
            "mono_sha256":mono_sha256,"mono_rule":MONO_RULE},
        "analyzer":{"sample_rate_hz":raw.input_sample_rate_hz(),"frame_count":raw.input_frame_count(),
            "sha256":input_sha256,"odf_hop_samples":raw.odf_hop_samples(),
            "transform_revision":KEY_PREPROCESSING},
        "configuration":{"step_secs":config.step_secs,"max_bin_hz":config.max_bin_hz,
            "input_tempo":config.input_tempo,"alpha":config.alpha,"tightness":config.tightness,
            "viterbi_sigma":config.viterbi_sigma,"window_length":config.window_length,"hop_size":config.hop_size},
        "raw":{"encoding":"float64-le/base64+uint64-le/base64",
            "beat_frames":packed_f64(raw.beat_frames().iter().copied()),
            "downbeat_raw_indices":STANDARD.encode(raw.downbeat_raw_indices().iter()
                .flat_map(|index| (*index as u64).to_le_bytes()).collect::<Vec<_>>()),
            "beat_seconds":packed_f64(raw.beat_seconds()),"downbeat_seconds":packed_f64(raw.downbeat_seconds())},
        "compatibility":{"encoding":"float32-le/base64","bpm":packed_f32([bpm]),
            "beats":packed_f32(grid.beats),"downbeats":packed_f32(grid.downbeats),"bars":packed_f32(grid.bars)}
    }))
}

pub(super) fn validate_publication(
    encoded: &str,
    identity: &PcmIdentity,
    produced: Option<&str>,
) -> Result<Value, String> {
    if encoded.len() > MAX_RESULT_BYTES {
        return Err("corrected legacy result limit exceeded".into());
    }
    let expected = produced.ok_or("corrected legacy successful native analysis required")?;
    // Only the original complete native bytes may publish. This also rejects
    // duplicate fields, equivalent reformats and caller-created hash receipts,
    // before serde_json can discard duplicate-key information.
    if encoded != expected {
        return Err("corrected legacy publication differs from native producer bytes".into());
    }
    let value: Value = serde_json::from_str(encoded).map_err(|error| error.to_string())?;
    if value["backend"] != BACKEND
        || value["schema_version"] != 1
        || value["diagnostic_only"] != true
        || value["identity"]
            != json!({"pad_id":identity.pad_id,
            "request_id":identity.request_id,"source_id":identity.source_id,"source_generation":identity.source_generation})
    {
        return Err("corrected legacy publication differs from native producer".into());
    }
    Ok(value)
}

#[cfg(test)]
#[path = "corrected_legacy_tests.rs"]
mod tests;
