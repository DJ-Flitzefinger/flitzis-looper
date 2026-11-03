//! Actual controller targets and advancing artifact conversion; no neural/device claim.

use super::*;
use crate::audio_engine::pad_start::CapturedPadStart;

pub(super) fn script_value(f: &Fixture, source: &str) -> bool {
    Python::attach(|py| {
        let locals = PyDict::new(py);
        locals.set_item("probe", f.probe.bind(py)).unwrap();
        py.eval(&CString::new(source).unwrap(), Some(&locals), None)
            .unwrap()
            .extract()
            .unwrap()
    })
}

pub(super) struct ConversionSession(pub(super) Py<PyAny>);

impl Drop for ConversionSession {
    fn drop(&mut self) {
        Python::attach(|py| {
            if let Err(error) = self.0.call_method0(py, "finish") {
                eprintln!("controller-overlap worker cleanup/verification failed: {error}");
            }
        });
    }
}

pub(super) fn active(f: &mut Fixture, id: usize) -> bool {
    f.state()
        .mixer
        .voices
        .iter()
        .any(|v| v.is_playing_sample(id))
}

pub(super) fn settle(f: &mut Fixture, ids: &[usize]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        f.drain();
        f.render(512);
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()");
        if ids.iter().all(|id| active(f, *id)) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "controller start did not become audible"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn start(f: &mut Fixture, id: usize, multi: bool) -> CapturedPadStart {
    f.script(&format!(
        "try:\n    probe.transport.global_params.set_multi_loop(enabled={multi})\nexcept RuntimeError as error:\n    signature = probe.input_mapping._last_input_runtime_state_signature\n    cached = None if signature is None else [(i, None if row.binding is None else (row.binding.is_current(), row.binding.metadata())) for i, row in enumerate(signature[2]) if i < 3]\n    fresh = [(i, None if (binding := probe._audio.current_input_runtime_pad_binding(i)) is None else (binding.is_current(), binding.metadata())) for i in range(3)]\n    raise RuntimeError(str(error) + '; native=' + str(probe.input_mapping._last_input_runtime_state_error) + '; cached=' + repr(cached) + '; fresh=' + repr(fresh) + '; selected=' + repr(probe._audio.input_selected_projection()) + '; deferred=' + repr(probe.transport._pair_deferred_sample_ids)) from error\nprobe.transport.playback.trigger_pad({id})",
        multi = if multi { "True" } else { "False" }
    ));
    captured_start(f, id)
}

fn captured_start(f: &Fixture, id: usize) -> CapturedPadStart {
    Python::attach(|py| {
        let residency = f
            .probe
            .bind(py)
            .getattr("transport")
            .unwrap()
            .getattr("residency")
            .unwrap();
        let pending = residency.getattr("_captured_starts").unwrap();
        let values = pending.call_method0("values").unwrap();
        let starts = values
            .try_iter()
            .unwrap()
            .map(|p| {
                p.unwrap()
                    .getattr("start")
                    .unwrap()
                    .extract::<PyRef<CapturedPadStart>>()
                    .unwrap()
                    .clone()
            })
            .filter(|s| s.diagnostic_slot() == Some(id))
            .collect::<Vec<_>>();
        assert_eq!(
            starts.len(),
            1,
            "retain the original real controller capture"
        );
        starts[0].clone()
    })
}

#[test]
#[ignore = "explicit controller-to-Native target diagnostic; no device or timing claim"]
fn actual_global_off_preserves_explicit_target_across_selection() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut f = Fixture::new(root, false);
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap();
        module
            .call_method1(
                "write_source",
                (root.join("source.wav").to_string_lossy().as_ref(), RATE, 3),
            )
            .unwrap();
    });
    let path = root.join("source.wav");
    f.load(&path);
    for id in [1, 2] {
        f.script(&format!("probe.load({id}, {:?})", path.to_string_lossy()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !script_value(&f, &format!("probe.poll_load({id})")) {
            f.drain();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }
    f.drain();
    let source_image = |f: &mut Fixture, id: usize| {
        let source = f.state().mixer.bank_for_measurement()[id].as_ref().unwrap();
        assert_eq!(source.frame_count(), 3 * RATE as usize);
        assert_eq!(source.resident_start(), 0);
        assert_eq!(source.resident_end(), source.frame_count());
        assert_eq!(source.samples.len(), 2 * source.frame_count());
        let mut hash = Sha256::new();
        for value in source.samples.iter() {
            hash.update(value.to_le_bytes());
        }
        (
            source.samples.as_ptr() as usize,
            format!("{:x}", hash.finalize()),
        )
    };
    let sources = [0, 1, 2].map(|id| source_image(&mut f, id));
    f.script("probe.route_sources = tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(3))\nprobe.input_mapping.on_frame_render()");
    let original = start(&mut f, 0, false);
    let original_timestamp = original.received_timestamp();
    settle(&mut f, &[0]);
    assert_eq!(original.state(), "applied");
    assert_eq!(original.received_timestamp(), original_timestamp);
    assert!(!active(&mut f, 1) && !active(&mut f, 2));
    let old_generation = f.voice().generation;
    f.script("assert probe.session.active_sample_ids == {0}\nprobe.route_target = probe._audio.current_explicit_playback_target()\nassert probe.route_target.current_control_slot() == 0\nprobe.route_history = (probe.route_target.original_sequence(), probe.route_target.effective_frame(), probe.route_target.content_lifetime())\nprobe.select_pad(2)\nassert probe.project.selected_pad == 2\nassert probe._audio.input_selected_projection()[1] == 2\nassert not probe.project.multi_loop\nprobe.transport.playback.stop_global_start_stop()\nprobe.route_stop = probe.transport.playback._global_playback._fenced_batches[-1]\nassert probe.route_stop.publication_status() == 'pending'");
    f.drain();
    let stopped_pcm = f.render(512);
    f.call("poll_feedback");
    f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()\nassert probe.route_stop.publication_status() == 'accepted'\nassert not probe.session.active_sample_ids\nassert not probe.session.paused_sample_ids\nassert probe.route_target.current_control_slot() == 0\nprobe.route_after_stop = probe._audio.current_explicit_playback_target()\nassert probe.route_after_stop.current_control_slot() == 0\nassert (probe.route_after_stop.original_sequence(), probe.route_after_stop.effective_frame(), probe.route_after_stop.content_lifetime()) == probe.route_history\nprobe.select_pad(1)\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1\nprobe.transport.playback.start_or_restart_global_start_stop()");
    assert!(![0, 1, 2].into_iter().any(|id| active(&mut f, id)));
    assert!(stopped_pcm.iter().all(|sample| *sample == 0.0));
    // Retain the actual controller capture before any publication/poll removes it.
    let restart = captured_start(&f, 0);
    let timestamp = restart.received_timestamp();
    let sequence = restart.sequence();
    assert_ne!(sequence, original.sequence());
    assert_eq!(restart.content_key(), original.content_key());
    f.drain();
    let original_t = restart.original_frame().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut restart_pcm = Vec::new();
    let actual_e = loop {
        let frame = f.state().transport.output_frame();
        f.drain();
        restart_pcm.extend(f.render(1));
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()");
        assert!(!active(&mut f, 1) && !active(&mut f, 2));
        assert_eq!(restart.original_frame(), Some(original_t));
        assert_eq!(restart.received_timestamp(), timestamp);
        assert_eq!(restart.sequence(), sequence);
        if restart.state() == "applied" {
            assert!(active(&mut f, 0));
            break frame;
        }
        assert!(
            !active(&mut f, 0),
            "no voice before the original capture applies"
        );
        assert!(
            Instant::now() < deadline,
            "global explicit restart never applied"
        );
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(restart.execution_frame(), Some(actual_e));
    assert_eq!(restart.effective_frame(), Some(actual_e));
    let new_generation = f.voice().generation;
    assert_ne!(new_generation, old_generation);
    let reference = f
        .state()
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .source_playback
        .diagnostic_launch_reference()
        .unwrap()
        .1;
    assert_eq!(reference, original_t);
    restart_pcm.extend(f.render(512));
    assert!(restart_pcm.iter().all(|sample| sample.is_finite()));
    assert!(restart_pcm.iter().any(|sample| sample.abs() > 1e-6));
    f.call("poll_feedback");
    f.script(&format!("assert probe.session.active_sample_ids == {{0}}\nassert not probe.session.paused_sample_ids\nassert probe.project.selected_pad == 1\nassert probe._audio.input_selected_projection()[1] == 1\nprobe.route_after_start = probe._audio.current_explicit_playback_target()\nassert probe.route_after_start.current_control_slot() == 0\nassert probe.route_after_start.original_sequence() == {sequence}\nassert probe.route_after_start.effective_frame() == {actual_e}\nassert probe.route_after_start.content_lifetime() == probe.route_history[2]\nassert tuple((probe._audio.waveform_source_identity(i), probe._audio.loaded_source_digest(i), probe.source_history[i][-1]) for i in range(3)) == probe.route_sources\nassert all(probe._audio.current_input_runtime_pad_binding(i).is_current() for i in range(3))"));
    for (id, before) in sources.iter().enumerate() {
        assert_eq!(&source_image(&mut f, id), before);
    }
    f.script("probe.transport.playback.stop_all_pads()");
    f.drain();
    f.render(512);
    f.call("poll_feedback");
    assert!(![0, 1, 2].into_iter().any(|id| active(&mut f, id)));
    f.script("assert not probe.session.active_sample_ids");
    let joined_counts = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    assert_eq!(joined_counts, (0, 0, 0));
    f.close_callback();
    eprintln!(
        "global_explicit_raw {}",
        serde_json::json!({
            "rate": RATE, "frames_per_source": 3 * RATE, "channels": 2,
            "selected_stop": 2, "selected_restart": 1, "actual_target": 0,
            "original_sequence": original.sequence(), "restart_sequence": sequence,
            "restart_T": original_t, "restart_E": actual_e,
            "old_generation": old_generation, "new_generation": new_generation,
            "sources": sources, "joined_counts": joined_counts,
            "callback_retired": true, "stop_accepted": true,
            "limits": "One actual controller-to-Native functional bridge; no latency/device/NN/quality/Human or full-matrix acceptance"
        })
    );
}

#[test]
#[ignore = "explicit controller/conversion/load diagnostic; N1 software brackets only"]
fn actual_controller_routes_with_conversion_and_load() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut f = Fixture::new(root, false);
    let conversion = ConversionSession(Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../src/tests/flitzis_looper/controller/controller_overlap_probe.py"
            )))
            .unwrap(),
            c"controller_overlap_probe.py",
            c"controller_overlap_probe",
        )
        .unwrap();
        let path = root.join("source.wav");
        module
            .call_method1("write_source", (path.to_string_lossy().as_ref(), RATE, 3))
            .unwrap();
        module
            .call_method1(
                "write_source",
                (root.join("cold.wav").to_string_lossy().as_ref(), 44100, 60),
            )
            .unwrap();
        module
            .getattr("Conversion")
            .unwrap()
            .call1((
                root.to_string_lossy().as_ref(),
                py.get_type::<crate::audio_engine::stem_conversion::StemConversionReservation>(),
            ))
            .unwrap()
            .unbind()
    }));
    let path = root.join("source.wav");
    f.load(&path);
    for id in [1, 2] {
        f.script(&format!(
            "probe.load({id}, {:?})",
            path.to_string_lossy().as_ref()
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !script_value(&f, &format!("probe.poll_load({id})")) {
            f.drain();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }
    f.drain();
    f.script("probe.input_mapping.on_frame_render()\nprobe.select_pad(2)\nassert probe._audio.input_selected_projection()[1] == 2, probe.session.input_mapping_error\nassert not probe.session.active_sample_ids");
    let source_images = [0, 1, 2].map(|id| {
        let state = f.state();
        let source = state.mixer.bank_for_measurement()[id].as_ref().unwrap();
        assert_eq!(source.resident_start(), 0);
        assert_eq!(source.resident_end(), 3 * RATE as usize);
        assert_eq!(source.samples.len(), 3 * RATE as usize * 2);
        (source.samples.as_ptr() as usize, source.samples.len())
    });
    let peer = start(&mut f, 1, false);
    settle(&mut f, &[1]);
    assert_eq!(peer.state(), "applied");
    let exclusive = start(&mut f, 0, false);
    let timestamp = exclusive.received_timestamp();
    settle(&mut f, &[0]);
    assert!(!active(&mut f, 1));
    assert_eq!(exclusive.state(), "applied");
    assert_eq!(exclusive.received_timestamp(), timestamp);
    assert_eq!(exclusive.execution_frame(), exclusive.effective_frame());
    f.script("assert probe.session.active_sample_ids == {0}");
    let ordinary = start(&mut f, 1, true);
    settle(&mut f, &[0, 1]);
    assert_eq!(ordinary.state(), "applied");
    assert!(!active(&mut f, 2));
    let old_generations = [0, 1].map(|id| {
        f.state()
            .mixer
            .voices
            .iter()
            .find(|v| v.is_playing_sample(id))
            .unwrap()
            .generation
    });
    f.script("assert probe.session.active_sample_ids == {0, 1}\nprobe.transport.playback.start_or_restart_global_start_stop()\nprobe.route_group = (probe.transport.playback._global_playback._pending or probe.transport.playback._global_playback._deferred).capture\nassert set(probe.route_group.diagnostic_slots()) == {0, 1}\nassert probe.project.selected_pad == 2");
    f.drain();
    f.script("probe.route_group_t = probe.route_group.original_frame()\nassert probe.route_group_t is not None");
    let deadline = Instant::now() + Duration::from_secs(10);
    let global_effect = loop {
        let frame = f.state().transport.output_frame();
        f.drain();
        f.render(1);
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()");
        let changed = [0, 1].map(|id| {
            f.state()
                .mixer
                .voices
                .iter()
                .find(|v| v.is_playing_sample(id))
                .unwrap()
                .generation
                != old_generations[id]
        });
        if script_value(&f, "probe.route_group.state() == 'applied'") {
            assert_eq!(changed, [true, true]);
            break frame;
        }
        assert_eq!(changed, [false, false], "no early partial group effect");
        assert!(
            Instant::now() < deadline,
            "original controller group never applied"
        );
        thread::sleep(Duration::from_millis(1));
    };
    f.script("assert probe.route_group.original_frame() == probe.route_group_t\nassert set(probe.route_group.diagnostic_slots()) == {0, 1}");
    let births = [0, 1].map(|id| {
        let voice = f
            .state()
            .mixer
            .voices
            .iter()
            .find(|v| v.is_playing_sample(id))
            .unwrap();
        assert_ne!(voice.generation, old_generations[id]);
        voice
            .source_playback
            .diagnostic_launch_reference()
            .unwrap()
            .1
    });
    assert_eq!(
        births[0], births[1],
        "group voices retain the same original launch reference"
    );
    f.script(&format!("assert probe.route_group_t == {}", births[0]));
    assert!(!active(&mut f, 2));
    f.script(&format!(
        "probe.load(3, {:?})",
        root.join("cold.wav").to_string_lossy().as_ref()
    ));
    let began = Instant::now();
    Python::attach(|py| conversion.0.call_method0(py, "start").unwrap());
    let mut spans = Vec::new();
    let mut load_done = false;
    let mut load_brackets = Vec::new();
    for index in 0..96 {
        let target = Duration::from_secs_f64(index as f64 * 512.0 / f64::from(RATE));
        if target > began.elapsed() {
            thread::sleep(target - began.elapsed());
        }
        let before: (u64, usize, u64, usize) = Python::attach(|py| {
            conversion
                .0
                .call_method0(py, "snapshot")
                .unwrap()
                .extract(py)
                .unwrap()
        });
        let first = f.state().transport.output_frame();
        let entered = began.elapsed().as_nanos();
        f.drain();
        let pcm = f.render(512);
        let ended = began.elapsed().as_nanos();
        assert!(pcm.iter().all(|sample| sample.is_finite()));
        assert!(pcm.iter().any(|sample| sample.abs() > 1e-6));
        assert!(active(&mut f, 0) && active(&mut f, 1) && !active(&mut f, 2) && !active(&mut f, 3));
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()");
        let after: (u64, usize, u64, usize) = Python::attach(|py| {
            conversion
                .0
                .call_method0(py, "snapshot")
                .unwrap()
                .extract(py)
                .unwrap()
        });
        if !load_done {
            load_done = script_value(&f, "probe.poll_load(3)");
            load_brackets.push((first, began.elapsed().as_nanos(), load_done));
        }
        spans.push((first, entered, ended, before, after, load_done));
    }
    let finished = Python::attach(|py| conversion.0.call_method0(py, "finish").map(|_| ()));
    let calls: Vec<(u64, u64, u64, u64, usize, usize)> = Python::attach(|py| {
        conversion
            .0
            .getattr(py, "calls")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    let worker: (u64, u64, u64) = Python::attach(|py| {
        let value = conversion.0.bind(py);
        (
            value.getattr("started_ns").unwrap().extract().unwrap(),
            value.getattr("ended_ns").unwrap().extract().unwrap(),
            value.getattr("cpu_ns").unwrap().extract().unwrap(),
        )
    });
    let source_views = [0, 1, 2, 3].map(|id| {
        f.state().mixer.bank_for_measurement()[id]
            .as_ref()
            .map(|source| {
                (
                    source.frame_count(),
                    source.resident_start(),
                    source.resident_end(),
                    source.samples.as_ptr() as usize,
                    source.samples.len(),
                )
            })
    });
    eprintln!(
        "controller_raw calls={calls:?} spans={spans:?} worker_start_end_cpu_ns={worker:?} initial_source_views={source_images:?} final_source_views={source_views:?} load_observations={load_brackets:?} finish_ok={}; original raw observations before derived assertions",
        finished.is_ok()
    );
    let contained = spans
        .iter()
        .filter(|row| {
            calls
                .iter()
                .any(|call| call.0 >= row.3.0 && call.1 <= row.4.0 && call.3 > call.2)
        })
        .count();
    let playback_contained = spans
        .windows(2)
        .filter(|pair| {
            pair[1].0 == pair[0].0 + 512
                && pair[1].3.0 > pair[0].3.0
                && calls.iter().any(|call| {
                    call.0 >= pair[0].3.0 && call.1 <= pair[1].3.0 && call.3 > call.2 && call.5 > 0
                })
        })
        .count();
    let outputs: Vec<_> = ["vocals", "drums", "bass", "melody", "instrumental"]
        .into_iter()
        .filter_map(
            |name| match fs::read(root.join("generation").join(format!("{name}.wav"))) {
                Ok(bytes) => Some((name, bytes.len(), format!("{:x}", Sha256::digest(&bytes)))),
                Err(error) => {
                    eprintln!("controller_output_read_error name={name} error={error}");
                    None
                }
            },
        )
        .collect();
    eprintln!(
        "controller_conversion contained_playback_periods={playback_contained} contained_observer_brackets={contained} calls={calls:?} output_files={outputs:?}; complete guarded converter calls inside paced active playback periods, not per-render CPU attribution"
    );
    let advancing = spans
        .iter()
        .filter(|row| row.4.1 > row.3.1 && row.4.2 > row.3.2)
        .count();
    let sources: Vec<usize> = source_views
        .iter()
        .map(|view| view.map_or(0, |view| view.0))
        .collect();
    eprintln!(
        "controller_overlap routes=ordinary-exclusive,ordinary-multi,global-current active=2 selected=2 cold=3 conversion=actual-guarded-artifacts advancing_control_render_brackets={advancing} source_frames={sources:?} global_original_references={births:?} global_E={global_effect} exclusive_T={:?} exclusive_E={:?} ordinary_T={:?} ordinary_E={:?} spans={spans:?} load_observations={load_brackets:?}; N1 whole-control-render-brackets include bridge/alloc/poll; cold-import admission/later-success not exact load CPU overlap; no inference/device/quality/Human claim",
        exclusive.original_frame(),
        exclusive.effective_frame(),
        ordinary.original_frame(),
        ordinary.effective_frame()
    );
    finished.unwrap();
    assert_eq!(outputs.len(), 5);
    assert!(
        load_done,
        "real cold Loader must finish and pass original-source checks"
    );
    assert!(
        spans
            .windows(2)
            .all(|pair| pair[1].0 == pair[0].0 + 512 && pair[1].3.0 > pair[0].3.0)
    );
    assert!(
        playback_contained > 0,
        "a complete guarded converter call with positive worker CPU must occur inside a consecutive active paced playback period"
    );
    for (id, view) in source_views.into_iter().enumerate() {
        let view = view.expect("all actual sources must remain loaded");
        assert_eq!(view.0, if id == 3 { 60 } else { 3 } * RATE as usize);
        if id < 3 {
            assert_eq!((view.3, view.4), source_images[id]);
            assert_eq!((view.1, view.2), (0, 3 * RATE as usize));
        }
    }
    f.script("probe.transport.playback.stop_all_pads()");
    f.drain();
    f.render(512);
    f.call("poll_feedback");
    assert!(!active(&mut f, 0) && !active(&mut f, 1));
    Python::attach(|py| {
        f.engine.borrow_mut(py).shut_down().unwrap();
    });
    f.close_callback();
}
