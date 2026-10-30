//! Paced, device-free production capture/admission/worker/scheduler/mixer measurements.
//! This diagnostic deliberately preserves late/cold failures; it is not a latency gate.

use super::*;
use crate::audio_engine::pitch_runtime_tests::publish_tone_with_rate;
use crate::messages::SampleBuffer;
use pyo3::prelude::*;
use std::sync::Arc;

#[test]
#[ignore = "explicit live-original diagnostic; frozen readiness, software timeline only"]
fn live_original_chain_response() {
    use crate::audio_engine::ordinary_start_tests::preparation_deadline_tests::WorkerGate;
    for ratio in [0.8, 1.16] {
        let mut fixture = configured(48_000, true, ratio, 0);
        let mut originals = Vec::new();
        let mut gate = None;
        for frame in [0, 16, 32, 48] {
            assert!(fixture.render_to(frame).iter().all(|value| *value == 0.0));
            fixture.refresh_clock();
            let start = fixture
                .control
                .engine
                .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
                .unwrap();
            if frame == 0 {
                gate = Some(WorkerGate::for_ordinary(&start));
            }
            fixture.drain();
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pad_start(&start, None)
                    .unwrap()
            );
            fixture.drain();
            if frame == 0 {
                gate.as_ref().unwrap().wait_for(false);
            }
            assert_eq!(start.original_frame(), Some(frame));
            originals.push(start);
        }
        assert!(fixture.render_to(64).iter().all(|value| *value == 0.0));
        let admitted = originals
            .iter()
            .map(CapturedPadStart::execution_frame)
            .collect::<Vec<_>>();
        let began = Instant::now();
        let gate = gate.unwrap();
        gate.release();
        assert_eq!(gate.wait_for(true), None);
        crate::audio_engine::ordinary_start_tests::wait_ready(
            &mut fixture,
            &originals
                .iter()
                .map(CapturedPadStart::sequence)
                .collect::<Vec<_>>(),
        );
        let ready_us = began.elapsed().as_micros();
        assert_eq!(fixture.transport.output_frame(), 64);
        let mut pcm = Vec::new();
        for _ in 0..4 {
            pcm.extend(fixture.render(512));
        }
        let effects = originals
            .iter()
            .map(CapturedPadStart::effective_frame)
            .collect::<Vec<_>>();
        assert!(originals.iter().all(|start| start.state() == "applied"));
        assert_eq!(fixture.starts(0), 4);
        let last = (effects[3].unwrap() - 64) as usize;
        pcm.extend(fixture.pcm(last + 24_000 - pcm.len()));
        eprintln!(
            "live_chain ratio={ratio} T=[0,16,32,48] admitted={admitted:?} ready_frame=64 residual_release_ready_us={ready_us} effects={effects:?} last_attack_from_E={:?}; residentPCM n1 frozenclock512partitions no device/readiness-speedup claim",
            landmarks(&pcm[last..], 48_000)
        );
        assert!(landmarks(&pcm[last..], 48_000).2.is_some());
    }
}

fn burst(rate: u32, silence: usize) -> SampleBuffer {
    let start = rate as usize / 10;
    let samples = (0..3 * rate as usize)
        .map(|frame| {
            let Some(age) = frame.checked_sub(start + silence) else {
                return 0.0;
            };
            let seconds = age as f64 / f64::from(rate);
            // One declared principal attack, exponential 4 ms decay, no later peaks.
            (0.4 * (-seconds / 0.004).exp() * (seconds * 1000.0 * std::f64::consts::TAU).sin())
                as f32
        })
        .collect::<Vec<_>>();
    SampleBuffer {
        channels: 1,
        samples: Arc::from(samples),
        residency: None,
    }
    .with_complete_source(rate)
}

fn configured(rate: u32, lock: bool, ratio: f64, silence: usize) -> Fixture {
    let mut fixture = Fixture::new_format(lock, rate, 1);
    let sample = burst(rate, silence);
    publish_tone_with_rate(&fixture.control, 0, sample, true, rate);
    adopt_current_source_ids(&mut fixture, lock, &[0, 1]);
    refresh_actual_runtime_bindings(&fixture);
    for id in [0, 1] {
        fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
    }
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    fixture.mixer.set_pad_bpm(0, Some(120.0 / ratio));
    fixture.mixer.set_pad_bpm(1, Some(120.0));
    fixture
}

fn sleep_until(epoch: Instant, frames: u64, rate: u32) {
    let due = epoch + Duration::from_secs_f64(frames as f64 / f64::from(rate));
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

/// Independent fixture definition: first nonzero, 10% peak amplitude, then the
/// centre of the strongest 1 ms energy window of the single declared burst.
/// No nominal Native delay or production attack candidate enters this estimator.
fn landmarks(pcm: &[f32], rate: u32) -> (Option<usize>, Option<usize>, Option<usize>) {
    let peak = pcm.iter().map(|value| value.abs()).fold(0.0_f32, f32::max);
    if peak < 1.0e-7 {
        return (None, None, None);
    }
    let first = pcm.iter().position(|value| value.abs() > 1.0e-7);
    let significant = pcm.iter().position(|value| value.abs() >= peak * 0.1);
    let width = (rate as usize / 1000).max(1);
    let mut energy: f64 = pcm[..width]
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum();
    let mut maximum = (energy, 0);
    for index in width..pcm.len() {
        energy += f64::from(pcm[index]).powi(2) - f64::from(pcm[index - width]).powi(2);
        if energy > maximum.0 {
            maximum = (energy, index + 1 - width);
        }
    }
    (first, significant, Some(maximum.1 + width / 2))
}

fn dry_reference(
    rate: u32,
    ratio: f64,
    silence: usize,
) -> (Option<usize>, Option<usize>, Option<usize>) {
    let mut dry = configured(rate, false, ratio, silence);
    let start = dry
        .control
        .engine
        .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
        .unwrap();
    dry.drain();
    assert!(dry.control.engine.publish_pad_start(&start, None).unwrap());
    dry.drain();
    landmarks(&dry.pcm(rate as usize / 2), rate)
}

fn ordinary_case(
    rate: u32,
    lock: bool,
    ratio: f64,
    warm: bool,
    late: bool,
    irregular: bool,
    silence: usize,
) {
    let dry = dry_reference(rate, ratio, silence);
    let mut fixture = configured(rate, lock, ratio, silence);
    if warm && lock && ratio != 1.0 {
        select_and_wait(&mut fixture);
    }
    let origin_frame = fixture.transport.output_frame();
    let epoch = Instant::now();
    fixture.refresh_clock();
    let start = fixture
        .control
        .engine
        .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
        .unwrap();
    let captured_ns = start.received_timestamp();
    fixture.drain();
    let original = start.original_frame().unwrap();
    let mut pcm = Vec::new();
    // Actual two-stage publication can arrive after its captured original T.
    if late {
        pcm.extend(fixture.render(512));
        sleep_until(epoch, 512, rate);
    }
    assert!(
        fixture
            .control
            .engine
            .publish_pad_start(&start, None)
            .unwrap()
    );
    fixture.drain();
    let admitted_us = epoch.elapsed().as_micros();
    let admission_frame = fixture.transport.output_frame();
    let mut max_callback_us = 0;
    let mut overruns = 0;
    let mut callback = 0;
    let mut wall_spans = Vec::new();
    let mut max_pacing_late_us = 0;
    while fixture.transport.output_frame() - origin_frame < u64::from(rate) / 2 {
        let frames = if irregular {
            [127, 383, 257, 769][callback % 4]
        } else {
            512
        };
        let before = Instant::now();
        let frame = fixture.transport.output_frame() - origin_frame;
        let expected_us = u128::from(frame) * 1_000_000 / u128::from(rate);
        let before_us = epoch.elapsed().as_micros();
        max_pacing_late_us = max_pacing_late_us.max(before_us.saturating_sub(expected_us));
        let begin = pcm.len();
        fixture.drain();
        pcm.extend(fixture.render(frames));
        wall_spans.push((begin, pcm.len(), before_us, epoch.elapsed().as_micros()));
        let elapsed = before.elapsed();
        max_callback_us = max_callback_us.max(elapsed.as_micros());
        overruns += usize::from(elapsed.as_secs_f64() > frames as f64 / f64::from(rate));
        callback += 1;
        sleep_until(epoch, fixture.transport.output_frame() - origin_frame, rate);
    }
    let points = landmarks(&pcm, rate);
    let significant_host = points
        .1
        .and_then(|point| {
            wall_spans
                .iter()
                .find(|span| span.0 <= point && point < span.1)
        })
        .map(|span| (span.2, span.3));
    let principal_host = points
        .2
        .and_then(|point| {
            wall_spans
                .iter()
                .find(|span| span.0 <= point && point < span.1)
        })
        .map(|span| (span.2, span.3));
    let principal_minus_dry = points
        .2
        .zip(dry.2)
        .map(|(wet, dry)| wet as i64 - dry as i64);
    eprintln!(
        "response_probe rate={rate} lock={lock} ratio={ratio} warm={warm} late={late} irregular={irregular} authored_silence={silence} capture_ns={captured_ns} admission_us={admitted_us} admission_frame={admission_frame} T={original} E={:?} first={:?} significant={:?} principal={:?} dry_landmarks={dry:?} principal_minus_dry={principal_minus_dry:?} significant_host_us={significant_host:?} principal_host_us={principal_host:?} max_pacing_late_us={max_pacing_late_us} state={} reason={:?} harness_render_max_us={max_callback_us} harness_overruns={overruns}; mono single-burst, two loaded PADs, software only",
        start.effective_frame(),
        points.0,
        points.1,
        points.2,
        start.state(),
        start.reason()
    );
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert!(points.2.is_some(), "missing principal burst");
    assert_eq!(start.original_frame(), Some(original));
}

#[test]
#[ignore = "explicit frozen-readiness cancellation diagnostic; software timeline only"]
fn completed_cancelled_predecessor_response() {
    for ratio in [0.8, 1.16] {
        let mut fixture = configured(48_000, true, ratio, 0);
        let first = fixture
            .control
            .engine
            .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
            .unwrap();
        fixture.drain();
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&first, None)
                .unwrap()
        );
        fixture.drain();
        crate::audio_engine::ordinary_start_tests::wait_ready(&mut fixture, &[first.sequence()]);
        assert!(first.cancel());
        assert!(fixture.render(16).iter().all(|value| *value == 0.0));
        fixture.refresh_clock();
        let next = fixture
            .control
            .engine
            .capture_pad_start(0, 0.1, Some(2.1), 1.0, false, None, None)
            .unwrap();
        fixture.drain();
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&next, None)
                .unwrap()
        );
        fixture.drain();
        let admitted = next.execution_frame();
        let began = Instant::now();
        crate::audio_engine::ordinary_start_tests::wait_ready(&mut fixture, &[next.sequence()]);
        let ready_us = began.elapsed().as_micros();
        let ready_frame = fixture.transport.output_frame();
        let pcm = fixture.pcm(24_000);
        let points = landmarks(&pcm, 48_000);
        eprintln!(
            "terminal_predecessor ratio={ratio} T={:?} admitted_E={admitted:?} ready_frame={ready_frame} ready_wait_us={ready_us} E={:?} first_from_T={:?} significant_from_T={:?} principal_from_T={:?}; readiness wait freezes timeline, no device/host deadline claim",
            next.original_frame(),
            next.effective_frame(),
            points.0,
            points.1,
            points.2
        );
        assert_eq!(next.state(), "applied", "{:?}", next.reason());
        assert_eq!(first.effective_frame(), None);
        assert_eq!(fixture.starts(0), 1);
        assert!(points.2.is_some());
    }
}

#[test]
#[ignore = "explicit old-group-peer diagnostic; frozen readiness, software timeline only"]
fn rejected_group_peer_response() {
    for ratio in [0.8, 1.16] {
        let mut fixture = configured(48_000, true, ratio, 0);
        publish_tone_with_rate(&fixture.control, 1, burst(48_000, 0), true, 48_000);
        adopt_current_source_ids(&mut fixture, true, &[0, 1]);
        refresh_actual_runtime_bindings(&fixture);
        fixture.mixer.set_pad_bpm(1, Some(120.0 / ratio));
        let captured = Python::attach(|py| {
            let entries = (0..2)
                .map(|id| {
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
        let originals = captured.resolve().unwrap().unwrap();
        assert!(
            fixture
                .control
                .engine
                .publish_captured_global_start(&captured)
                .unwrap()
                .is_some()
        );
        fixture.drain();
        crate::audio_engine::ordinary_start_tests::wait_ready(
            &mut fixture,
            &originals
                .iter()
                .map(|member| member.start.sequence())
                .collect::<Vec<_>>(),
        );
        assert!(originals[0].start.cancel());
        // The successor uses the OTHER, still-live peer's content.
        assert_ne!(originals[1].start.state(), "rejected");
        assert!(fixture.render(16).iter().all(|sample| *sample == 0.0));
        fixture.refresh_clock();
        let next = fixture
            .control
            .engine
            .capture_pad_start(1, 0.1, Some(2.1), 1.0, false, None, None)
            .unwrap();
        fixture.drain();
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&next, None)
                .unwrap()
        );
        fixture.drain();
        let admitted = next.execution_frame();
        let began = Instant::now();
        crate::audio_engine::ordinary_start_tests::wait_ready(&mut fixture, &[next.sequence()]);
        let ready_us = began.elapsed().as_micros();
        let ready_frame = fixture.transport.output_frame();
        let pcm = fixture.pcm(24_000);
        let points = landmarks(&pcm, 48_000);
        eprintln!(
            "rejected_group_peer ratio={ratio} T={:?} admitted_E={admitted:?} ready_frame={ready_frame} residual_wait_us={ready_us} E={:?} first={:?} significant={:?} principal={:?}; n1 residentPCM frozenclock no device/readiness-speedup claim",
            next.original_frame(),
            next.effective_frame(),
            points.0,
            points.1,
            points.2
        );
        assert_eq!(next.state(), "applied", "{:?}", next.reason());
        assert!(
            originals
                .iter()
                .all(|member| member.start.effective_frame().is_none())
        );
        assert_eq!(fixture.starts(0), 0);
        assert_eq!(fixture.starts(1), 1);
        assert!(points.2.is_some());
    }
}

#[test]
#[ignore = "paced production diagnostic; run explicitly with --ignored --nocapture --test-threads=1"]
fn paced_production_response_matrix() {
    for ratio in [0.8, 1.0, 1.16] {
        for lock in [false, true] {
            for warm in [false, true] {
                for late in [false, true] {
                    ordinary_case(48_000, lock, ratio, warm, late, false, 0);
                }
            }
        }
    }
    for rate in [44_100, 96_000] {
        for late in [false, true] {
            ordinary_case(rate, true, 0.8, true, late, true, 0);
        }
    }
    ordinary_case(48_000, true, 0.8, true, true, true, 960);
    ordinary_case(48_000, false, 0.8, true, true, true, 960);
}

#[test]
#[ignore = "paced production diagnostic; run explicitly with --ignored --nocapture --test-threads=1"]
fn paced_production_group_response() {
    for count in [2, 4, 6] {
        for warm in [false, true] {
            for wet in [1, 2] {
                let mut fixture = configured(RATE, true, 0.8, 0);
                for id in 1..count {
                    publish_tone_with_rate(&fixture.control, id, burst(RATE, 0), true, RATE);
                }
                adopt_current_source_ids(&mut fixture, true, &(0..count).collect::<Vec<_>>());
                refresh_actual_runtime_bindings(&fixture);
                for id in 0..count {
                    fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
                    fixture
                        .mixer
                        .set_pad_bpm(id, Some(if id < wet { 150.0 } else { 120.0 }));
                }
                if warm {
                    select_and_wait(&mut fixture);
                }
                let epoch = Instant::now();
                fixture.refresh_clock();
                let captured = Python::attach(|py| {
                    let entries = (0..count)
                        .map(|id| {
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
                    .map(|member| member.start)
                    .collect::<Vec<_>>();
                let mut pcm = fixture.render(512);
                sleep_until(epoch, 512, RATE);
                assert!(
                    fixture
                        .control
                        .engine
                        .publish_captured_global_start(&captured)
                        .unwrap()
                        .is_some()
                );
                fixture.drain();
                let admission_us = epoch.elapsed().as_micros();
                let mut max_callback_us = 0;
                while fixture.transport.output_frame() < u64::from(RATE) / 2 {
                    let before = Instant::now();
                    fixture.drain();
                    pcm.extend(fixture.render(512));
                    max_callback_us = max_callback_us.max(before.elapsed().as_micros());
                    sleep_until(epoch, fixture.transport.output_frame(), RATE);
                }
                let effects = starts
                    .iter()
                    .map(CapturedPadStart::effective_frame)
                    .collect::<Vec<_>>();
                eprintln!(
                    "response_group pads={count} wet={wet} warm={warm} admission_us={admission_us} T={:?} effects={effects:?} mix_landmarks={:?} harness_render_max_us={max_callback_us}; commonE/mix only, per-wet attack separately unresolved",
                    captured.original_frame(),
                    landmarks(&pcm, RATE)
                );
                assert!(
                    effects
                        .iter()
                        .all(|effect| *effect == effects[0] && effect.is_some())
                );
                assert!(starts.iter().all(|start| start.state() == "applied"));
            }
        }
    }
}

#[test]
#[ignore = "paced production diagnostic; run explicitly with --ignored --nocapture --test-threads=1"]
fn paced_production_piano_response() {
    for ratio in [0.8, 1.0, 1.16] {
        for warm in [false, true] {
            let mut fixture = configured(RATE, true, ratio, 0);
            fixture.rows(crate::audio_engine::input_mapping::PitchPadProjection {
                key_lock: true,
                source_root: Some(0),
                source_mode: Some(crate::audio_engine::pitch_actions::PitchKeyMode::Major),
                ..crate::audio_engine::input_mapping::PitchPadProjection::default()
            });
            if warm && ratio != 1.0 {
                select_and_wait(&mut fixture);
            }
            let epoch = Instant::now();
            let action = fixture.capture(0, "pitch.extra:0:set_retrigger");
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pitch_action(&action)
                    .unwrap()
            );
            fixture.drain();
            let admitted_us = epoch.elapsed().as_micros();
            let mut pcm = Vec::new();
            while fixture.transport.output_frame() < u64::from(RATE) / 2 {
                fixture.drain();
                pcm.extend(fixture.render(512));
                sleep_until(epoch, fixture.transport.output_frame(), RATE);
            }
            eprintln!(
                "response_piano ratio={ratio} warm={warm} capture_ns={} admission_us={admitted_us} T={} E={:?} landmarks={:?} state={} reason={:?}; public GUI-origin software only",
                action.received_timestamp(),
                action.original_due_frame(),
                action.effective().then_some(action.execution_target()),
                landmarks(&pcm, RATE),
                action.state(),
                action.reason()
            );
            assert!(action.effective(), "{:?}", action.reason());
        }
    }
}
