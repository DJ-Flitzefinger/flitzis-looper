//! Independent signed physical-source input and uncropped direct-C-ABI Native output.
//!
//! The remote tail never enters the physical positive capture. Wrapped prehistory is a
//! separate hypothetical input, not actual previously played audio. Whole-mixture differences
//! diagnose context sensitivity; neither they nor a raw peak select a musical anchor C.
//! No productive history bound, discard, source reader, adapter or FIFO supplies this oracle.

use super::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, MAX_DISCARDED_ENERGY, TIMING_BUDGET_MS, energy_times,
    maximum_energy_discard, stereo_energy, stereo_response,
};
use super::rubberband_backend::RubberBandLiveStateOpaque;
use std::ptr::NonNull;

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const BLOCK: usize = 512;
pub(super) const PREROLL: usize = 32_768;
pub(super) const POSTROLL: usize = 16_384;
const FRAMES: usize = PREROLL + POSTROLL;
const LOOP_FRAMES: usize = 48_000;
const TAIL_START: usize = 40_000;
const ATTACK_FRAMES: usize = 192;
const WINDOW_START: usize = PREROLL - 8_192;
pub(super) const RATES: [f64; 3] = [4.0 / 5.0, 1.0, 29.0 / 25.0];

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub(super) name: &'static str,
    pub(super) options: i32,
}

// Pinned v4.0.0 RubberBandLiveShifter.h / rubberband-c.h Live enum, not Stretcher flags:
// WindowShort=0, WindowMedium=0x00100000, ChannelsTogether=0x10000000.
pub(super) const PROFILES: [Profile; 2] = [
    Profile {
        name: "current_short",
        options: 0x1000_0000,
    },
    Profile {
        name: "candidate_medium",
        options: 0x1010_0000,
    },
];

unsafe extern "C" {
    #[link_name = "rubberband_live_new"]
    fn raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
    #[link_name = "rubberband_live_delete"]
    fn raw_delete(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_set_pitch_scale"]
    fn raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
    #[link_name = "rubberband_live_reset"]
    fn raw_reset(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_get_block_size"]
    fn raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_channel_count"]
    fn raw_channels(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_start_delay"]
    fn raw_delay(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_shift"]
    fn raw_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const f32,
        output: *const *mut f32,
    );
}

pub(super) struct RawNative(NonNull<RubberBandLiveStateOpaque>);

impl RawNative {
    fn new(sample_rate_hz: u32, pitch: f64, profile: Profile) -> Self {
        // Each profile owns a fresh exact-pitch/reset handle; no state crosses profiles.
        assert!(sample_rate_hz > 0);
        assert!(pitch.is_finite() && pitch > 0.0);
        let state =
            NonNull::new(unsafe { raw_new(sample_rate_hz, CHANNELS as u32, profile.options) })
                .expect("independent raw Native construction");
        let raw = Self(state);
        unsafe {
            assert_eq!(raw_block(state.as_ptr()) as usize, BLOCK);
            assert_eq!(raw_channels(state.as_ptr()) as usize, CHANNELS);
            raw_pitch(state.as_ptr(), pitch);
            raw_reset(state.as_ptr());
        }
        raw
    }

    pub(super) fn capture(
        input: &[Vec<f32>; CHANNELS],
        pitch: f64,
        profile: Profile,
    ) -> ([Vec<f32>; CHANNELS], u32) {
        // Preserve the original fixed-48k wrapper's geometry and accepted pitch domain.
        // The delegated constructor/512-frame loop retains the same C-ABI call order.
        assert!(input.iter().all(|channel| channel.len() == FRAMES));
        assert!((0.5..=2.0).contains(&pitch));
        Self::capture_at_rate(input, RATE, pitch, profile)
    }

    pub(super) fn capture_at_rate(
        input: &[Vec<f32>; CHANNELS],
        sample_rate_hz: u32,
        pitch: f64,
        profile: Profile,
    ) -> ([Vec<f32>; CHANNELS], u32) {
        let frames = input[0].len();
        assert!(frames > 0 && frames.is_multiple_of(BLOCK));
        assert!(input.iter().all(|channel| channel.len() == frames));
        let raw = Self::new(sample_rate_hz, pitch, profile);
        let nominal_delay = unsafe { raw_delay(raw.0.as_ptr()) };
        let mut captured = std::array::from_fn(|_| Vec::with_capacity(frames));
        for start in (0..frames).step_by(BLOCK) {
            let block: [[f32; BLOCK]; CHANNELS] = std::array::from_fn(|channel| {
                std::array::from_fn(|offset| input[channel][start + offset])
            });
            let input_ptrs = [block[0].as_ptr(), block[1].as_ptr()];
            let mut output = [[0.0; BLOCK]; CHANNELS];
            let output_ptrs = [output[0].as_mut_ptr(), output[1].as_mut_ptr()];
            unsafe { raw_shift(raw.0.as_ptr(), input_ptrs.as_ptr(), output_ptrs.as_ptr()) };
            for (captured, output) in captured.iter_mut().zip(output) {
                assert!(output.iter().all(|sample| sample.is_finite()));
                captured.extend_from_slice(&output);
            }
        }
        (captured, nominal_delay)
    }
}

impl Drop for RawNative {
    fn drop(&mut self) {
        unsafe { raw_delete(self.0.as_ptr()) };
    }
}

/// A bounded profile comparison, not a production latency or musical-quality gate.
#[test]
#[ignore = "explicit uncropped Short/Medium attack assessment; no runtime cutover"]
fn practical_attack_profile_assessment() {
    fn landmarks(input: &[Vec<f32>; CHANNELS], rate: u32) -> [usize; 3] {
        let magnitude = input[0]
            .iter()
            .zip(&input[1])
            .map(|(a, b)| a.abs().max(b.abs()))
            .collect::<Vec<_>>();
        let peak = magnitude.iter().copied().fold(0.0_f32, f32::max);
        assert!(peak > 1.0e-7);
        let energy = stereo_energy(input);
        let width = (rate as usize / 1000).max(1);
        let mut sum = energy[..width].iter().sum::<f64>();
        let mut maximum = (sum, 0);
        for end in width..energy.len() {
            sum += energy[end] - energy[end - width];
            if sum > maximum.0 {
                maximum = (sum, end + 1 - width);
            }
        }
        [
            magnitude.iter().position(|x| *x > 1.0e-7).unwrap(),
            magnitude.iter().position(|x| *x >= peak * 0.1).unwrap(),
            maximum.1 + width / 2,
        ]
    }

    for rate in [44_100, 48_000, 96_000] {
        for ratio in RATES {
            for silence in [0, rate as usize / 50] {
                // This source is declared independently of Native or its nominal delay.
                // Interpolate integer source samples; never borrow a positive tap before
                // physical zero. The second case retains 20 ms of authored source silence.
                let frames = (PREROLL + rate as usize / 2).div_ceil(BLOCK) * BLOCK;
                let source = |frame: usize| {
                    let Some(age) = frame.checked_sub(silence) else {
                        return 0.0_f32;
                    };
                    let seconds = age as f64 / f64::from(rate);
                    (0.4 * (-seconds / 0.004).exp()
                        * (std::f64::consts::TAU * 1000.0 * seconds).sin())
                        as f32
                };
                let dry: [Vec<f32>; CHANNELS] = std::array::from_fn(|channel| {
                    (0..frames)
                        .map(|frame| {
                            let Some(age) = frame.checked_sub(PREROLL) else {
                                return 0.0;
                            };
                            let physical = age as f64 * ratio;
                            let left = physical.floor() as usize;
                            let a = source(left);
                            let b = source(left + 1);
                            let value = a + (b - a) * physical.fract() as f32;
                            value * if channel == 0 { 1.0 } else { -0.73 }
                        })
                        .collect()
                });
                assert!(
                    dry.iter()
                        .all(|channel| channel[..PREROLL].iter().all(|x| *x == 0.0))
                );
                let dry_marks = landmarks(&dry, rate).map(|x| x - PREROLL);
                for profile in PROFILES {
                    let (wet, delay) = RawNative::capture_at_rate(&dry, rate, 1.0 / ratio, profile);
                    let marks = landmarks(&wet, rate);
                    let energy = stereo_energy(&wet);
                    let total = energy.iter().sum::<f64>();
                    assert!(total.is_finite() && total > 0.0);
                    let cut = PREROLL + delay as usize;
                    assert!(cut < frames);
                    let nominal_discard_fraction = energy[..cut].iter().sum::<f64>() / total;
                    let tail_start = frames - rate as usize / 50;
                    let tail_fraction = energy[tail_start..].iter().sum::<f64>() / total;
                    println!(
                        "practical_attack profile={} sr={rate} ratio={ratio:.17} source_silence={silence} \
                         native_block={BLOCK} nominal_delay={delay} dry_first_sig10_principal={dry_marks:?} \
                         raw_first_sig10_principal={:?} nominal_discard_fraction={nominal_discard_fraction:.12e} \
                         nominal_discards_principal={} tail_fraction={tail_fraction:.12e} \
                         frames={frames} n=1 uncropped no_production_E_or_readiness_or_quality_claim",
                        profile.name,
                        marks.map(|x| x as i64 - PREROLL as i64),
                        marks[2] < cut,
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum History {
    Physical,
    Wrapped,
}

fn source(frame: usize, channel: usize, remote_tail: bool) -> f32 {
    if frame < ATTACK_FRAMES {
        // The unique event and its exact first physical sample are declared before Native.
        let attack = 0.2 + frame as f32 / 2048.0;
        return attack * if channel == 0 { 1.0 } else { -0.73 };
    }
    if remote_tail && (TAIL_START..LOOP_FRAMES).contains(&frame) {
        let time = (frame - TAIL_START) as f64 / f64::from(RATE);
        let phase = channel as f64 * 0.83;
        return (0.025 * (std::f64::consts::TAU * 223.0 * time + phase).sin()
            + 0.017 * (std::f64::consts::TAU * 947.0 * time + phase + 0.4).cos())
            as f32;
    }
    0.0
}

fn input(rate: f64, history: History, remote_tail: bool) -> [Vec<f32>; CHANNELS] {
    assert!((POSTROLL as f64 - 1.0) * rate + 1.0 < TAIL_START as f64);
    std::array::from_fn(|channel| {
        (0..FRAMES)
            .map(|frame| {
                let signed_output = frame as i64 - PREROLL as i64;
                let physical = signed_output as f64 * rate;
                let coordinate = match history {
                    // A pre-source output frame is silence; it cannot borrow a first-sample
                    // interpolation tap from physical zero. No pre-start source was played.
                    History::Physical if physical < 0.0 => return 0.0,
                    History::Physical => physical,
                    History::Wrapped => physical.rem_euclid(LOOP_FRAMES as f64),
                };
                let left = coordinate.floor() as usize;
                let right = match history {
                    History::Physical => left + 1,
                    History::Wrapped => (left + 1) % LOOP_FRAMES,
                };
                let a = source(left, channel, remote_tail);
                let b = source(right, channel, remote_tail);
                a + (b - a) * coordinate.fract() as f32
            })
            .collect()
    })
}

fn assert_same(
    left: &[Vec<f32>; CHANNELS],
    right: &[Vec<f32>; CHANNELS],
    start: usize,
    context: &str,
) {
    let first = (0..CHANNELS).find_map(|channel| {
        (start..FRAMES)
            .find(|&frame| left[channel][frame] != right[channel][frame])
            .map(|frame| (channel, frame, left[channel][frame], right[channel][frame]))
    });
    assert_eq!(first, None, "{context}");
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RawProbeContract {
    pub(super) sample_rate_hz: u32,
    pub(super) source_rate: f64,
    pub(super) pitch: f64,
    pub(super) source_zero_frame: usize,
}

pub(super) fn report_isolated(
    profile: Profile,
    rate: f64,
    dry: &[Vec<f32>; CHANNELS],
    wet: &[Vec<f32>; CHANNELS],
    delay: u32,
) {
    assert!(dry.iter().chain(wet).all(|channel| channel.len() == FRAMES));
    report_isolated_at(
        RawProbeContract {
            sample_rate_hz: RATE,
            source_rate: rate,
            pitch: 1.0 / rate,
            source_zero_frame: PREROLL,
        },
        profile,
        dry,
        wet,
        delay,
    );
}

pub(super) fn report_isolated_at(
    contract: RawProbeContract,
    profile: Profile,
    dry: &[Vec<f32>; CHANNELS],
    wet: &[Vec<f32>; CHANNELS],
    delay: u32,
) {
    let RawProbeContract {
        sample_rate_hz,
        source_rate: rate,
        pitch,
        source_zero_frame,
    } = contract;
    let frames = dry[0].len();
    assert!(sample_rate_hz > 0);
    assert!(rate.is_finite() && rate > 0.0 && pitch.is_finite() && pitch > 0.0);
    assert!(dry.iter().chain(wet).all(|channel| channel.len() == frames));
    assert!(source_zero_frame < frames);
    let tail_frames = sample_rate_hz as usize / 50;
    assert!(tail_frames > 0 && tail_frames <= frames);
    let dry_energy = stereo_energy(dry);
    let raw_energy = stereo_energy(wet);
    let dry_times = energy_times(&dry_energy, sample_rate_hz);
    let raw_times = energy_times(&raw_energy, sample_rate_hz);
    let errors = std::array::from_fn::<_, 3, _>(|index| raw_times[index] - dry_times[index]);
    let raw = stereo_response(wet);
    let total = raw_energy.iter().sum::<f64>();
    assert!(total.is_finite() && total > 0.0);
    let prefix_fraction = raw_energy[..source_zero_frame].iter().sum::<f64>() / total;
    let tail_fraction = raw_energy[frames - tail_frames..].iter().sum::<f64>() / total;
    let relative = |frame: usize| frame as i64 - source_zero_frame as i64;
    let timing_budget = f64::from(sample_rate_hz) * TIMING_BUDGET_MS / 1000.0;
    let timing_pass = errors[..2]
        .iter()
        .all(|error| (*error as f64).abs() <= timing_budget);
    // Integer translations must obey the unchanged exact 2ms bound: at 44.1kHz,
    // 88 frames are eligible (88.2-frame budget), not 89. At 48/96kHz this is exact.
    let budget_frames = timing_budget.floor() as i64;
    let timing_lower = errors[0].max(errors[1]) - budget_frames;
    let timing_upper = errors[0].min(errors[1]) + budget_frames;
    let energy_boundary = maximum_energy_discard(&raw_energy);
    let retention_upper = relative(raw.peak_frame.min(energy_boundary));
    let joint_upper = timing_upper.min(retention_upper);
    let timing_interval_exists = timing_lower <= timing_upper;
    let joint_interval_exists =
        timing_lower <= joint_upper && tail_fraction <= MAX_CAPTURE_TAIL_ENERGY;
    let retention_gap = (timing_lower - retention_upper).max(0);
    // If q10/q50 already have no common translation, earlier emission cannot repair their
    // spread. Otherwise this is only a necessary lead bound for this isolated fixture.
    let necessary_lead = timing_interval_exists.then_some(retention_gap);
    let necessary_lead_ms =
        necessary_lead.map(|frames| frames as f64 * 1000.0 / f64::from(sample_rate_hz));
    // These are uncropped diagnostic predicates for the explicitly declared u=0 boundary.
    // No waveform is cut, no offset is fitted, and no predicate is asserted as musical PASS.
    println!(
        "physical_onset contract profile={} sample_rate_hz={sample_rate_hz} \
         source_rate={rate:.17} pitch={pitch:.17} source_zero_frame={source_zero_frame} \
         capture_frames={frames} contract={contract:?}",
        profile.name,
    );
    println!(
        "physical_onset isolated profile={name} options=0x{options:08x} \
         rate={rate:.17} pitch={:.17} nominal_delay={delay} \
         dry_q={:?} raw_onset={:?} raw_one_percent_onset={:?} raw_peak={} raw_q={:?} \
         q_errors={errors:?} timing_2ms={timing_pass} prefix_fraction={prefix_fraction:.12e} \
         prefix_0_1percent={} peak_after_zero={} tail_fraction={tail_fraction:.12e} tail_bound={} \
         latest_retaining_boundary={} timing_c_interval=[{timing_lower},{timing_upper}] \
         retention_u={retention_upper} joint_c_interval=[{timing_lower},{joint_upper}] \
         timing_interval_exists={timing_interval_exists} joint_interval_exists={joint_interval_exists} \
         retention_gap_frames={retention_gap} necessary_lead_frames={necessary_lead:?} \
         necessary_lead_ms={necessary_lead_ms:?} accepted_musical_c=none",
        pitch,
        dry_times.map(|frame| frame - source_zero_frame as i64),
        raw.onset.map(relative),
        raw.significant_onset.map(relative),
        relative(raw.peak_frame),
        raw_times.map(|frame| frame - source_zero_frame as i64),
        prefix_fraction <= MAX_DISCARDED_ENERGY,
        raw.peak_frame >= source_zero_frame,
        tail_fraction <= MAX_CAPTURE_TAIL_ENERGY,
        relative(energy_boundary),
        name = profile.name,
        options = profile.options,
    );
}

#[test]
fn signed_physical_beginning_cannot_depend_on_an_unread_future_tail() {
    for (profile, rate) in PROFILES
        .into_iter()
        .flat_map(|profile| RATES.into_iter().map(move |rate| (profile, rate)))
    {
        let dry_a = input(rate, History::Physical, false);
        let dry_b = input(rate, History::Physical, true);
        assert_same(
            &dry_a,
            &dry_b,
            0,
            "the remote physical tail was never addressed",
        );
        assert!(
            dry_a
                .iter()
                .all(|channel| channel[..PREROLL].iter().all(|x| *x == 0.0))
        );
        assert_eq!(dry_a[0][PREROLL], 0.2);
        let (raw_a, delay_a) = RawNative::capture(&dry_a, 1.0 / rate, profile);
        let (raw_b, delay_b) = RawNative::capture(&dry_b, 1.0 / rate, profile);
        assert_eq!(delay_a, delay_b);
        assert_same(
            &raw_a,
            &raw_b,
            0,
            "unread future data cannot change direct Native output",
        );
        report_isolated(profile, rate, &dry_a, &raw_a, delay_a);
    }
}

#[test]
fn wrapped_future_tail_reports_fixed_window_context_sensitivity_without_an_attack_decomposition() {
    for (profile, rate) in PROFILES
        .into_iter()
        .flat_map(|profile| RATES.into_iter().map(move |rate| (profile, rate)))
    {
        let physical = input(rate, History::Physical, false);
        let wrapped_a = input(rate, History::Wrapped, false);
        let wrapped_b = input(rate, History::Wrapped, true);
        assert_same(
            &wrapped_a,
            &physical,
            PREROLL,
            "the positive input remains physical",
        );
        assert_same(
            &wrapped_b,
            &physical,
            PREROLL,
            "the positive input remains physical",
        );
        assert!(
            (0..CHANNELS)
                .any(|channel| wrapped_a[channel][..PREROLL] != wrapped_b[channel][..PREROLL]),
            "only wrapped negative input reads the remote tail"
        );
        let (raw_a, delay_a) = RawNative::capture(&wrapped_a, 1.0 / rate, profile);
        let (raw_b, delay_b) = RawNative::capture(&wrapped_b, 1.0 / rate, profile);
        assert_eq!(delay_a, delay_b);
        let mut maximum = 0.0_f32;
        let mut difference_energy = 0.0;
        let mut reference_energy = 0.0;
        for channel in 0..CHANNELS {
            for frame in WINDOW_START..FRAMES {
                let difference = raw_b[channel][frame] - raw_a[channel][frame];
                maximum = maximum.max(difference.abs());
                difference_energy += f64::from(difference).powi(2);
                reference_energy += f64::from(raw_a[channel][frame]).powi(2);
            }
        }
        assert!(reference_energy.is_finite() && reference_energy > 0.0);
        let relative_l2 = (difference_energy / reference_energy).sqrt();
        let quantiles_a = energy_times(&stereo_energy(&raw_a), RATE).map(|x| x - PREROLL as i64);
        let quantiles_b = energy_times(&stereo_energy(&raw_b), RATE).map(|x| x - PREROLL as i64);
        println!(
            "physical_onset whole_mixture profile={name} options=0x{options:08x} \
             rate={rate:.17} nominal_delay={delay_a} window=[-8192,{POSTROLL}) \
             max_context_difference={maximum:.12e} relative_l2={relative_l2:.12e} \
             uncropped_q_a={quantiles_a:?} uncropped_q_b={quantiles_b:?} \
             isolated_attack_retention=unavailable accepted_musical_c=none",
            name = profile.name,
            options = profile.options,
        );
    }
}
