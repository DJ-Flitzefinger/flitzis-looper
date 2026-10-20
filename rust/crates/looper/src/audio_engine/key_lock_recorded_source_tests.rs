//! Opt-in read-only recorded-source geometry; no wet owner or musical attack reference.
//!
//! This binds the confirmed private original to the existing decoder and playback transform.
//! Threshold crossings describe PCM activity only. The historical loop frames, independent
//! source attack, analysis result and causal emission contract remain unfrozen and unproved.

use super::initial_loop_start::detect_initial_loop_start;
use super::prepared_source::file_sha256;
use super::sample_loader::{decode_audio_snapshot, prepare_playback};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::mem::size_of;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

const SOURCE: &str = "C:/SCRATCHBEATS/Creutzfeld & Jakob - Anfangsstadium (Instr.).mp3";
const SOURCE_BYTES: u64 = 10_638_171;
const SOURCE_SHA256: &str = "0652da38d0884ce2d2b3bc6db75b992de704949b061481c76f61d26d30e873b8";
const OUTPUT_RATE: u32 = 48_000;
const OUTPUT_CHANNELS: usize = 2;
const PCM_BUDGET: usize = 768 * 1024 * 1024;
const EXPORTED_SECONDS: usize = 32;
const OUTPUT_FILES: [&str; 3] = [
    "decoded-44100-first32s.f32le",
    "playback-48000-first32s.f32le",
    "source-geometry.json",
];

fn source_path() -> PathBuf {
    // An ordinary test run never accesses this personal path. Even an explicitly ignored
    // run needs the exact opt-in source argument before any source filesystem access.
    let supplied = PathBuf::from(
        std::env::var_os("FLITZI_RECORDED_SOURCE")
            .expect("set FLITZI_RECORDED_SOURCE to the explicitly authorized original"),
    );
    assert!(supplied.is_absolute());
    assert_eq!(
        supplied,
        Path::new(SOURCE),
        "only the named original is authorized"
    );
    assert_eq!(fs::metadata(&supplied).unwrap().len(), SOURCE_BYTES);
    assert_eq!(file_sha256(&supplied).unwrap(), SOURCE_SHA256);
    supplied
}

fn require_regular_directory(path: &Path) {
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_dir(), "not a directory: {}", path.display());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.file_attributes() & 0x400, 0, "reparse directory");
}

fn output_directory() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let expected = workspace
        .join("test-artifacts/attack-creutzfeld-reference-20261020/native-source-geometry");
    let supplied = PathBuf::from(
        std::env::var_os("FLITZI_RECORDED_OUTPUT")
            .expect("set FLITZI_RECORDED_OUTPUT to the existing owned diagnostic directory"),
    );
    assert!(supplied.is_absolute());
    assert_eq!(
        supplied, expected,
        "output must be the exact owned block directory"
    );
    for ancestor in expected.ancestors() {
        require_regular_directory(ancestor);
    }
    let owner = fs::symlink_metadata(expected.join("OWNER.md")).unwrap();
    assert!(owner.is_file(), "Root must precreate the output OWNER.md");
    assert!(!owner.file_type().is_symlink());
    assert_eq!(owner.file_attributes() & 0x400, 0, "reparse owner marker");
    for name in OUTPUT_FILES {
        match fs::symlink_metadata(expected.join(name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            result => panic!("output must not exist: {name}: {result:?}"),
        }
    }
    expected
}

fn pcm_sha256(samples: &[f32]) -> String {
    let mut hash = Sha256::new();
    for sample in samples {
        assert!(sample.is_finite());
        hash.update(sample.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn activity_diagnostics(samples: &[f32], channels: usize, rate: u32) -> Value {
    assert!(channels > 0 && samples.len().is_multiple_of(channels));
    let rows = [0.0_f32, 0.0001, 0.001, 0.005, 0.01].map(|threshold| {
        let first = samples.chunks_exact(channels).position(|frame| {
            frame
                .iter()
                .any(|sample| sample.is_finite() && sample.abs() > threshold)
        });
        json!({
            "absolute_threshold_strictly_exceeded": threshold,
            "first_any_channel_pcm_frame": first,
            "preceding_frame_clamped_to_zero": first.map(|frame| frame.saturating_sub(1)),
            "first_activity_seconds": first.map(|frame| frame as f64 / f64::from(rate)),
        })
    });
    json!({
        "meaning": "diagnostic_activity_only_not_musical_c",
        "coordinate_domain": "containing_pcm_geometry",
        "thresholds": rows,
    })
}

fn export_pcm(path: &Path, samples: &[f32], channels: usize, rate: u32) -> Value {
    let frames = (samples.len() / channels).min(EXPORTED_SECONDS * rate as usize);
    let count = frames.checked_mul(channels).unwrap();
    let exported = &samples[..count];
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    let mut writer = BufWriter::new(file);
    for sample in exported {
        writer.write_all(&sample.to_le_bytes()).unwrap();
    }
    writer.flush().unwrap();
    writer.get_ref().sync_all().unwrap();
    json!({
        "filename": path.file_name().unwrap().to_str().unwrap(),
        "format": "interleaved-f32-little-endian",
        "source_start_frame": 0,
        "frames": frames,
        "sample_rate_hz": rate,
        "channels": channels,
        "bytes": count * size_of::<f32>(),
        "sha256": pcm_sha256(exported),
    })
}

#[test]
#[ignore = "explicit private MP3 geometry only; requires FLITZI_RECORDED_SOURCE and preowned FLITZI_RECORDED_OUTPUT"]
fn confirmed_recorded_source_reports_production_decoder_and_48k_playback_origin() {
    let source = source_path();
    let output = output_directory();
    let decoded = decode_audio_snapshot(
        File::open(&source).unwrap(),
        &source,
        OUTPUT_RATE,
        PCM_BUDGET,
        &|| false,
        |_| {},
    )
    .unwrap();
    assert_eq!(decoded.rate_hz, 44_100);
    assert_eq!(decoded.channels, 2);
    assert!(decoded.samples.len().is_multiple_of(decoded.channels));
    let decoded_frames = decoded.samples.len() / decoded.channels;
    let decoded_sha256 = pcm_sha256(&decoded.samples);
    let decoder = decoded.decoder.to_json();
    let (playback, transform) = prepare_playback(
        &decoded,
        OUTPUT_CHANNELS,
        OUTPUT_RATE,
        PCM_BUDGET,
        &|| false,
        |_| {},
    )
    .unwrap();
    assert_eq!(playback.channels, OUTPUT_CHANNELS);
    assert!(playback.samples.len().is_multiple_of(OUTPUT_CHANNELS));
    let playback_frames = playback.samples.len() / OUTPUT_CHANNELS;
    let expected_frames =
        (decoded_frames as u128 * u128::from(OUTPUT_RATE)).div_ceil(u128::from(decoded.rate_hz));
    assert_eq!(playback_frames as u128, expected_frames);
    let playback_sha256 = pcm_sha256(&playback.samples);
    assert_eq!(file_sha256(&source).unwrap(), SOURCE_SHA256);
    assert_eq!(fs::metadata(&source).unwrap().len(), SOURCE_BYTES);
    let decoded_export = export_pcm(
        &output.join(OUTPUT_FILES[0]),
        &decoded.samples,
        decoded.channels,
        decoded.rate_hz,
    );
    let playback_export = export_pcm(
        &output.join(OUTPUT_FILES[1]),
        &playback.samples,
        playback.channels,
        OUTPUT_RATE,
    );
    let report = json!({
        "schema": "recorded-source-geometry-v1",
        "source": {"bytes": SOURCE_BYTES, "sha256": SOURCE_SHA256},
        "pcm_all_frames_hashed": true,
        "pcm_budget_bytes": PCM_BUDGET,
        "decoded": {
            "sample_rate_hz": decoded.rate_hz, "channels": decoded.channels,
            "frames": decoded_frames, "bytes": decoded.samples.len() * size_of::<f32>(),
            "pcm_sha256": decoded_sha256, "decoder": decoder,
            "activity": activity_diagnostics(&decoded.samples, decoded.channels, decoded.rate_hz),
        },
        "playback": {
            "sample_rate_hz": OUTPUT_RATE, "channels": playback.channels,
            "frames": playback_frames, "bytes": playback.samples.len() * size_of::<f32>(),
            "pcm_sha256": playback_sha256, "transform": transform.to_json(),
            "activity": activity_diagnostics(&playback.samples, playback.channels, OUTPUT_RATE),
            "initial_loop_activity_suggestion_seconds": detect_initial_loop_start(&playback, OUTPUT_RATE),
        },
        "exports": [decoded_export, playback_export],
        "limits": {
            "wet_native_owner_constructed": false, "accepted_musical_c": null,
            "independent_source_attack_frozen": false, "precise_loop_frames_frozen": false,
            "analysis_result_frozen": false, "historical_exact_reproduction": false,
            "vbr_or_bitrate_warning_proves_cause": false,
            "decoder_origin_equal_to_other_decoder_origin_assumed": false,
        },
    });
    let mut report_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join(OUTPUT_FILES[2]))
        .unwrap();
    serde_json::to_writer_pretty(&mut report_file, &report).unwrap();
    report_file.write_all(b"\n").unwrap();
    report_file.sync_all().unwrap();
    assert_eq!(file_sha256(&source).unwrap(), SOURCE_SHA256);
    println!(
        "recorded_source_geometry {}",
        serde_json::to_string(&report).unwrap()
    );
}
