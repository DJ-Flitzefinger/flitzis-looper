//! Fixed neighboring attacks for raw Short/Medium diagnostics, not music acceptance.
//!
//! Source event zero and finite source support are declared before any wet owner.
//! Physical negative coordinates are silence. Neither Native delay nor a measured
//! peak/quantile chooses a boundary, and incomplete captures have no isolated retention.

use super::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, energy_times, stereo_energy, stereo_response,
};
use super::key_lock_physical_onset_tests::{
    POSTROLL, PREROLL, PROFILES, RATES, RawNative, report_isolated,
};
use sha2::{Digest, Sha256};

const RATE: u32 = 48_000;
const FRAMES: usize = PREROLL + POSTROLL;
const TAIL_FRAMES: usize = RATE as usize / 50;
const SOURCE_GUARD: usize = 24_000;

#[derive(Clone, Copy)]
enum Attack {
    Burst900Hz,
    DecayingKick,
    StereoTonalPercussive,
}

impl Attack {
    fn kind(self) -> &'static str {
        match self {
            Self::Burst900Hz => "windowed_tonal_burst",
            Self::DecayingKick => "low_frequency_decaying_kick",
            Self::StereoTonalPercussive => "distinct_stereo_tonal_percussive",
        }
    }

    fn identity(self) -> &'static str {
        match self {
            Self::Burst900Hz => "cosine_900hz_8ms_half_cosine_envelope_v1",
            Self::DecayingKick => "kick_80ms_80_to_42hz_18ms_decay_v1",
            Self::StereoTonalPercussive => "distinct_stereo_tonal_percussive_36ms_v1",
        }
    }

    fn support_end(self) -> usize {
        match self {
            Self::Burst900Hz => 384,
            Self::DecayingKick => 3840,
            Self::StereoTonalPercussive => 1728,
        }
    }

    fn sample(self, frame: usize, channel: usize) -> f32 {
        let end = self.support_end();
        if frame >= end {
            return 0.0;
        }
        let time = frame as f64 / f64::from(RATE);
        let tau = std::f64::consts::TAU;
        let value = match self {
            Self::Burst900Hz => {
                // Exactly 384 source frames; the causal first sample is nonzero.
                let envelope =
                    0.5 * (1.0 + (std::f64::consts::PI * frame as f64 / (end - 1) as f64).cos());
                let level = if channel == 0 { 0.35 } else { 0.23 };
                level * envelope * (tau * 900.0 * time).cos()
            }
            Self::DecayingKick => {
                // Analytic phase integrates f(t)=42+38*exp(-t/.012), no wet estimate.
                let phase = tau * (42.0 * time + 38.0 * 0.012 * (1.0 - (-time / 0.012).exp()));
                let low = 0.52 * (-time / 0.018).exp() * phase.cos();
                let click = 0.07 * (-time / 0.0007).exp() * (tau * 2200.0 * time).cos();
                let level = if channel == 0 { 1.0 } else { 0.74 };
                level * (low + click) * terminal_taper(frame, end)
            }
            Self::StereoTonalPercussive => {
                // Different tones, chirps, phases and percussive decays in each channel;
                // this is neither dual mono nor a cancelling opposite-polarity pair.
                let (tone, percussion) = if channel == 0 {
                    (
                        0.28 * (-time / 0.010).exp()
                            * (tau * (330.0 * time + 1200.0 * time * time)).cos(),
                        0.14 * (-time / 0.0012).exp() * (tau * 3200.0 * time).cos(),
                    )
                } else {
                    (
                        0.24 * (-time / 0.014).exp()
                            * (tau * (523.25 * time + 780.0 * time * time) + 0.41).cos(),
                        0.16 * (-time / 0.0016).exp() * (tau * 2300.0 * time + 0.2).cos(),
                    )
                };
                (tone + percussion) * terminal_taper(frame, end)
            }
        };
        value as f32
    }
}

fn terminal_taper(frame: usize, end: usize) -> f64 {
    // The last 5ms taper is part of the declared source, never a wet-output crop.
    let taper_frames = RATE as usize / 200;
    let start = end - taper_frames;
    if frame < start {
        1.0
    } else {
        0.5 * (1.0
            + (std::f64::consts::PI * (frame - start) as f64 / (taper_frames - 1) as f64).cos())
    }
}

fn source(attack: Attack) -> [Vec<f32>; 2] {
    std::array::from_fn(|channel| {
        (0..SOURCE_GUARD)
            .map(|frame| attack.sample(frame, channel))
            .collect()
    })
}

fn source_sha256(source: &[Vec<f32>; 2]) -> String {
    let mut hash = Sha256::new();
    // Identity is all 24000 declared source frames, stereo interleaved f32 little-endian.
    for frame in 0..SOURCE_GUARD {
        for channel in source {
            hash.update(channel[frame].to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

fn physical_input(source: &[Vec<f32>; 2], rate: f64) -> [Vec<f32>; 2] {
    // Even the right interpolation tap cannot address a later event at this guard.
    assert!((POSTROLL as f64 - 1.0) * rate + 1.0 < SOURCE_GUARD as f64);
    std::array::from_fn(|channel| {
        (0..FRAMES)
            .map(|frame| {
                let signed = frame as i64 - PREROLL as i64;
                if signed < 0 {
                    return 0.0;
                }
                let coordinate = signed as f64 * rate;
                let left = coordinate.floor() as usize;
                let a = source[channel][left];
                let b = source[channel][left + 1];
                a + (b - a) * coordinate.fract() as f32
            })
            .collect()
    })
}

#[test]
fn isolated_tonal_attacks_qualify_raw_short_medium_without_selecting_a_musical_boundary() {
    for attack in [
        Attack::Burst900Hz,
        Attack::DecayingKick,
        Attack::StereoTonalPercussive,
    ] {
        let source = source(attack);
        let source_sha256 = source_sha256(&source);
        let support_end = attack.support_end();
        let source_gap = source.iter().all(|channel| {
            channel.iter().all(|sample| sample.is_finite())
                && channel[support_end..].iter().all(|sample| *sample == 0.0)
        });
        assert!(source_gap);
        assert!(source.iter().all(|channel| channel[0] != 0.0));
        if matches!(attack, Attack::StereoTonalPercussive) {
            assert_ne!(source[0], source[1]);
            assert!(
                source[0][..support_end]
                    .iter()
                    .zip(&source[1][..support_end])
                    .any(|(left, right)| (*left + *right).abs() > 0.01)
            );
        }
        for rate in RATES {
            let dry = physical_input(&source, rate);
            assert!(
                dry.iter()
                    .all(|channel| { channel[..PREROLL].iter().all(|sample| *sample == 0.0) })
            );
            assert!((POSTROLL - TAIL_FRAMES) as f64 * rate >= support_end as f64);
            let dry_gap = dry.iter().all(|channel| {
                channel[FRAMES - TAIL_FRAMES..]
                    .iter()
                    .all(|sample| *sample == 0.0)
            });
            for profile in PROFILES {
                println!(
                    "tonal_onset source identity={} source_type={} source_pcm_sha256={source_sha256} \
                     source_format=stereo_interleaved_f32le_48000 source_identity_frames={SOURCE_GUARD} \
                     source_event_frame=0 declared_source_support=[0,{support_end}) \
                     future_source_guard={SOURCE_GUARD} negative_history=silence \
                     capture_signed=[-{PREROLL},{POSTROLL}) profile={} options=0x{:08x} \
                     rate={rate:.17} pitch={:.17} anchor_provenance=analytic_source_definition \
                     accepted_musical_c=none",
                    attack.identity(),
                    attack.kind(),
                    profile.name,
                    profile.options,
                    1.0 / rate,
                );
                let (wet, nominal_delay) = RawNative::capture(&dry, 1.0 / rate, profile);
                let energy = stereo_energy(&wet);
                let total = energy.iter().sum::<f64>();
                assert!(total.is_finite() && total > 0.0);
                let tail_fraction = energy[FRAMES - TAIL_FRAMES..].iter().sum::<f64>() / total;
                let capture_complete = tail_fraction <= MAX_CAPTURE_TAIL_ENERGY;
                println!(
                    "tonal_onset isolation identity={} profile={} rate={rate:.17} \
                     source_silent_gap={source_gap} dry_final_20ms_silent={dry_gap} \
                     full_uncropped_capture_frames={FRAMES} raw_total_energy={total:.12e} \
                     final_20ms_signed=[{},{POSTROLL}) raw_tail_fraction={tail_fraction:.12e} \
                     raw_tail_limit={MAX_CAPTURE_TAIL_ENERGY:.12e} raw_tail_bound={capture_complete}",
                    attack.identity(),
                    profile.name,
                    POSTROLL - TAIL_FRAMES,
                );
                if source_gap && dry_gap && capture_complete {
                    // Unchanged original full-capture denominators and 2ms/.1%/peak/tail
                    // joint diagnostic. No predicate is asserted as acoustic acceptance.
                    report_isolated(profile, rate, &dry, &wet, nominal_delay);
                } else {
                    let response = stereo_response(&wet);
                    let relative = |frame: usize| frame as i64 - PREROLL as i64;
                    let raw_q = energy_times(&energy, RATE).map(|frame| frame - PREROLL as i64);
                    println!(
                        "tonal_onset unisolated_capture identity={} profile={} rate={rate:.17} \
                         nominal_delay={nominal_delay} raw_onset={:?} raw_one_percent_onset={:?} \
                         raw_peak={} whole_capture_q={raw_q:?} isolated_attack_retention=unavailable \
                         joint_interval=unavailable accepted_musical_c=none",
                        attack.identity(),
                        profile.name,
                        response.onset.map(relative),
                        response.significant_onset.map(relative),
                        relative(response.peak_frame),
                    );
                }
            }
        }
    }
}
