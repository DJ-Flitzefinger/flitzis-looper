//! Matched stereo all-wet software workload; no device or stem-selection claim.

use super::*;
use crate::audio_engine::audio_stream::render_scheduled_audio;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::native_render_observation::{Observation, Scope};

const FRAMES: usize = 512;
const PERIODS: usize = 768;
const RATIO: f64 = 1.2;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
struct Voice {
    active: bool,
    generation: u64,
    source: usize,
    native: usize,
    ratio: f64,
    target: f64,
    native_pitch: Option<f64>,
    origin: Option<(bool, u64)>,
    wet: bool,
    failed: bool,
    pending: bool,
    history_frames: Option<u64>,
    continuity: u64,
    position: usize,
    fraction: f64,
    fifo: (usize, usize),
    peak: f32,
}

#[derive(serde::Serialize)]
struct Period {
    index: usize,
    first: u64,
    drain_ns: u128,
    render_ns: u128,
    pacing_late_ns: u128,
    native: Observation,
    voices: [Voice; 6],
    mix_peak: f32,
    max_step: f32,
    nonfinite: usize,
    subnormal: usize,
    clipped: usize,
    trajectory_errors: usize,
}

fn sources() -> [SampleBuffer; 6] {
    std::array::from_fn(|id| {
        SampleBuffer {
            channels: 2,
            residency: None,
            samples: (0..3 * RATE as usize)
                .flat_map(|frame| {
                    (0..2).map(move |channel| {
                        let t = frame as f64 / f64::from(RATE);
                        let hz = 110.0 + id as f64 * 37.0 + channel as f64 * 13.0;
                        let phase = t * std::f64::consts::TAU;
                        // Integer frequencies and a two-second envelope keep the loop continuous.
                        let envelope = 0.7 + 0.3 * (phase * 0.5).cos();
                        (0.045 * envelope * ((phase * hz).sin() + 0.3 * (phase * (hz * 2.0)).sin()))
                            as f32
                    })
                })
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(RATE)
    })
}

fn bind(fixture: &Fixture, count: usize) {
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    for id in 0..count {
        loaded[id] = true;
        starts[id] = 0.1;
        ends[id] = Some(2.1);
        bindings[id] = fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
        assert!(bindings[id].as_ref().unwrap().is_current());
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
        )
        .unwrap();
}

fn voices(fixture: &Fixture, peaks: &[f32; NUM_SAMPLES]) -> [Voice; 6] {
    std::array::from_fn(|id| {
        let Some(voice) = fixture
            .mixer
            .voices
            .iter()
            .find(|v| v.is_playing_sample(id))
        else {
            return Voice::default();
        };
        let position = voice.source_playback.position();
        Voice {
            active: true,
            generation: voice.generation,
            source: voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
            native: voice.stretch.native_state_address(),
            ratio: voice.source_playback.tempo_ratio(),
            target: voice.source_playback.rate_target(),
            native_pitch: voice.stretch.native_pitch_scale_observation(),
            origin: voice
                .stretch
                .native_origin_tuple()
                .map(|(_, lock, request)| (lock, request)),
            wet: voice.stretch.wet_current_source_output_ready(),
            failed: voice.stretch.wet_mode_failed(),
            pending: voice.stretch.source_preparation_pending(),
            history_frames: voice
                .stretch
                .productive_history()
                .map(|h| h.fed_output_frames),
            continuity: voice.source_feed_history.continuity_revision(),
            position: position.frame,
            fraction: position.fraction,
            fifo: voice.stretch.pending_fifo_frames(),
            peak: peaks[id],
        }
    })
}

fn case(sources: &[SampleBuffer; 6], count: usize, lock: bool) -> bool {
    let mut fixture = Fixture::new_format(lock, RATE, 2);
    let ids = (0..count).collect::<Vec<_>>();
    for (id, source) in sources.iter().enumerate().take(count) {
        publish_tone_with_rate(&fixture.control, id, source.clone(), true, RATE);
    }
    adopt_current_source_ids(&mut fixture, lock, &ids);
    bind(&fixture, count);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    for &id in &ids {
        fixture.mixer.set_pad_bpm(id, Some(100.0));
        fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
        assert_eq!(fixture.mixer.key_lock_for_measurement(id), lock);
    }
    fixture.refresh_clock();
    let captured = Python::attach(|py| {
        let entries = ids
            .iter()
            .map(|&id| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (Py::new(py, binding).unwrap(), 0.1, Some(2.1), None)
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    fixture.drain();
    let starts = captured
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|m| m.start)
        .collect::<Vec<_>>();
    let original = captured.original_frame();
    assert!(
        fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .is_some()
    );
    fixture.drain();
    let admitted = starts
        .iter()
        .map(CapturedPadStart::execution_frame)
        .collect::<Vec<_>>();
    let prepared_begin = Instant::now();
    if lock {
        crate::audio_engine::ordinary_start_tests::wait_ready(
            &mut fixture,
            &starts
                .iter()
                .map(CapturedPadStart::sequence)
                .collect::<Vec<_>>(),
        );
    }
    let preparation_ns = prepared_begin.elapsed().as_nanos();
    fixture.render(0);
    fixture.feedback.reserve(128);
    let origin = fixture.transport.output_frame();
    let mut output = [0.0_f32; FRAMES * 2];
    let mut peaks = [0.0; NUM_SAMPLES];
    let mut rows = Vec::with_capacity(PERIODS);
    let mut previous_sample = [0.0_f32; 2];
    let epoch = Instant::now();
    for index in 0..PERIODS {
        sleep_until(epoch, (index * FRAMES) as u64, RATE);
        let due = Duration::from_secs_f64((index * FRAMES) as f64 / f64::from(RATE));
        let pacing_late_ns = epoch.elapsed().saturating_sub(due).as_nanos();
        let before: [_; 6] = std::array::from_fn(|id| {
            fixture
                .mixer
                .voices
                .iter()
                .find(|v| v.is_playing_sample(id))
                .map(|v| v.source_playback)
        });
        let first = fixture.transport.output_frame();
        let began = Instant::now();
        fixture.drain();
        let drain_ns = began.elapsed().as_nanos();
        let scope = Scope::begin();
        let began = Instant::now();
        render_scheduled_audio(
            &mut fixture.mixer,
            &mut fixture.scheduler,
            &mut output,
            &mut peaks,
            first,
            2,
            &mut fixture.transport,
            &mut fixture.feedback,
            &mut fixture.retirement,
        );
        let render_ns = began.elapsed().as_nanos();
        let native = scope.finish();
        fixture.transport.advance_by_rendered_frames(FRAMES);
        let actual = voices(&fixture, &peaks);
        let mut trajectory_errors = 0;
        for (id, previous) in before.into_iter().enumerate() {
            if let Some(mut expected) = previous
                && (!expected.advance(FRAMES)
                    || !fixture.voice(id).source_playback.matches_exact(&expected))
            {
                trajectory_errors += 1;
            }
        }
        let mut max_step = 0.0_f32;
        for frame in output.chunks_exact(2) {
            for channel in 0..2 {
                max_step = max_step.max((frame[channel] - previous_sample[channel]).abs());
                previous_sample[channel] = frame[channel];
            }
        }
        rows.push(Period {
            index,
            first,
            drain_ns,
            render_ns,
            pacing_late_ns,
            native,
            voices: actual,
            mix_peak: output.iter().map(|v| v.abs()).fold(0.0, f32::max),
            max_step,
            nonfinite: output.iter().filter(|v| !v.is_finite()).count(),
            subnormal: output.iter().filter(|v| v.is_subnormal()).count(),
            clipped: output.iter().filter(|v| v.abs() >= 1.0).count(),
            trajectory_errors,
        });
    }
    let effects = starts
        .iter()
        .map(CapturedPadStart::effective_frame)
        .collect::<Vec<_>>();
    eprintln!(
        "multivoice_raw {}",
        serde_json::json!({
            "count":count,"key_lock":lock,"rate":RATE,"channels":2,"frames":FRAMES,
            "master_bpm":120,"source_bpm":100,"ratio":RATIO,"periods":PERIODS,
            "original_T":original,"admitted_E":admitted,"effects_E":effects,"origin":origin,"preparation_ns":preparation_ns,"waited_for_native":lock,
            "source_addresses":sources.iter().map(|s|s.samples.as_ptr() as usize).collect::<Vec<_>>(),
            "source_frames":sources.iter().map(SampleBuffer::frame_count).collect::<Vec<_>>(),
            "rows":rows,"limits":"Synthetic complete stereo sources, no selected stems, manual Native BPM authority, public original group; render-thread scoped cfgtest instrumentation, no device/controller/hearing/quality/Human acceptance. FIFO deficits cover Rust adapter only, not Native internal fallback."
        })
    );
    // FirstSound may accelerate to this callback or retain the admitted E.
    // Preserve pre-E silence and every onset row, including silent Native output.
    let effect = effects[0].unwrap_or(u64::MAX);
    let active_rows = rows
        .iter()
        .filter(|row| row.first >= effect)
        .collect::<Vec<_>>();
    let checks = [
        (
            "originals_applied",
            starts
                .iter()
                .all(|s| s.state() == "applied" && s.original_frame() == original),
        ),
        (
            "common_actual_E",
            effects.iter().all(|e| *e == effects[0] && e.is_some()),
        ),
        ("bpm_lock", fixture.mixer.bpm_lock_for_measurement()),
        (
            "finite_unclipped_exact_trajectory",
            rows.iter()
                .all(|r| r.nonfinite == 0 && r.clipped == 0 && r.trajectory_errors == 0),
        ),
        (
            "adapter_source_success",
            rows.iter().all(|r| {
                r.native.adapter_resets == 0
                    && r.native.source_refusals == 0
                    && r.native.adapter_shortfall_channel_frames == 0
            }),
        ),
        (
            "three_source_loops",
            active_rows.len() as f64 * FRAMES as f64 * RATIO > 6.0 * f64::from(RATE),
        ),
        (
            "pre_E_silence",
            rows.iter()
                .filter(|row| row.first + FRAMES as u64 <= effect)
                .all(|row| row.mix_peak == 0.0 && row.native.shifts == 0),
        ),
        (
            "actual_voice_source_wet",
            active_rows.iter().all(|r| {
                r.voices.iter().take(count).enumerate().all(|(id, v)| {
                    v.active
                        && !v.failed
                        && !v.pending
                        && v.source == sources[id].samples.as_ptr() as usize
                        && (v.ratio - RATIO).abs() < 1e-12
                        && (v.target - RATIO).abs() < 1e-12
                        && if lock {
                            v.wet
                                && v.native != 0
                                && v.native_pitch
                                    .is_some_and(|p| (p - 1.0 / RATIO).abs() < 1e-12)
                        } else {
                            !v.wet
                        }
                })
            }),
        ),
        // This independently declared settling span is not an onset/quality gate.
        (
            "settled_positive_pad_output",
            active_rows
                .iter()
                .filter(|r| r.first - effect >= 8192)
                .all(|r| r.voices.iter().take(count).all(|v| v.peak > 1e-7)),
        ),
        (
            "actual_shift_count",
            active_rows.iter().all(|r| {
                if lock {
                    r.native.shifts == count as u64
                } else {
                    r.native.shifts == 0
                }
            }),
        ),
        (
            "stable_history_and_owners",
            active_rows.windows(2).all(|pair| {
                (0..count).all(|id| {
                    let (a, b) = (pair[0].voices[id], pair[1].voices[id]);
                    (a.generation, a.native, a.origin, a.continuity)
                        == (b.generation, b.native, b.origin, b.continuity)
                        && b.history_frames == a.history_frames.map(|n| n + FRAMES as u64)
                })
            }),
        ),
    ];
    eprintln!(
        "multivoice_checks {}",
        serde_json::json!({"count":count,"key_lock":lock,"checks":checks})
    );
    checks.iter().all(|(_, passed)| *passed)
}

#[test]
#[ignore = "explicit matched 4/5/6-wet plus6OFF paced stereo diagnostic; no device"]
fn matched_multivoice_key_lock_workload() {
    let sources = sources();
    let results = [(4, true), (5, true), (6, true), (6, false)]
        .map(|(count, lock)| (count, lock, case(&sources, count, lock)));
    assert!(results.iter().all(|(_, _, passed)| *passed), "{results:?}");
}
