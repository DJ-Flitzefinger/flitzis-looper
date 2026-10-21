//! Opt-in B5 measurement over the real accepted inverse, using existing renderer owners.
//! Natural-clock failures are results; coordinate/protocol/refusal errors are assertions.

use super::*;
use crate::audio_engine::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, MAX_DISCARDED_ENERGY, TIMING_BUDGET_MS, energy_times, stereo_energy,
    stereo_peak,
};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::prepared_native_history::NativeAdapterState;
use crate::audio_engine::rubberband_backend::{RubberBandLiveOptions, RubberBandLiveShifter};
use crate::audio_engine::source_beatmap::render_comparison_tests::{
    CanonicalFixture, OUTPUT, RATE, SOURCE, SPAN, Signal, pcm_hash,
};
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, FrameRange, SourceLoopDomain, SourceReadPlan, StemRenderSelection,
    StemTransition,
};
use crate::messages::SampleBuffer;

const TAIL: usize = 32_768;
const CALLERS: [(&str, &[usize]); 2] = [("512", &[512]), ("irregular", &[73, 511, 7, 512, 127, 3])];

fn emit_map(value: serde_json::Value) {
    println!("ACCEPTED_MAP_RENDER_COMPARISON {value}");
}

fn planar(sample: &SampleBuffer) -> Vec<Vec<f32>> {
    (0..2)
        .map(|channel| sample.samples.chunks_exact(2).map(|p| p[channel]).collect())
        .collect()
}

fn output_hash(output: &[Vec<f32>]) -> String {
    let interleaved = (0..output[0].len())
        .flat_map(|n| [output[0][n], output[1][n]])
        .collect::<Vec<_>>();
    pcm_hash(&interleaved)
}

struct LiveCapture {
    output: Vec<Vec<f32>>,
    dry: Vec<Vec<f32>>,
    mixed_pitch_blocks: usize,
}

fn mapped_live(
    fixture: &CanonicalFixture,
    sample: &SampleBuffer,
    pitch: PitchIntent,
    lock: bool,
    phase: usize,
    caller: (&str, &[usize]),
) -> LiveCapture {
    assert!(matches!(phase, 0 | 17));
    let started = Instant::now();
    let h = pitch.frequency_ratio();
    let first_pitch = pitch.native_pitch_scale(fixture.rates[0], lock).unwrap();
    let mut native = RubberBandLiveShifter::new(RATE, 2).unwrap();
    assert_eq!(native.block_size(), 512);
    native.prepare_exact_pitch(first_pitch).unwrap();
    let native_identity = native.state_address();
    let nominal_delay = native.start_delay();
    let mut adapter = NativeAdapterState::new(Some(native), 2);
    adapter.pitch_scale = first_pitch;
    let mut output = (0..2)
        .map(|_| Vec::with_capacity(OUTPUT + TAIL))
        .collect::<Vec<_>>();
    let mut dry = (0..2)
        .map(|_| Vec::with_capacity(OUTPUT + TAIL))
        .collect::<Vec<_>>();
    let mut feed = vec![vec![0.0; 512]; 2];
    let mut wet = vec![vec![0.0; 512]; 2];
    let region = FrameRange {
        start: 0,
        end: SOURCE,
    };
    let domain = SourceLoopDomain::physical(SOURCE, region);
    let mut caller_partition = 0;
    let mut caller_cursor = 0;
    let mut staged = 0;
    let mut native_cursor = 0;
    let mut mixed_pitch_blocks = 0;
    // Caller partitions only fill a canonical 512-output-frame staging interval. Native input
    // subchunks are then split at actual map knots. Every caller uses the SAME block ownership.
    while caller_cursor < OUTPUT {
        let count = caller.1[caller_partition % caller.1.len()].min(OUTPUT - caller_cursor);
        let mut remaining = count;
        while remaining != 0 {
            let copied = remaining.min(512 - staged);
            staged += copied;
            caller_cursor += copied;
            remaining -= copied;
            if staged == 512 || caller_cursor == OUTPUT {
                let stop = native_cursor + staged;
                let mut cursor = native_cursor;
                let mut previous_pitch = None;
                let mut block_mixed = false;
                while cursor < stop {
                    let mapped = (cursor + phase) % OUTPUT;
                    let segment = mapped / SPAN;
                    let frames = (SPAN - mapped % SPAN).min(stop - cursor);
                    let coordinate = fixture.coordinates[mapped];
                    let playback = SourcePlayback::diagnostic_affine_epoch(
                        coordinate,
                        fixture.rates[segment],
                        domain,
                    )
                    .unwrap();
                    for offset in 0..frames {
                        let actual = playback.position_at(offset);
                        let expected = fixture.coordinates[(mapped + offset) % OUTPUT];
                        assert!((actual.frame as f64 + actual.fraction - expected).abs() <= 1.0e-8);
                    }
                    let position = playback.position();
                    let plan = SourceReadPlan {
                        channels: 2,
                        sample_frames: SOURCE,
                        frame_pos: position.frame,
                        loop_region: region,
                        loop_period: None,
                        seek_mode: ExplicitSeekMode::Normal,
                        selection: StemRenderSelection::full_mix(),
                        transition: StemTransition::default(),
                    };
                    plan.fill_fractional_buffers(sample, None, &playback, &mut feed, frames);
                    let scale = pitch
                        .native_pitch_scale(fixture.rates[segment], lock)
                        .unwrap();
                    assert_eq!(scale, if lock { h / fixture.rates[segment] } else { h });
                    if previous_pitch.is_some_and(|old: f64| old.to_bits() != scale.to_bits()) {
                        block_mixed = true;
                    }
                    previous_pitch = Some(scale);
                    adapter.render(&feed, &mut wet, frames, scale).unwrap();
                    for channel in 0..2 {
                        dry[channel].extend_from_slice(&feed[channel][..frames]);
                        output[channel].extend_from_slice(&wet[channel][..frames]);
                    }
                    cursor += frames;
                }
                mixed_pitch_blocks += usize::from(block_mixed);
                native_cursor = stop;
                staged = 0;
            }
        }
        caller_partition += 1;
    }
    assert_eq!(native_cursor, OUTPUT);
    assert_eq!(staged, 0);
    let scale = pitch
        .native_pitch_scale(fixture.rates[((OUTPUT - 1 + phase) % OUTPUT) / SPAN], lock)
        .unwrap();
    feed.iter_mut().for_each(|channel| channel.fill(0.0));
    for _ in 0..TAIL / 512 {
        adapter.render(&feed, &mut wet, 512, scale).unwrap();
        for channel in 0..2 {
            output[channel].extend_from_slice(&wet[channel]);
            dry[channel].extend_from_slice(&feed[channel]);
        }
    }
    assert_eq!(output[0].len(), OUTPUT + TAIL);
    assert!(output.iter().flatten().all(|value| value.is_finite()));
    emit_map(
        serde_json::json!({"stage":"mapped_live_complete","caller":caller.0,
        "map_revision":fixture.map.map_revision(),"source_identity":fixture.map.source_identity(),
        "total_semitones":pitch.total_semitones(),"key_lock":lock,"output_phase":phase,
        "source_start_frames":fixture.coordinates[phase],"source_frames":SOURCE,
        "mapped_output_frames":OUTPUT,"uncropped_output_frames":OUTPUT+TAIL,
        "zero_feed_capture_tail_frames":TAIL,"internal_native_tail_exhaustion_proven":false,
        "native_identity":native_identity,"native_options":RubberBandLiveOptions::default_realtime().bits(),
        "nominal_start_delay_observation_only":nominal_delay,"external_translation":0,"external_gain":1,
        "mixed_pitch_native_blocks":mixed_pitch_blocks,
        "intrablock_pitch_limitation":"existing adapter sets the completing subchunk pitch for a whole pending FIFO block; not arbitrary intrablock pitch support",
        "input_fifo_frames":adapter.input_fifo.iter().map(|f|f.len()).collect::<Vec<_>>(),
        "output_fifo_frames":adapter.output_fifo.iter().map(|f|f.len()).collect::<Vec<_>>(),
        "complete_pcm_sha256":output_hash(&output),"elapsed_ms":started.elapsed().as_secs_f64()*1000.0}),
    );
    LiveCapture {
        output,
        dry,
        mixed_pitch_blocks,
    }
}

fn comparison_metrics(label: &str, output: &[Vec<f32>], dry: &[Vec<f32>], marker: usize) {
    let energy = stereo_energy(output);
    let total = energy.iter().sum::<f64>();
    assert!(total.is_finite());
    if total == 0.0 {
        emit_map(
            serde_json::json!({"stage":"acoustic","label":label,"missing_response":true,"pass":false}),
        );
        return;
    }
    let dry_q = energy_times(&stereo_energy(dry), RATE);
    let actual_q = energy_times(&energy, RATE);
    let offsets = std::array::from_fn::<_, 3, _>(|i| actual_q[i] - dry_q[i]);
    let prefix = energy[..marker.min(energy.len())].iter().sum::<f64>() / total;
    let tail = energy[energy.len().saturating_sub(RATE as usize / 50)..]
        .iter()
        .sum::<f64>()
        / total;
    let (peak_at, peak) = stereo_peak(output);
    let timing = (f64::from(RATE) * TIMING_BUDGET_MS / 1000.0).ceil() as i64;
    let predicates = [
        offsets[0].abs() <= timing,
        offsets[1].abs() <= timing,
        prefix <= MAX_DISCARDED_ENERGY,
        tail <= MAX_CAPTURE_TAIL_ENERGY,
        peak_at >= marker,
    ];
    emit_map(
        serde_json::json!({"stage":"acoustic","label":label,"frames":output[0].len(),
        "natural_clock_offsets_q10_q50_q90":offsets,"raw_quantiles":actual_q,"dry_quantiles":dry_q,
        "complete_peak_frame":peak_at,"peak":peak,"energy_before_declared_marker":prefix,
        "final_20ms_energy_fraction":tail,"timing_budget_ms":TIMING_BUDGET_MS,
        "discard_budget":MAX_DISCARDED_ENERGY,"tail_budget":MAX_CAPTURE_TAIL_ENERGY,
        "predicates_q10_q50_discard_tail_peak":predicates,"pass":predicates.into_iter().all(|p|p),
        "scope":"original predicates within the declared finite capture; no full internal Native tail exhaustion or B5 acceptance",
        "fitted_delay_gain_crop":false}),
    );
}

fn frequency(samples: &[f32]) -> (Option<f64>, usize, f64) {
    let mut crossings = Vec::new();
    for (index, pair) in samples.windows(2).enumerate() {
        if pair[0] <= 0.0 && pair[1] > 0.0 {
            crossings.push(index as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0]));
        }
    }
    let measured = (crossings.len() >= 3).then(|| {
        (crossings.len() - 1) as f64 * f64::from(RATE) / (crossings.last().unwrap() - crossings[0])
    });
    let energy = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    (measured, crossings.len(), energy)
}

fn pitch_metrics(
    label: &str,
    output: &[Vec<f32>],
    fixture: &CanonicalFixture,
    pitch: PitchIntent,
    lock: bool,
    phase: usize,
) {
    for segment in 0..4 {
        let first = segment * SPAN + 16_384 - phase;
        let last = first + 8192;
        for channel in 0..2 {
            // Independent integer-semitone oracle, outside the production pitch helpers.
            let expected = [160.0, 211.0][channel]
                * 2.0_f64.powf(f64::from(pitch.base_shift() + pitch.extra_shift()) / 12.0)
                * if lock { 1.0 } else { fixture.rates[segment] };
            let (measured, crossings, energy) = frequency(&output[channel][first..last]);
            let cents = measured.map(|hz| 1200.0 * (hz / expected).log2());
            // Retain the original public tonal proof's energy, cycle and strict cents guards.
            // Too few cycles in this frozen window is missing evidence, never a pitch pass.
            let eligible = crossings > 8 && energy > 0.001;
            emit_map(
                serde_json::json!({"stage":"pitch","label":label,"segment":segment,
                "channel":channel,"window":[first,last],"expected_hz":expected,"measured_hz":measured,
                "cents":cents,"budget_cents_strict_less_than":15.0,
                "positive_crossings":crossings,"required_crossings_strict_greater_than":8,
                "mean_energy":energy,"required_energy_strict_greater_than":0.001,
                "pitch_evidence_eligible":eligible,
                "pass":eligible && cents.is_some_and(|c|c.abs()<15.0)}),
            );
        }
    }
}

fn assert_complete_partition_equality(left: &[Vec<f32>], right: &[Vec<f32>]) {
    assert_eq!(left.len(), right.len());
    for (a, b) in left.iter().zip(right) {
        assert_eq!(a.len(), b.len());
        assert!(
            a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()),
            "canonical native block ownership must not depend on caller partitions"
        );
    }
}

#[test]
#[ignore = "explicit bounded accepted-map renderer diagnostic; no production, device or hearing acceptance"]
fn compare_accepted_nonuniform_map_renderers() {
    #[cfg(windows)]
    emit_map(serde_json::json!({"stage":"linked_native_identity",
        "identity":crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("actual linked Native DLL identity must be available")}));
    #[cfg(not(windows))]
    emit_map(
        serde_json::json!({"stage":"linked_native_identity","identity":null,
        "limit":"Windows loaded-DLL identity helper is unavailable on this platform"}),
    );
    let pitches = [
        PitchIntent::default(),
        PitchIntent::new(0, 7).unwrap(),
        PitchIntent::new(-5, -18).unwrap(),
        PitchIntent::new(6, 18).unwrap(),
    ];
    let signals = [
        Signal::StereoTone,
        Signal::IsolatedAttack(SPAN / 4),
        Signal::IsolatedAttack(SPAN + SPAN / 4),
        Signal::IsolatedAttack(2 * SPAN + SPAN / 4),
        Signal::IsolatedAttack(3 * SPAN + SPAN / 4),
    ];
    for signal in signals {
        let fixture = CanonicalFixture::new(signal);
        fixture.verify_pcm();
        let original = planar(&fixture.sample);
        emit_map(
            serde_json::json!({"stage":"frozen_fixture","signal":format!("{signal:?}"),
            "map_revision":fixture.map.map_revision(),"source_identity":fixture.map.source_identity(),
            "source_pcm_sha256":fixture.pcm_hash,"mono_sha256":fixture.mono_hash,
            "source_frames":SOURCE,"output_spans":SPAN,"source_to_output_keyframes":fixture.keyframes,
            "rates_from_shared_inverse":fixture.rates,"all_used_coordinates_checked":fixture.coordinates.len(),
            "synthetic_consistency_not_live_custody":true,"production_sync_enabled":false}),
        );
        for pitch in pitches {
            for lock in [true, false] {
                let mut base_dry = None;
                for phase in [0, 17] {
                    let mut prior: Option<Vec<Vec<f32>>> = None;
                    for caller in CALLERS {
                        let capture =
                            mapped_live(&fixture, &fixture.sample, pitch, lock, phase, caller);
                        if let Some(ref previous) = prior {
                            assert_complete_partition_equality(previous, &capture.output);
                        }
                        if phase == 17 && lock {
                            assert!(capture.mixed_pitch_blocks >= 3);
                        }
                        let label = format!(
                            "Live_{signal:?}_k{}_lock{lock}_phase{phase}_{}",
                            pitch.total_semitones(),
                            caller.0
                        );
                        match signal {
                            Signal::StereoTone => {
                                pitch_metrics(&label, &capture.output, &fixture, pitch, lock, phase)
                            }
                            Signal::IsolatedAttack(marker) => comparison_metrics(
                                &label,
                                &capture.output,
                                &capture.dry,
                                marker - phase,
                            ),
                        }
                        if phase == 17 {
                            let seam = OUTPUT - phase;
                            emit_map(
                                serde_json::json!({"stage":"loop_seam","label":label,"at":seam,
                                "raw_step":(0..2).map(|ch|capture.output[ch][seam]-capture.output[ch][seam-1]).collect::<Vec<_>>(),
                                "dry_step":(0..2).map(|ch|capture.dry[ch][seam]-capture.dry[ch][seam-1]).collect::<Vec<_>>(),
                                "no_delay_fitted_no_seam_acceptance":true}),
                            );
                        } else {
                            base_dry = Some(capture.dry);
                        }
                        prior = Some(capture.output);
                    }
                }
                if !lock {
                    emit_map(
                        serde_json::json!({"stage":"offline_unsupported","total_semitones":pitch.total_semitones(),
                        "key_lock":false,"reason":"offline Stretcher cannot automate h*r pitch after study begins"}),
                    );
                    continue;
                }
                let configuration = OfflineConfiguration {
                    sample_rate_hz: RATE,
                    engine: 3,
                    options: THREADING_NEVER | CHANNELS_TOGETHER | ENGINE_FINER,
                    time_ratio: OUTPUT as f64 / SOURCE as f64,
                    pitch_scale: pitch.frequency_ratio(),
                    keyframes: None,
                };
                let mut previous: Option<Vec<Vec<f32>>> = None;
                for caller in CALLERS {
                    let output = offline_capture_with_map(
                        &original,
                        configuration,
                        caller.0,
                        caller.1,
                        OUTPUT,
                        true,
                        Some(&fixture.keyframes),
                    );
                    if let Some(ref previous) = previous {
                        assert_complete_partition_equality(previous, &output);
                    }
                    let label = format!(
                        "OfflineR3_{signal:?}_k{}_{}",
                        pitch.total_semitones(),
                        caller.0
                    );
                    match signal {
                        Signal::StereoTone => {
                            pitch_metrics(&label, &output, &fixture, pitch, true, 0)
                        }
                        Signal::IsolatedAttack(marker) => {
                            comparison_metrics(&label, &output, base_dry.as_ref().unwrap(), marker)
                        }
                    }
                    emit_map(
                        serde_json::json!({"stage":"offline_complete_identity","label":label,
                        "map_revision":fixture.map.map_revision(),"complete_pcm_sha256":output_hash(&output),
                        "phase17":"unsupported: unrolled offline preparation boundary not implemented; no rotated/cropped equivalence"}),
                    );
                    previous = Some(output);
                }
            }
        }
        if matches!(signal, Signal::StereoTone) {
            let pitch = PitchIntent::new(0, 7).unwrap();
            let parent = mapped_live(&fixture, &fixture.sample, pitch, true, 0, CALLERS[0]);
            let mut sum = vec![vec![0.0; OUTPUT + TAIL]; 2];
            let mut raw_sum = vec![0.0_f32; fixture.sample.samples.len()];
            let mut component_inputs = Vec::new();
            for gain in [0.1_f32, 0.2, 0.3, 0.4] {
                let component = SampleBuffer {
                    samples: fixture
                        .sample
                        .samples
                        .iter()
                        .map(|v| v * gain)
                        .collect::<Vec<_>>()
                        .into(),
                    channels: 2,
                    residency: None,
                };
                for (sum, value) in raw_sum.iter_mut().zip(component.samples.iter()) {
                    *sum += value;
                }
                component_inputs.push(serde_json::json!({"gain":gain,
                    "pcm_sha256":pcm_hash(&component.samples)}));
                let rendered = mapped_live(&fixture, &component, pitch, true, 0, CALLERS[0]);
                for channel in 0..2 {
                    for (sum, value) in sum[channel].iter_mut().zip(&rendered.output[channel]) {
                        *sum += value;
                    }
                }
            }
            let errors = sum
                .iter()
                .flatten()
                .zip(parent.output.iter().flatten())
                .map(|(a, b)| f64::from(*a - *b))
                .collect::<Vec<_>>();
            emit_map(
                serde_json::json!({"stage":"synthetic_parent_map_components","map_revision":fixture.map.map_revision(),
                "independent_serial_native_instances":4,"shared_canonical_coordinates":true,
                "component_inputs":component_inputs,
                "raw_component_sum_max_error":raw_sum.iter().zip(fixture.sample.samples.iter())
                    .map(|(a,b)|f64::from(*a-*b).abs()).fold(0.0_f64,f64::max),
                "actual_prepared_stem_set_masks_lifetimes_tested":false,
                "audio_max_error":errors.iter().map(|e|e.abs()).fold(0.0_f64,f64::max),
                "audio_rms_error":(errors.iter().map(|e|e*e).sum::<f64>()/errors.len() as f64).sqrt(),
                "sum_energy_quantiles":energy_times(&stereo_energy(&sum),RATE),
                "parent_energy_quantiles":energy_times(&stereo_energy(&parent.output),RATE),
                "coherent_source_timing_does_not_assert_identical_nonlinear_stem_audio":true}),
            );
        }
    }
}
