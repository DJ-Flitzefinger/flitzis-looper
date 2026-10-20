//! Opt-in actual-file authored-pulse engineering, not a Human/music anchor certificate.
//!
//! The unchanged private WAV declares pulse zero before wet inspection. A source silent gap
//! and the unchanged capture-tail criterion must both hold before isolated retention is
//! reported. Missing isolation never turns whole-mixture energy into an attack denominator.

use super::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, energy_times, stereo_energy, stereo_response,
};
use super::key_lock_attack_reference::{
    AttackLagRange, SourceAttackReference, compare_attack_reference,
};
use super::key_lock_physical_onset_tests::{
    POSTROLL, PREROLL, PROFILES, RATES, RawNative, report_isolated,
};
use super::prepared_source::file_sha256;
use super::sample_loader::decode_audio_file_to_sample_buffer;
use super::source_reader::FrameRange;
use pyo3::prelude::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const RATE: u32 = 48_000;
const SOURCE_FRAMES: usize = 28_800_000;
const QUARTER: usize = 24_000;
const FRAMES: usize = PREROLL + POSTROLL;
const TAIL_FRAMES: usize = RATE as usize / 50;
const ORIGINAL_SHA256: &str = "96ffe98cf44215719b0b57d605d6dc586c9c4e763ad3d47d512d0ba787d204ef";

fn authored_source() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap()
        .canonicalize()
        .unwrap();
    let path = workspace
        .join("test-audio/metronom_120_BPM.wav")
        .canonicalize()
        .unwrap();
    assert!(path.starts_with(workspace.join("test-audio")));
    assert_eq!(file_sha256(&path).unwrap(), ORIGINAL_SHA256);
    Python::initialize();
    Python::attach(|py| {
        // Existing bounded verifier checks exact SHA, mono PCM24/48kHz/600s and every
        // one of the 1200 identical complete quarter blocks. It creates no project/files.
        let source_path = py
            .import("pathlib")
            .unwrap()
            .getattr("Path")
            .unwrap()
            .call1((path.to_str().unwrap(),))
            .unwrap();
        let reference = py
            .import("flitzis_looper.analysis.productive_loop_packet")
            .unwrap()
            .getattr("authored_quarter_reference")
            .unwrap()
            .call1((source_path,))
            .unwrap();
        assert_eq!(
            reference
                .getattr("source_sha256")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            ORIGINAL_SHA256
        );
        assert_eq!(
            reference
                .getattr("source_sample_rate_hz")
                .unwrap()
                .extract::<u32>()
                .unwrap(),
            RATE
        );
        assert_eq!(
            reference
                .getattr("source_frame_count")
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            SOURCE_FRAMES
        );
        assert_eq!(
            reference
                .getattr("reference_kind")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "authored_quarter_pulse_fixture"
        );
        assert_eq!(
            reference
                .getattr("origin_seconds")
                .unwrap()
                .extract::<f64>()
                .unwrap(),
            0.0
        );
        let features = reference
            .getattr("feature_source_frames")
            .unwrap()
            .extract::<Vec<usize>>()
            .unwrap();
        assert_eq!(features.len(), 1200);
        assert!(
            features
                .iter()
                .enumerate()
                .all(|(index, frame)| *frame == index * QUARTER)
        );
        // Its separate 50ms raw-QM lookup allowance is not this probe's 2ms B5 criterion.
    });
    path
}

fn physical_input(source: &[f32], rate: f64) -> [Vec<f32>; 2] {
    assert_eq!(source.len(), SOURCE_FRAMES);
    // The next independently authored pulse cannot enter either interpolation tap.
    assert!((POSTROLL as f64 - 1.0) * rate + 1.0 < QUARTER as f64);
    let mono = (0..FRAMES)
        .map(|frame| {
            let signed = frame as i64 - PREROLL as i64;
            if signed < 0 {
                return 0.0;
            }
            let coordinate = signed as f64 * rate;
            let left = coordinate.floor() as usize;
            let a = source[left];
            let b = source[left + 1];
            a + (b - a) * coordinate.fract() as f32
        })
        .collect::<Vec<_>>();
    assert_eq!(mono[PREROLL], source[0]);
    // Explicit dual-mono representation of the verified mono source, not an original
    // stereo/channel-cancellation claim. Both channels enter one ChannelsTogether owner.
    [mono.clone(), mono]
}

#[test]
#[ignore = "explicit private authored WAV engineering; needs repository Python imports and unchanged workspace test-audio/metronom_120_BPM.wav"]
fn private_authored_pulse_compares_raw_short_medium_with_external_zero_reference() {
    let path = authored_source();
    let decoded = decode_audio_file_to_sample_buffer(&path, 1, RATE, |_| {}).unwrap();
    assert_eq!(decoded.channels, 1);
    assert_eq!(decoded.samples.len(), SOURCE_FRAMES);
    assert!(decoded.samples.iter().all(|sample| sample.is_finite()));
    let pcm_sha256 = format!(
        "{:x}",
        decoded
            .samples
            .iter()
            .fold(Sha256::new(), |mut hash, sample| {
                hash.update(sample.to_le_bytes());
                hash
            })
            .finalize()
    );
    let quarter = &decoded.samples[..QUARTER];
    // Recorded fixture documentation independently declares the first pulse peak at zero.
    let peak = quarter
        .iter()
        .enumerate()
        .fold((0, 0.0_f32), |best, (frame, value)| {
            if value.abs() > best.1 {
                (frame, value.abs())
            } else {
                best
            }
        });
    assert_eq!(peak.0, 0);
    assert!(peak.1 >= 0.05);
    println!(
        "authored_onset source_sha256={ORIGINAL_SHA256} decoded_pcm_sha256={pcm_sha256} \
         original_format=mono_pcm24_48000_600s decoded_format=mono_f32_48000 \
         raw_channels=explicit_dual_mono source_event_frame=0 quarter_frames={QUARTER} \
         anchor_provenance=independently_authored_pulse human_musical_c=none"
    );
    for rate in RATES {
        let dry = physical_input(&decoded.samples, rate);
        // Freeze source event/support/search before constructing any wet owner.
        let reference = SourceAttackReference {
            frame: PREROLL,
            support: FrameRange {
                start: PREROLL - 4096,
                end: PREROLL + 8192,
            },
        };
        let search = AttackLagRange {
            min: -8192,
            max: 8192,
        };
        let source_gap_start = ((POSTROLL - TAIL_FRAMES) as f64 * rate).floor() as usize;
        let source_gap = quarter[source_gap_start..]
            .iter()
            .all(|sample| *sample == 0.0);
        let dry_gap = dry.iter().all(|channel| {
            channel[FRAMES - TAIL_FRAMES..]
                .iter()
                .all(|sample| *sample == 0.0)
        });
        for profile in PROFILES {
            let (wet, nominal_delay) = RawNative::capture(&dry, 1.0 / rate, profile);
            let correspondence = compare_attack_reference(&dry, &wet, RATE, reference, search);
            println!(
                "authored_onset correspondence profile={name} options=0x{options:08x} \
                 rate={rate:.17} source_frame=0 reference={reference:?} search={search:?} \
                 result={correspondence:?} feature_support_is_not_sample_precision=true \
                 accepted_musical_c=none",
                name = profile.name,
                options = profile.options,
            );
            let energy = stereo_energy(&wet);
            let total = energy.iter().sum::<f64>();
            assert!(total.is_finite() && total > 0.0);
            let tail_fraction = energy[FRAMES - TAIL_FRAMES..].iter().sum::<f64>() / total;
            let capture_complete = tail_fraction <= MAX_CAPTURE_TAIL_ENERGY;
            println!(
                "authored_onset isolation profile={name} rate={rate:.17} \
                 source_gap_start={source_gap_start} source_silent_gap={source_gap} \
                 dry_silent_gap={dry_gap} raw_tail_fraction={tail_fraction:.12e} \
                 raw_tail_bound={capture_complete} next_source_pulse_frame={QUARTER}",
                name = profile.name,
            );
            if source_gap && dry_gap && capture_complete {
                // Same uncropped response denominator, peak/tail and 2ms/.1% joint bounds
                // as the synthetic probe. No prefix, fitted offset or quantile becomes C.
                report_isolated(profile, rate, &dry, &wet, nominal_delay);
            } else {
                let response = stereo_response(&wet);
                let raw_q = energy_times(&energy, RATE).map(|frame| frame - PREROLL as i64);
                let relative = |frame: usize| frame as i64 - PREROLL as i64;
                println!(
                    "authored_onset unisolated_capture profile={name} rate={rate:.17} \
                     nominal_delay={nominal_delay} raw_onset={:?} raw_one_percent_onset={:?} \
                     raw_peak={} whole_capture_q={raw_q:?} \
                     isolated_attack_retention=unavailable joint_interval=unavailable \
                     accepted_musical_c=none",
                    response.onset.map(relative),
                    response.significant_onset.map(relative),
                    relative(response.peak_frame),
                    name = profile.name,
                );
            }
        }
    }
    assert_eq!(
        file_sha256(&path).unwrap(),
        ORIGINAL_SHA256,
        "private original remains unchanged"
    );
}
