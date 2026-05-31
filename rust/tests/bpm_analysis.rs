//! Integration tests for the BPM detection pipeline.
//!
//! Uses `tests/fixtures/test_120bpm.mp3` (known 120 BPM, ~31 s) as the test
//! fixture and exercises every stage of the qm-dsp pipeline:
//! DetectionFunction (ComplexSD), RCF / Viterbi beat period estimation,
//! DP beat tracking, and final BPM calculation.

use flitzis_looper_audio::AnalysisResult;

const TEST_FILE: &str = "tests/fixtures/test_120bpm.mp3";
const KNOWN_BPM: f32 = 120.0;

fn analyze_test_fixture() -> std::io::Result<AnalysisResult> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let path = std::path::Path::new(&manifest_dir).join(TEST_FILE);
    flitzis_looper_audio::analyze_audio_file(&path)
}

// ODF stage

#[test]
fn odf_produces_frames() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.odf_frame_count > 0,
        "ODF should produce frames, got {}",
        result.odf_frame_count
    );
}

#[test]
fn odf_frame_count_matches_fixture_duration() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    // Fixture is ~31 s. Frame step ≈ 0.01161 s → ~2 700 frames.
    // Allow generous bounds: 2 000–4 000.
    assert!(
        result.odf_frame_count >= 2_000 && result.odf_frame_count <= 4_000,
        "ODF frame count {} outside expected range [2000, 4000] for ~31 s fixture",
        result.odf_frame_count
    );
}

// RCF / Viterbi beat period stage

#[test]
fn beat_period_centers_near_expected() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");

    // At 120 BPM with step_secs=0.01161, beat period ≈ 0.5 / 0.01161 ≈ 43 frames.
    // Allow ±5 frames tolerance.
    let avg = result.avg_beat_period_frames;
    assert!(
        (38.0..=48.0).contains(&avg),
        "Average beat period {avg:.2} frames outside expected range [38, 48] for 120 BPM"
    );
}

#[test]
fn beat_period_is_positive() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.avg_beat_period_frames > 0.0,
        "Average beat period must be positive, got {}",
        result.avg_beat_period_frames
    );
}

// Beat tracking stage

#[test]
fn beat_count_matches_expected_density() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");

    // ~31 s at 120 BPM → ~62 beats. Allow ±50 % tolerance.
    let expected_min = 30;
    let expected_max = 100;
    assert!(
        result.beat_count >= expected_min && result.beat_count <= expected_max,
        "Beat count {} outside expected range [{}, {}]",
        result.beat_count,
        expected_min,
        expected_max
    );
}

#[test]
fn beats_are_strictly_increasing() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    for i in 1..result.beat_positions.len() {
        assert!(
            result.beat_positions[i] > result.beat_positions[i - 1],
            "Beats must be strictly increasing: beat[{i}]={:.3} <= beat[{}]={:.3}",
            result.beat_positions[i],
            i - 1,
            result.beat_positions[i - 1]
        );
    }
}

#[test]
fn beats_span_the_audio() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    let last_beat = result.beat_positions.last().copied().unwrap_or(0.0);
    // The fixture is ~31 s. The last beat should be well into the file.
    assert!(
        last_beat > 20.0,
        "Last beat at {:.1}s — should span most of the ~31 s fixture",
        last_beat
    );
}

// Final BPM calculation

#[test]
fn bpm_within_tolerance() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    let diff = (result.bpm - KNOWN_BPM).abs();
    assert!(
        diff <= 2.0,
        "BPM {:.2} is outside ±2 BPM of expected {KNOWN_BPM} (diff={diff:.2})",
        result.bpm
    );
}

#[test]
fn bpm_is_reasonable() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.bpm > 0.0 && result.bpm < 300.0,
        "BPM {:.2} is outside reasonable range (0, 300)",
        result.bpm
    );
}

// Downbeat detection

#[test]
fn downbeats_detected() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.downbeat_count > 0,
        "Expected at least one downbeat, got {}",
        result.downbeat_count
    );
}

#[test]
fn downbeats_are_subset_of_beats() {
    let result = analyze_test_fixture().expect("failed to decode test fixture");
    assert!(
        result.downbeat_count <= result.beat_count,
        "Downbeat count {} should not exceed beat count {}",
        result.downbeat_count,
        result.beat_count
    );
}
