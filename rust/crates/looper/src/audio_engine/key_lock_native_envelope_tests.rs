//! Bounded independent analytic inputs across the raw LiveShifter candidate envelope.
//!
//! Fixed source/rate/pitch/event declarations precede wet processing. Large silent
//! guards are external probe geometry, not productive H/D/C or chosen live policy.
//! All metrics retain full uncropped denominators; absent/incomplete responses have
//! no isolated retention. This finite generated family establishes no music acceptance.

use super::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, energy_times, stereo_energy, stereo_response,
};
use super::key_lock_physical_onset_tests::{
    PROFILES, RawNative, RawProbeContract, report_isolated_at,
};
use sha2::{Digest, Sha256};

const HISTORY_FRAMES: usize = 131_072;
const POSTROLL_FRAMES: usize = 65_536;
const FRAME_CAP: usize = 196_608;
const SOURCE_FRAME_CAP: usize = POSTROLL_FRAMES * 2 + 2;
const CASE_CAP: usize = 324;
const SAMPLE_RATES: [u32; 3] = [44_100, 48_000, 96_000];
const SOURCE_RATES: [f64; 3] = [0.5, 1.0, 2.0];
const SEMITONES: [i32; 3] = [-23, 0, 24];
const EVENT_PHASES: [usize; 3] = [0, 17, 511];

#[derive(Clone, Copy)]
enum Attack {
    DecayingKick,
    StereoTonalPercussive,
}

impl Attack {
    fn identity(self) -> &'static str {
        match self {
            Self::DecayingKick => "kick_80ms_80_to_42hz_18ms_decay_v1",
            Self::StereoTonalPercussive => "distinct_stereo_tonal_percussive_36ms_v1",
        }
    }

    fn support_end(self, sample_rate_hz: u32) -> usize {
        let milliseconds = match self {
            Self::DecayingKick => 80,
            Self::StereoTonalPercussive => 36,
        };
        (sample_rate_hz as usize * milliseconds).div_ceil(1000)
    }

    fn sample(self, frame: usize, channel: usize, sample_rate_hz: u32) -> f32 {
        let end = self.support_end(sample_rate_hz);
        if frame >= end {
            return 0.0;
        }
        let time = frame as f64 / f64::from(sample_rate_hz);
        let tau = std::f64::consts::TAU;
        let value = match self {
            Self::DecayingKick => {
                // Independent integrated source phase: f(t)=42+38*exp(-t/.012).
                let phase = tau * (42.0 * time + 38.0 * 0.012 * (1.0 - (-time / 0.012).exp()));
                let low = 0.52 * (-time / 0.018).exp() * phase.cos();
                let click = 0.07 * (-time / 0.0007).exp() * (tau * 2200.0 * time).cos();
                let level = if channel == 0 { 1.0 } else { 0.74 };
                level * (low + click)
            }
            Self::StereoTonalPercussive => {
                // Distinct noncancelling channel content, not a mono-folded reference.
                if channel == 0 {
                    0.28 * (-time / 0.010).exp()
                        * (tau * (330.0 * time + 1200.0 * time * time)).cos()
                        + 0.14 * (-time / 0.0012).exp() * (tau * 3200.0 * time).cos()
                } else {
                    0.24 * (-time / 0.014).exp()
                        * (tau * (523.25 * time + 780.0 * time * time) + 0.41).cos()
                        + 0.16 * (-time / 0.0016).exp() * (tau * 2300.0 * time + 0.2).cos()
                }
            }
        };
        // Fixed final 5ms source taper, declared before wet; no output window is cut.
        let taper_frames = sample_rate_hz as usize / 200;
        let taper_start = end - taper_frames;
        let taper = if frame < taper_start {
            1.0
        } else {
            0.5 * (1.0
                + (std::f64::consts::PI * (frame - taper_start) as f64 / (taper_frames - 1) as f64)
                    .cos())
        };
        (value * taper) as f32
    }
}

fn source(attack: Attack, sample_rate_hz: u32) -> [Vec<f32>; 2] {
    assert!(attack.support_end(sample_rate_hz) <= sample_rate_hz as usize * 80 / 1000);
    std::array::from_fn(|channel| {
        (0..SOURCE_FRAME_CAP)
            .map(|frame| attack.sample(frame, channel, sample_rate_hz))
            .collect()
    })
}

fn source_sha256(source: &[Vec<f32>; 2]) -> String {
    let mut hash = Sha256::new();
    for frame in 0..SOURCE_FRAME_CAP {
        for channel in source {
            hash.update(channel[frame].to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

fn physical_input(
    source: &[Vec<f32>; 2],
    source_rate: f64,
    source_zero_frame: usize,
) -> [Vec<f32>; 2] {
    assert_eq!(HISTORY_FRAMES + POSTROLL_FRAMES, FRAME_CAP);
    assert!(source_zero_frame < FRAME_CAP);
    assert!(
        source
            .iter()
            .all(|channel| channel.len() == SOURCE_FRAME_CAP)
    );
    // This guard includes the right interpolation tap. No wrap, future event or tail
    // outside the declared source extent can enter even at source rate 2.
    assert!(
        (FRAME_CAP - 1 - source_zero_frame) as f64 * source_rate + 1.0 < SOURCE_FRAME_CAP as f64
    );
    std::array::from_fn(|channel| {
        (0..FRAME_CAP)
            .map(|frame| {
                let signed = frame as i64 - source_zero_frame as i64;
                if signed < 0 {
                    return 0.0;
                }
                let coordinate = signed as f64 * source_rate;
                let left = coordinate.floor() as usize;
                let a = source[channel][left];
                let b = source[channel][left + 1];
                a + (b - a) * coordinate.fract() as f32
            })
            .collect()
    })
}

#[test]
fn raw_native_envelope_reports_fixed_isolation_and_joint_bounds_without_live_selection() {
    let mut cases = 0;
    println!(
        "native_envelope matrix source_types=2 sample_rates={SAMPLE_RATES:?} \
         source_rates={SOURCE_RATES:?} total_semitones={SEMITONES:?} \
         event_phases={EVENT_PHASES:?} profiles=2 expected_cases={CASE_CAP} \
         fixed_history={HISTORY_FRAMES} fixed_postroll={POSTROLL_FRAMES} \
         frame_cap_per_capture={FRAME_CAP} native_block_frames=512 \
         source_frame_cap={SOURCE_FRAME_CAP} accepted_musical_c=none"
    );
    for sample_rate_hz in SAMPLE_RATES {
        let tail_frames = sample_rate_hz as usize / 50;
        let tail_start = FRAME_CAP - tail_frames;
        for attack in [Attack::DecayingKick, Attack::StereoTonalPercussive] {
            let source = source(attack, sample_rate_hz);
            let source_sha256 = source_sha256(&source);
            let support_end = attack.support_end(sample_rate_hz);
            let source_gap = source.iter().all(|channel| {
                channel.iter().all(|sample| sample.is_finite())
                    && channel[support_end..].iter().all(|sample| *sample == 0.0)
            });
            assert!(source_gap);
            assert!(source.iter().all(|channel| channel[0] != 0.0));
            for source_rate in SOURCE_RATES {
                for phase in EVENT_PHASES {
                    let source_zero_frame = HISTORY_FRAMES + phase;
                    assert_eq!(source_zero_frame % 512, phase);
                    let dry = physical_input(&source, source_rate, source_zero_frame);
                    assert!(dry.iter().enumerate().all(|(channel, samples)| {
                        samples[..source_zero_frame]
                            .iter()
                            .all(|sample| *sample == 0.0)
                            && samples[source_zero_frame] == source[channel][0]
                    }));
                    let tail_source_start =
                        ((tail_start - source_zero_frame) as f64 * source_rate).floor() as usize;
                    assert!(tail_source_start >= support_end);
                    let dry_gap = dry
                        .iter()
                        .all(|channel| channel[tail_start..].iter().all(|sample| *sample == 0.0));
                    for semitones in SEMITONES {
                        let pitch = 2.0_f64.powf(f64::from(semitones) / 12.0) / source_rate;
                        let contract = RawProbeContract {
                            sample_rate_hz,
                            source_rate,
                            pitch,
                            source_zero_frame,
                        };
                        for profile in PROFILES {
                            cases += 1;
                            assert!(cases <= CASE_CAP);
                            println!(
                                "native_envelope source case={cases}/{CASE_CAP} identity={} \
                                 source_pcm_sha256={source_sha256} \
                                 source_format=stereo_interleaved_f32le source_sample_rate_hz={sample_rate_hz} \
                                 source_event_frame=0 declared_source_support=[0,{support_end}) \
                                 source_identity_frames={SOURCE_FRAME_CAP} negative_history=silence \
                                 total_semitones={semitones} source_rate={source_rate:.17} \
                                 pitch={pitch:.17} event_phase={phase} source_zero_frame={source_zero_frame} \
                                 capture_frames={FRAME_CAP} profile={} options=0x{:08x} \
                                 anchor_provenance=analytic_source_definition accepted_musical_c=none",
                                attack.identity(),
                                profile.name,
                                profile.options,
                            );
                            let (wet, nominal_delay) =
                                RawNative::capture_at_rate(&dry, sample_rate_hz, pitch, profile);
                            let energy = stereo_energy(&wet);
                            let total = energy.iter().sum::<f64>();
                            let has_response = total.is_finite() && total > 0.0;
                            let tail_fraction = has_response
                                .then(|| energy[tail_start..].iter().sum::<f64>() / total);
                            let capture_complete = tail_fraction
                                .is_some_and(|fraction| fraction <= MAX_CAPTURE_TAIL_ENERGY);
                            println!(
                                "native_envelope isolation case={cases} identity={} profile={} \
                                 contract={contract:?} source_silent_gap={source_gap} \
                                 source_tail_gap_start={tail_source_start} dry_final_20ms_silent={dry_gap} \
                                 full_uncropped_capture_frames={FRAME_CAP} raw_total_energy={total:.12e} \
                                 final_20ms_absolute=[{tail_start},{FRAME_CAP}) \
                                 raw_tail_fraction={tail_fraction:?} \
                                 raw_tail_limit={MAX_CAPTURE_TAIL_ENERGY:.12e} raw_tail_bound={capture_complete}",
                                attack.identity(),
                                profile.name,
                            );
                            if source_gap && dry_gap && capture_complete {
                                report_isolated_at(contract, profile, &dry, &wet, nominal_delay);
                            } else if has_response {
                                let response = stereo_response(&wet);
                                let relative =
                                    |frame: usize| frame as i64 - source_zero_frame as i64;
                                let raw_q = energy_times(&energy, sample_rate_hz)
                                    .map(|frame| frame - source_zero_frame as i64);
                                println!(
                                    "native_envelope unisolated_capture case={cases} nominal_delay={nominal_delay} \
                                     raw_onset={:?} raw_one_percent_onset={:?} raw_peak={} \
                                     whole_capture_q={raw_q:?} isolated_attack_retention=unavailable \
                                     joint_interval=unavailable accepted_musical_c=none",
                                    response.onset.map(relative),
                                    response.significant_onset.map(relative),
                                    relative(response.peak_frame),
                                );
                            } else {
                                println!(
                                    "native_envelope unavailable_response case={cases} nominal_delay={nominal_delay} \
                                     reason=no_finite_positive_response quantiles=unavailable \
                                     isolated_attack_retention=unavailable joint_interval=unavailable \
                                     accepted_musical_c=none"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        cases, CASE_CAP,
        "entire fixed diagnostic matrix was visited"
    );
    println!("native_envelope complete actual_cases={cases} acoustic_acceptance=unclaimed");
}
