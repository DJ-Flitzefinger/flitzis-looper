//! Complete Loader cohorts and repeated public groups beside real conversion.
//! The ten-PAD cohort is synthetic stress, separate from the productive six-PAD target.

use super::controller_overlap::{ConversionSession, active, script_value, settle, start};
use super::*;
use crate::audio_engine::CapturedGlobalStart;
use crate::audio_engine::c3_process_metrics;
use crate::audio_engine::source_playback::SourcePlayback;

type ConversionCall = (u64, u64, u64, u64, usize, usize);
type ConversionSnapshot = (u64, usize, u64, usize);

pub(super) fn sources<const N: usize>(
    f: &mut Fixture,
    pads: &[usize; N],
) -> [(usize, usize, String); N] {
    pads.map(|id| {
        let source = f.state().mixer.bank_for_measurement()[id].as_ref().unwrap();
        assert_eq!(source.frame_count(), 3 * RATE as usize);
        assert_eq!(
            (source.resident_start(), source.resident_end()),
            (0, source.frame_count())
        );
        assert_eq!(source.channels, 2);
        assert_eq!(source.samples.len(), source.frame_count() * 2);
        let mut hash = Sha256::new();
        for value in source.samples.iter() {
            hash.update(value.to_le_bytes());
        }
        (
            source.samples.as_ptr() as usize,
            source.samples.len(),
            format!("{:x}", hash.finalize()),
        )
    })
}

pub(super) fn voices<const N: usize>(
    f: &mut Fixture,
    pads: &[usize; N],
) -> [(u64, usize, usize, SourcePlayback); N] {
    pads.map(|id| {
        let voice = f
            .state()
            .mixer
            .voices
            .iter()
            .find(|v| v.is_playing_sample(id))
            .unwrap();
        assert!(!voice.paused && voice.volume > 0.0);
        (
            voice.generation,
            voice.sample.as_ref().unwrap().samples.as_ptr() as usize,
            voice.stretch.native_state_address(),
            voice.source_playback,
        )
    })
}

fn poll(f: &Fixture) {
    f.call("poll_feedback");
    f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()");
}

pub(super) fn stop(f: &mut Fixture, pads: &[usize]) {
    // MultiLoop ON retains this original in _pending, not _fenced_batches.
    f.script("probe.transport.playback.stop_global_start_stop()\nprobe.conversion_stop = probe.transport.playback._global_playback._pending.ticket\nassert probe.conversion_stop.publication_status() == 'pending'");
    f.drain();
    let pcm = f.render(512);
    assert!(pcm.iter().all(|value| *value == 0.0));
    assert!(pads.iter().all(|id| !active(f, *id)));
    poll(f);
    f.script("assert probe.conversion_stop.publication_status() == 'accepted'\nassert not probe.session.active_sample_ids\nassert not probe.session.paused_sample_ids\nassert probe.session.global_stop_restore_sample_ids == set(range(probe.conversion_pad_count))\nassert probe.session.global_stop_engaged");
}

fn snapshot(conversion: &ConversionSession) -> ConversionSnapshot {
    Python::attach(|py| {
        conversion
            .0
            .call_method0(py, "snapshot")
            .unwrap()
            .extract(py)
            .unwrap()
    })
}

#[test]
#[ignore = "explicit six-PAD repeated controller/conversion diagnostic; no device"]
fn actual_six_pad_repeated_start_stop_with_conversion() {
    run::<6>("six_pad_conversion", false);
}

#[test]
#[ignore = "explicit ten-PAD synthetic overlap/settlement diagnostic; no device"]
fn actual_ten_pad_synthetic_restarts_with_conversion() {
    run::<10>("ten_pad_conversion", false);
}

#[test]
#[ignore = "explicit fixed-storage six-PAD settlement diagnostic; no device"]
fn actual_six_pad_settlement_after_diagnostic_release() {
    run::<6>("six_pad_settlement", true);
}

#[derive(Clone, Copy, Default, serde::Serialize)]
struct SettlementCycle {
    cycle: usize,
    original_frame: u64,
    effect_frame: u64,
    conversion_calls: usize,
    overlap_witnesses: usize,
    readers_opened: usize,
    readers_closed: usize,
    metrics: c3_process_metrics::SettlementSnapshot,
}

fn run<const N: usize>(label: &str, settlement: bool) {
    assert!(N == 6 || N == 10);
    assert!(!settlement || N == 6);
    // Exactly one warmup and five measured cycles, declared before loading.
    // No variable evidence is retained in this result array or serialized until
    // after the final measurement. Historical functional rows stay separate.
    let mut settled = [SettlementCycle::default(); 6];
    let pads: [usize; N] = std::array::from_fn(|id| id);
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut f = Fixture::new(root, false);
    f.script(&format!("probe.conversion_pad_count = {N}"));
    let module = Python::attach(|py| {
        PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap()
        .unbind()
    });
    for id in pads {
        let path = root.join(format!("source-{id}.wav"));
        Python::attach(|py| {
            module
                .call_method1(
                    py,
                    "write_source",
                    (path.to_string_lossy().as_ref(), RATE, 3, 1000 + id * 101),
                )
                .unwrap();
        });
        f.script(&format!("probe.load({id}, {:?})", path.to_string_lossy()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !script_value(&f, &format!("probe.poll_load({id})")) {
            f.drain();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }
    f.drain();
    let original_sources = sources(&mut f, &pads);
    let markers = pads.map(|id| f.state().mixer.loop_region_frames(id));
    f.script("probe.conversion_sources = tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(probe.conversion_pad_count))\nprobe.input_mapping.on_frame_render()\nprobe.select_pad(probe.conversion_pad_count - 1)");
    let mut source_keys = Vec::new();
    for id in pads {
        let capture = start(&mut f, id, true);
        settle(&mut f, &pads[..=id]);
        assert_eq!(capture.state(), "applied");
        source_keys.push(capture.content_key());
    }
    let mut previous_generations = voices(&mut f, &pads).map(|voice| voice.0);
    stop(&mut f, &pads);
    let initial_metrics = (!settlement).then(c3_process_metrics::snapshot);
    let initial_settlement = settlement.then(c3_process_metrics::settlement_snapshot);
    let mut rows = Vec::new();
    let mut previous_sequences = [0; N];
    for (cycle, settled_cycle) in settled.iter_mut().enumerate() {
        // Every variable Rust/Python cycle owner dies at this scope boundary.
        let mut summary = {
            let conversion = ConversionSession(Python::attach(|py| {
                module.getattr(py, if settlement { "SettlementConversion" } else { "Conversion" }).unwrap().call1(py, (
                root.join(format!("cycle-{cycle}")).to_string_lossy().as_ref(),
                py.get_type::<crate::audio_engine::stem_conversion::StemConversionReservation>(),
            )).unwrap()
            }));
            Python::attach(|py| conversion.0.call_method0(py, "start").unwrap());
            // Wait only for actual productive worker entry, never park its work.
            let deadline = Instant::now() + Duration::from_secs(10);
            while Python::attach(|py| {
                conversion
                    .0
                    .getattr(py, "started_ns")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap()
            }) == 0
            {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            let unfinished_at_start = Python::attach(|py| {
                !conversion
                    .0
                    .getattr(py, "future")
                    .unwrap()
                    .call_method0(py, "done")
                    .unwrap()
                    .extract::<bool>(py)
                    .unwrap()
            });
            f.script("probe.transport.playback.start_or_restart_global_start_stop()\nprobe.conversion_capture = (probe.transport.playback._global_playback._pending or probe.transport.playback._global_playback._deferred).capture\nassert probe.conversion_capture.diagnostic_slots() == list(range(probe.conversion_pad_count))");
            // Public start may already publish. Retain the original owners before
            // the first subsequent callback drain/render or controller poll.
            let capture = Python::attach(|py| {
                f.probe
                    .bind(py)
                    .getattr("conversion_capture")
                    .unwrap()
                    .extract::<PyRef<CapturedGlobalStart>>()
                    .unwrap()
                    .clone()
            });
            let members = capture.diagnostic_members();
            assert_eq!(members.len(), N);
            let originals = members
                .iter()
                .enumerate()
                .map(|(id, member)| {
                    assert_eq!(member.diagnostic_slot(), Some(id));
                    assert_eq!(member.content_key(), source_keys[id]);
                    assert!(member.sequence() > previous_sequences[id]);
                    previous_sequences[id] = member.sequence();
                    (
                        member.sequence(),
                        member.content_key(),
                        member.received_timestamp(),
                    )
                })
                .collect::<Vec<_>>();
            let received = capture.received_timestamp();
            f.drain();
            let original_t = capture.original_frame().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let effect = loop {
                let frame = f.state().transport.output_frame();
                f.drain();
                f.render(1);
                poll(&f);
                assert_eq!(capture.original_frame(), Some(original_t));
                assert_eq!(capture.received_timestamp(), received);
                if capture.state() == "applied" {
                    assert!(pads.iter().all(|id| active(&mut f, *id)));
                    break frame;
                }
                assert!(
                    pads.iter().all(|id| !active(&mut f, *id)),
                    "no partial group before E"
                );
                assert_ne!(capture.state(), "rejected", "{:?}", capture.reason());
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            };
            let births = voices(&mut f, &pads);
            for (id, member) in members.iter().enumerate() {
                assert_eq!(
                    (
                        member.sequence(),
                        member.content_key(),
                        member.received_timestamp()
                    ),
                    originals[id]
                );
                assert_eq!(member.received_timestamp(), received);
                assert_eq!(member.original_frame(), Some(original_t));
                assert_eq!(member.execution_frame(), Some(effect));
                assert_eq!(member.effective_frame(), Some(effect));
                assert_eq!(
                    births[id].3.diagnostic_launch_reference().unwrap().1,
                    original_t
                );
                assert!(births[id].0 > previous_generations[id]);
                previous_generations[id] = births[id].0;
                assert_eq!(births[id].1, original_sources[id].0);
            }
            let began = Instant::now();
            let mut spans = Vec::new();
            let mut contributions = [0; N];
            for period in 0..96 {
                let target = Duration::from_secs_f64(period as f64 * 512.0 / f64::from(RATE));
                if target > began.elapsed() {
                    thread::sleep(target - began.elapsed());
                }
                let before = snapshot(&conversion);
                let first = f.state().transport.output_frame();
                let mut expected = voices(&mut f, &pads);
                for voice in &mut expected {
                    assert!(voice.3.advance(512));
                }
                f.drain();
                let pcm = f.render(512);
                let after = snapshot(&conversion);
                let actual = voices(&mut f, &pads);
                let peaks = pads.map(|id| f.state().pad_peaks[id]);
                for id in pads {
                    assert_eq!(
                        (actual[id].0, actual[id].1, actual[id].2),
                        (expected[id].0, expected[id].1, expected[id].2)
                    );
                    assert!(actual[id].3.matches_exact(&expected[id].3));
                    assert!(
                        peaks[id].is_finite() && peaks[id] > 1e-6,
                        "actual PAD {id} post-DSP contribution"
                    );
                    contributions[id] += 1;
                }
                assert!(pcm.iter().all(|value| value.is_finite()));
                assert!(pcm.iter().any(|value| value.abs() > 1e-6));
                poll(&f);
                f.script("assert probe.session.active_sample_ids == set(range(probe.conversion_pad_count))\nassert not probe.session.paused_sample_ids");
                spans.push((first, before, after, peaks.to_vec()));
            }
            stop(&mut f, &pads);
            Python::attach(|py| conversion.0.call_method0(py, "finish").unwrap());
            let (calls, worker): (Vec<ConversionCall>, (u64, u64, u64)) = Python::attach(|py| {
                let value = conversion.0.bind(py);
                (
                    value.getattr("calls").unwrap().extract().unwrap(),
                    (
                        value.getattr("started_ns").unwrap().extract().unwrap(),
                        value.getattr("ended_ns").unwrap().extract().unwrap(),
                        value.getattr("cpu_ns").unwrap().extract().unwrap(),
                    ),
                )
            });
            let witnesses = spans
                .windows(2)
                .enumerate()
                .filter_map(|(period, pair)| {
                    assert_eq!(pair[1].0, pair[0].0 + 512);
                    calls.iter().enumerate().find_map(|(index, call)| {
                        (call.0 >= pair[0].1.0
                            && call.1 <= pair[1].1.0
                            && call.3 > call.2
                            && call.5 > 0)
                            .then_some((period, index))
                    })
                })
                .collect::<Vec<_>>();
            let output_hashes = ["vocals", "drums", "bass", "melody", "instrumental"].map(|name| {
                let bytes =
                    fs::read(root.join(format!("cycle-{cycle}/generation/{name}.wav"))).unwrap();
                (name, bytes.len(), format!("{:x}", Sha256::digest(&bytes)))
            });
            assert_eq!(sources(&mut f, &pads), original_sources);
            assert_eq!(
                pads.map(|id| f.state().mixer.loop_region_frames(id)),
                markers
            );
            f.script("assert tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(probe.conversion_pad_count)) == probe.conversion_sources\nassert all(probe._audio.current_input_runtime_pad_binding(i).is_current() for i in range(probe.conversion_pad_count))\nassert probe.project.selected_pad == probe.conversion_pad_count - 1\nassert probe._audio.input_selected_projection()[1] == probe.conversion_pad_count - 1\ndel probe.conversion_capture\ndel probe.conversion_stop");
            let counts = Python::attach(|py| f.engine.borrow(py).cold_jobs.counts_for_test());
            assert!(unfinished_at_start);
            assert!(
                !witnesses.is_empty(),
                "complete advancing converter call inside consecutive all-PAD-active playback periods"
            );
            assert_eq!(contributions, [96; N]);
            assert_eq!(
                counts,
                (0, 0, 0),
                "work settlement, not persistent worker END"
            );
            assert!(worker.1 > worker.0 && worker.2 > 0);
            let (readers_opened, readers_closed) = if settlement {
                Python::attach(|py| {
                    let value = conversion.0.bind(py);
                    assert_eq!(
                        value
                            .getattr("live_readers")
                            .unwrap()
                            .extract::<usize>()
                            .unwrap(),
                        0
                    );
                    (
                        value
                            .getattr("readers_opened")
                            .unwrap()
                            .extract::<usize>()
                            .unwrap(),
                        value
                            .getattr("readers_closed")
                            .unwrap()
                            .extract::<usize>()
                            .unwrap(),
                    )
                })
            } else {
                (0, 0)
            };
            // Py<T> decrefs must execute under the GIL, rather than remain deferred
            // until a later Python attach beyond the measurement boundary.
            Python::attach(|_| {
                drop(members);
                drop(capture);
                drop(conversion);
            });
            let summary = SettlementCycle {
                cycle,
                original_frame: original_t,
                effect_frame: effect,
                conversion_calls: calls.len(),
                overlap_witnesses: witnesses.len(),
                readers_opened,
                readers_closed,
                ..SettlementCycle::default()
            };
            if !settlement {
                let metrics = c3_process_metrics::snapshot();
                let row = serde_json::json!({"cycle":cycle,"warmup":cycle==0,"T":original_t,"E":effect,
                "received_ns":received,"sequences":previous_sequences.as_slice(),"contributions":contributions.as_slice(),
                "unfinished_at_start":unfinished_at_start,"spans":spans,"calls":calls,"worker":worker,
                "witnesses":witnesses,"outputs":output_hashes,"cold_job_counts":counts,
                "conversion_executor_joined":true,"full_output_oracle":true,"settled_metrics":metrics});
                eprintln!("{label}_cycle_raw {row}");
                rows.push(row);
            }
            summary
        };
        if settlement {
            summary.metrics = c3_process_metrics::settlement_snapshot();
            *settled_cycle = summary;
        }
    }
    let joined_counts = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    assert_eq!(joined_counts, (0, 0, 0));
    f.close_callback();
    f.call("close");
    if settlement {
        Python::attach(|_| drop(f));
    } else {
        drop(f);
    }
    if settlement {
        let final_metrics = c3_process_metrics::settlement_snapshot();
        eprintln!(
            "{label}_raw {}",
            serde_json::json!({
                "pads":N,"cycles":6,"warmup_cycles":1,"measured_cycles":5,
                "periods_per_cycle":96,"frames_per_period":512,"frames_per_source":3*RATE,"channels":2,
                "initial_stopped_fixture":initial_settlement,"stopped_fixture_cycles":settled,
                "measured_baseline_cycle":0,"after_final_pool_join_and_callback_retirement":final_metrics,
                "joined_counts":joined_counts,"callback_retired":true,
                "limits":"Fixed scalar result storage allocated before loading. Cycle transients and converter Python owners released before every stopped-fixture sample; real PCM reader context exits and executor joins checked. Allocator retention may remain. Final teardown is a different phase. Descriptive samples only, no arbitrary no-growth, leak absence, general plateau, finite Native/appwide bound, device, latency, neural, quality or Human acceptance."
            })
        );
        return;
    }
    let final_metrics = c3_process_metrics::snapshot();
    eprintln!(
        "{label}_raw {}",
        serde_json::json!({
            "pads":N,"synthetic_stress":N==10,"cycles":6,"warmup_cycles":1,"frames_per_source":3*RATE,"channels":2,
            "sources":original_sources.as_slice(),"rows":rows,"initial_metrics":initial_metrics,"final_metrics":final_metrics,
            "joined_counts":joined_counts,"callback_retired":true,
            "limits":"One software cohort history; per-PAD post-DSP peaks and exact trajectories, not independently reconstructed stereo PCM. Metrics are observed samples only, no finite Native/appwide bound, device deadline, latency, neural, quality, Human or full-matrix acceptance."
        })
    );
}
