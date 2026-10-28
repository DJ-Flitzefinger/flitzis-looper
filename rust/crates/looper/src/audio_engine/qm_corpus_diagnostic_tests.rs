//! Opt-in source-signal diagnostic. No Engine, device, NN, key or publication.
#![cfg(windows)]

use crate::audio_engine::{analysis_pcm, c3_process_metrics as process, cold_store, sample_loader};
use flitzis_looper_analysis::{AnalysisConfig, analyze_bpm_raw, default_bpm_working_bytes};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const LIMIT: usize = 512 * 1024 * 1024;
// Paths/descriptors/configuration, hash buffers, 4096 stereo frames, serializer
// scratch and allocator headers remain separate from every complete PCM array.
const FIXED_BYTES: usize = 1024 * 1024;
const CHUNK_FRAMES: usize = 4096;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn sum(values: &[usize]) -> Result<usize> {
    values
        .iter()
        .try_fold(0_usize, |n, value| n.checked_add(*value))
        .ok_or_else(|| "QM diagnostic byte overflow".into())
}

fn bytes<T>(count: usize) -> Result<usize> {
    count
        .checked_mul(size_of::<T>())
        .ok_or_else(|| "QM diagnostic extent overflow".into())
}

fn admit(values: &[usize]) -> Result<usize> {
    let peak = sum(&[sum(values)?, FIXED_BYTES])?;
    if peak > LIMIT {
        return Err("complete QM diagnostic exceeds 512 MiB".into());
    }
    Ok(peak)
}

fn reserve<T>(count: usize, other_bytes: usize) -> Result<Vec<T>> {
    admit(&[bytes::<T>(count)?, other_bytes])?;
    let mut result = Vec::new();
    result.try_reserve_exact(count)?;
    admit(&[bytes::<T>(result.capacity())?, other_bytes])?;
    Ok(result)
}

fn digest(reader: &mut File) -> Result<String> {
    reader.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    reader.seek(SeekFrom::Start(0))?;
    Ok(format!("{:x}", hash.finalize()))
}

fn path_digest(path: &Path) -> Result<String> {
    digest(&mut cold_store::sealed_reader(path)?)
}

#[derive(Serialize)]
struct RawCapture<'a> {
    beat_frames: &'a [f64],
    beat_seconds: &'a [f64],
    downbeat_raw_indices: &'a [usize],
    downbeat_seconds: &'a [f64],
    odf_hop_samples: usize,
    actual_step_seconds: f64,
    legacy_bpm: f32,
}

#[derive(Serialize)]
struct Capture<'a> {
    schema_version: u32,
    diagnostic_only: bool,
    source: Value,
    decoder: Value,
    playback: Value,
    analyzer: Value,
    configuration: Value,
    preparation: Value,
    runtime: Value,
    process_before: Value,
    process_after: Value,
    elapsed_seconds: f64,
    cancellation: &'static str,
    raw: RawCapture<'a>,
}

#[test]
#[ignore = "explicit frozen source/SHA and OWNER.md output; one device-free QM process"]
fn authorized_source_qm_capture() -> Result<()> {
    let source = PathBuf::from(std::env::var_os("FLITZI_QM_SOURCE").ok_or("explicit QM source")?);
    let expected = std::env::var("FLITZI_QM_SOURCE_SHA256")?;
    let output = PathBuf::from(std::env::var_os("FLITZI_QM_OUTPUT").ok_or("explicit QM output")?);
    assert!(source.is_absolute() && output.is_absolute());
    assert!(source.as_os_str().len() <= 32_768 && output.as_os_str().len() <= 32_768);
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    );
    let parent = output.parent().ok_or("QM output parent")?.canonicalize()?;
    let owner = parent.join("OWNER.md");
    assert!(fs::metadata(&owner)?.len() > 0 && fs::metadata(&owner)?.len() <= 65_536);
    assert!(!fs::read_to_string(owner)?.trim().is_empty());
    let output = parent.join(output.file_name().ok_or("QM output filename")?);
    assert!(!output.exists(), "inspect previous effects before retry");
    let started = Instant::now();
    let process_before = process::snapshot();
    let executable = std::env::current_exe()?;
    let executable_sha256 = path_digest(&executable)?;
    let runtime = json!({"process_id":std::process::id(),
        "native_executable":{"path":executable,"sha256":executable_sha256},
        "compiled_helper_sha256":format!("{:x}",Sha256::digest(include_bytes!("qm_corpus_diagnostic_tests.rs")))});
    eprintln!(
        "QM_DIAGNOSTIC_START pid={} source_sha256={expected}",
        std::process::id()
    );
    let temporary = tempfile::tempdir_in(&parent)?;
    let snapshot_path = temporary.path().join("source.snapshot");
    let mut original = cold_store::sealed_reader(&source)?;
    let source_before = digest(&mut original)?;
    assert_eq!(source_before, expected);
    let source_bytes = original.metadata()?.len();
    let mut snapshot_writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot_path)?;
    assert_eq!(io::copy(&mut original, &mut snapshot_writer)?, source_bytes);
    snapshot_writer.sync_all()?;
    drop(snapshot_writer);
    assert_eq!(digest(&mut original)?, expected);
    drop(original);
    let mut snapshot = cold_store::sealed_reader(&snapshot_path)?;
    let snapshot_sha256 = digest(&mut snapshot)?;
    assert_eq!(snapshot_sha256, expected);
    let mut decoded = sample_loader::decode_audio_snapshot_streamed(
        snapshot.try_clone()?,
        &source,
        &temporary.path().join("decoder.f32le"),
        48_000,
        LIMIT - FIXED_BYTES,
        &|| false,
        |_| {},
    )?;
    let decoder = decoded.artifact.descriptor.clone();
    let (mut playback, transform) = sample_loader::prepare_playback_streamed(
        &mut decoded,
        &temporary.path().join("playback.f32le"),
        2,
        48_000,
        LIMIT - FIXED_BYTES,
        &|| false,
        |_| {},
    )?;
    let stream_scratch = decoded.scratch_reserved_bytes;
    let stream_peak = admit(&[stream_scratch])?;
    let playback_description =
        json!({"pcm":playback.descriptor.clone(),"transform":transform.to_json()});
    drop(decoded);
    let frames = usize::try_from(
        playback_description["pcm"]["full_frames"]
            .as_u64()
            .ok_or("QM playback frames")?,
    )?;
    assert!(frames > 0);
    let converted_frames = usize::try_from((frames as u128 * 44_100).div_ceil(48_000))?;
    let (fft_input, fft_output) = analysis_pcm::fft::fft_dimensions(48_000, 44_100);
    let fft_bytes = sum(&[fft_input, fft_output])?
        .checked_mul(64 * 4)
        .ok_or("QM FFT overflow")?;
    // The pinned FFT delay and one final output unit fit in two output units.
    let converted_capacity_bound = sum(&[
        converted_frames,
        fft_output.checked_mul(2).ok_or("QM delay overflow")?,
    ])?;
    let converted_bytes_bound = bytes::<f32>(converted_capacity_bound)?;
    let qm_workspace =
        default_bpm_working_bytes(converted_frames).ok_or("QM workspace overflow")?;
    let converter_preflight = admit(&[bytes::<f32>(frames)?, converted_bytes_bound, fft_bytes])?;
    let widening_preflight = admit(&[converted_bytes_bound, bytes::<f64>(converted_frames)?])?;
    let qm_preflight = admit(&[bytes::<f64>(converted_frames)?, qm_workspace])?;
    let mut mono = reserve::<f32>(frames, 0)?;
    let mono_capacity_bytes = bytes::<f32>(mono.capacity())?;
    let converter_peak = admit(&[mono_capacity_bytes, converted_bytes_bound, fft_bytes])?;
    playback.reader.seek(SeekFrom::Start(0))?;
    let mut buffer = [0_u8; CHUNK_FRAMES * 2 * 4];
    let mut mono_hash = Sha256::new();
    let mut left = frames;
    while left > 0 {
        let count = left.min(CHUNK_FRAMES);
        playback.reader.read_exact(&mut buffer[..count * 8])?;
        for frame in buffer[..count * 8].chunks_exact(8) {
            let l = f32::from_le_bytes(frame[..4].try_into()?);
            let r = f32::from_le_bytes(frame[4..].try_into()?);
            let value = (l + r) * 0.5;
            assert!(l.is_finite() && r.is_finite() && value.is_finite());
            mono_hash.update(value.to_le_bytes());
            mono.push(value);
        }
        left -= count;
    }
    assert_eq!(playback.reader.read(&mut [0_u8; 1])?, 0);
    drop(playback);
    let normal_mono_sha256 = format!("{:x}", mono_hash.finalize());
    let converted = analysis_pcm::resample_mono_cancellable(
        mono,
        48_000,
        44_100,
        LIMIT - FIXED_BYTES - fft_bytes,
        &|| false,
    )?;
    assert_eq!(converted.len(), converted_frames);
    let converted_capacity_bytes = bytes::<f32>(converted.capacity())?;
    assert!(converted_capacity_bytes <= converted_bytes_bound);
    let mut analyzer = reserve::<f64>(converted_frames, converted_capacity_bytes)?;
    let widening_peak = admit(&[converted_capacity_bytes, bytes::<f64>(analyzer.capacity())?])?;
    let mut analyzer_hash = Sha256::new();
    for &value in &converted {
        let value = f64::from(value);
        assert!(value.is_finite());
        analyzer_hash.update(value.to_le_bytes());
        analyzer.push(value);
    }
    drop(converted);
    let analyzer_sha256 = format!("{:x}", analyzer_hash.finalize());
    let qm_peak = admit(&[bytes::<f64>(analyzer.capacity())?, qm_workspace])?;
    let config = AnalysisConfig::default();
    // QM has no internal cancellation callback. The Root owns this one PID;
    // source/resource checks bracket the call, without claiming cooperative stop.
    assert_eq!(path_digest(&source)?, expected);
    let raw = analyze_bpm_raw(&analyzer, 44_100, &config)?;
    assert_eq!(raw.input_frame_count(), converted_frames);
    assert_eq!(raw.input_sample_rate_hz(), 44_100);
    drop(analyzer);
    let seconds_bytes = sum(&[
        bytes::<f64>(raw.beat_frames().len())?,
        bytes::<f64>(raw.downbeat_raw_indices().len())?,
    ])?;
    let legacy_bytes = sum(&[
        bytes::<f32>(raw.beat_frames().len())?,
        bytes::<f32>(raw.downbeat_raw_indices().len())?
            .checked_mul(2)
            .ok_or("QM legacy extent overflow")?,
    ])?;
    let raw_report_preflight = admit(&[qm_workspace, seconds_bytes, legacy_bytes])?;
    let mut beats = reserve::<f64>(raw.beat_frames().len(), qm_workspace)?;
    beats.extend(raw.beat_seconds());
    let mut downbeats = reserve::<f64>(
        raw.downbeat_raw_indices().len(),
        sum(&[qm_workspace, bytes::<f64>(beats.capacity())?])?,
    )?;
    downbeats.extend(raw.downbeat_seconds());
    let raw_report_peak = admit(&[
        qm_workspace,
        bytes::<f64>(beats.capacity())?,
        bytes::<f64>(downbeats.capacity())?,
        legacy_bytes,
    ])?;
    let (legacy_bpm, legacy_grid) = raw.legacy_result();
    drop(legacy_grid);
    let source_after = path_digest(&source)?;
    assert_eq!(source_after, expected);
    drop(snapshot);
    // All snapshot and PCM readers are closed before the owned directory retires.
    temporary.close()?;
    let actual_hop = raw.odf_hop_samples();
    let process_after = process::snapshot();
    let capture = Capture {
        schema_version: 1,
        diagnostic_only: true,
        source: json!({"path":source,"sha256_before":source_before,"snapshot_sha256":snapshot_sha256,
            "sha256_after":source_after,"bytes":source_bytes}),
        decoder,
        playback: playback_description,
        analyzer: json!({"sample_rate_hz":44_100,"frame_count":converted_frames,"origin_seconds":0.0,
            "f64_le_sha256":analyzer_sha256,"normal_mono_f32_sha256":normal_mono_sha256,
            "downmix_revision":"normal-analysis-stereo-f32-add-multiply-half-v1",
            "resampler":"rubato-fft-1.0-44100-delay-trim-tail-flush-v1"}),
        configuration: json!({"step_secs":config.step_secs,"max_bin_hz":config.max_bin_hz,
            "input_tempo":config.input_tempo,"alpha":config.alpha,"tightness":config.tightness,
            "viterbi_sigma":config.viterbi_sigma,"window_length":config.window_length,"hop_size":config.hop_size}),
        preparation: json!({"limit_bytes":LIMIT,"fixed_overhead_bytes":FIXED_BYTES,
            "stream_scratch_reserved_bytes":stream_scratch,"stream_peak_bytes":stream_peak,
            "mono_capacity_bytes":mono_capacity_bytes,"converted_capacity_bytes":converted_capacity_bytes,
            "converter_fft_bound_bytes":fft_bytes,"converter_preflight_bytes":converter_preflight,
            "converter_peak_bound_bytes":converter_peak,"widening_preflight_bytes":widening_preflight,
            "widening_peak_bytes":widening_peak,"qm_preflight_bytes":qm_preflight,
            "qm_working_bytes":qm_workspace,"qm_peak_bytes":qm_peak,"raw_report_preflight_bytes":raw_report_preflight,
            "raw_report_peak_bound_bytes":raw_report_peak,
            "peak_bound_bytes":([stream_peak,converter_peak,widening_peak,qm_peak,raw_report_peak].into_iter().max().unwrap())}),
        runtime,
        process_before,
        process_after,
        elapsed_seconds: started.elapsed().as_secs_f64(),
        cancellation: "No cooperative callback inside QM; pre/post source/resource checks and Root-owned process interruption only",
        raw: RawCapture {
            beat_frames: raw.beat_frames(),
            beat_seconds: &beats,
            downbeat_raw_indices: raw.downbeat_raw_indices(),
            downbeat_seconds: &downbeats,
            odf_hop_samples: actual_hop,
            actual_step_seconds: actual_hop as f64 / 44_100.0,
            legacy_bpm,
        },
    };
    let mut report = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    serde_json::to_writer(&mut report, &capture)?;
    report.write_all(b"\n")?;
    report.sync_all()?;
    println!(
        "QM_DIAGNOSTIC_COMPLETE source_sha256={expected} pid={}",
        std::process::id()
    );
    Ok(())
}
