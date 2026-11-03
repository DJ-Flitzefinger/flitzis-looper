//! Actual Loader-worker CPU counters while two complete sources keep playing.

use super::controller_overlap::{active, script_value, settle, start};
use super::*;
use crate::audio_engine::loader_overlap_observation::{Observation, Scope, current_worker};

fn source_image(f: &mut Fixture, id: usize) -> (usize, usize, usize, usize, String) {
    let source = f.state().mixer.bank_for_measurement()[id].as_ref().unwrap();
    let mut hash = Sha256::new();
    for value in source.samples.iter() {
        hash.update(value.to_le_bytes());
    }
    (
        source.samples.as_ptr() as usize,
        source.frame_count(),
        source.resident_start(),
        source.resident_end(),
        format!("{:x}", hash.finalize()),
    )
}

#[test]
#[ignore = "explicit Loader-worker CPU diagnostic; paced software periods, no device"]
fn actual_loader_cpu_during_active_playback() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    // The observer outlives Fixture even on an assertion/unwind; Fixture joins
    // actual cold workers before its callback and source directory retire.
    let observation = Observation::new();
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
        for (name, rate, seconds) in [("source.wav", RATE, 3), ("cold.wav", 44100, 60)] {
            module
                .call_method1(
                    "write_source",
                    (root.join(name).to_string_lossy().as_ref(), rate, seconds),
                )
                .unwrap();
        }
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
    f.script("probe.input_mapping.on_frame_render()\nprobe.select_pad(2)\nassert probe._audio.input_selected_projection()[1] == 2");
    let first_start = start(&mut f, 0, true);
    settle(&mut f, &[0]);
    let starts = [first_start, start(&mut f, 1, true)];
    settle(&mut f, &[0, 1]);
    assert!(starts.iter().all(|capture| capture.state() == "applied"));
    let initial = [0, 1, 2].map(|id| source_image(&mut f, id));
    let render_worker = current_worker();
    let began = Instant::now();
    {
        let _scope = Scope::install(Some(observation.clone()));
        f.script(&format!(
            "probe.load(3, {:?})\nprobe.loader.set_incoming_loop_intent(3, probe.pending_requests[3], start_s=0.0, end_s=None, auto=False, bars=8.0)",
            root.join("cold.wav").to_string_lossy()
        ));
    }
    let request: u64 = Python::attach(|py| {
        f.probe
            .bind(py)
            .getattr("pending_requests")
            .unwrap()
            .get_item(3)
            .unwrap()
            .extract()
            .unwrap()
    });
    let mut periods = Vec::new();
    let mut load_done = false;
    for index in 0..96 {
        let target = Duration::from_secs_f64(index as f64 * 512.0 / f64::from(RATE));
        if target > began.elapsed() {
            thread::sleep(target - began.elapsed());
        }
        let first = f.state().transport.output_frame();
        let entered = observation.epoch.elapsed().as_nanos();
        f.drain();
        let pcm = f.render(512);
        let ended = observation.epoch.elapsed().as_nanos();
        let voices = [0, 1, 2, 3].map(|id| active(&mut f, id));
        let finite = pcm.iter().all(|value| value.is_finite());
        let nonquiet = pcm.iter().any(|value| value.abs() > 1e-6);
        f.call("poll_feedback");
        f.script("probe.transport.on_frame_render()\nprobe.input_mapping.on_frame_render()");
        if !load_done {
            load_done = script_value(&f, "probe.poll_load(3)");
        }
        periods.push((first, entered, ended, voices, finite, nonquiet, load_done));
    }
    let adopted_all = script_value(
        &f,
        "probe.project.pad_loop_auto[3] is False and probe.project.pad_loop_start_s[3] == 0.0 and probe.project.pad_loop_end_s[3] is None",
    );
    let final_sources = [0, 1, 2, 3].map(|id| {
        let present = f.state().mixer.bank_for_measurement()[id].is_some();
        present.then(|| source_image(&mut f, id))
    });
    f.script("probe.transport.playback.stop_all_pads()");
    f.drain();
    f.render(512);
    f.call("poll_feedback");
    let stopped = !active(&mut f, 0) && !active(&mut f, 1);
    let joined_counts = Python::attach(|py| {
        let mut engine = f.engine.borrow_mut(py);
        engine.shut_down().unwrap();
        engine.cold_jobs.counts_for_test()
    });
    f.close_callback();
    let points = observation.points();
    let witnesses: Vec<_> = periods
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            points.windows(2).enumerate().find_map(|(point, samples)| {
                let [left, right] = samples else {
                    unreachable!()
                };
                (pair[1].0 == pair[0].0 + 512
                    && pair[0].3 == [true, true, false, false]
                    && pair[1].3 == pair[0].3
                    && left.before_ns >= pair[0].1
                    && right.after_ns <= pair[1].1
                    && left.pad == 3
                    && right.pad == 3
                    && left.request == request
                    && right.request == request
                    && left.stage == right.stage
                    && matches!(left.stage, "decoding" | "resampling")
                    && right.percent > left.percent
                    && left.worker.id == right.worker.id
                    && left.worker.birth_100ns == right.worker.birth_100ns
                    && left.worker.id != render_worker.id
                    && right.worker.cpu_ns > left.worker.cpu_ns)
                    .then_some((index, point))
            })
        })
        .collect();
    // Preserve all observations and original CPU values before interpreting any
    // overlap assertion. Pacing, I/O, cancellation checks, ordinary progress
    // reporting and observer overhead are included, not isolated decoder CPU.
    eprintln!(
        "loader_cpu_raw {}",
        serde_json::json!({
            "points": points, "periods": periods, "witnesses": witnesses,
            "request": request, "render_worker": render_worker,
            "initial_sources": initial, "final_sources": final_sources,
            "load_done": load_done, "adopted_all": adopted_all,
            "stopped": stopped, "joined_counts": joined_counts,
            "limits": "N1 observed Loader-worker CPU counter progress within paced active playback periods; includes reporting/observer/IO; no isolated decoder/per-render CPU, device deadline, NN, quality or Human acceptance"
        })
    );
    assert!(
        load_done,
        "original Loader source ACK/digest must be checked"
    );
    assert!(
        adopted_all,
        "the same incoming request must adopt explicit ALL"
    );
    assert!(stopped);
    assert_eq!(joined_counts, (0, 0, 0));
    assert!(!points.is_empty(), "observe the actual admitted Loader job");
    assert!(points.iter().all(|point| {
        point.pad == 3 && point.request == request && point.worker.id != render_worker.id
    }));
    assert!(
        periods
            .iter()
            .all(|row| row.3 == [true, true, false, false] && row.4 && row.5)
    );
    assert!(periods.windows(2).all(|pair| pair[1].0 == pair[0].0 + 512));
    assert!(
        !witnesses.is_empty(),
        "actual progressing Loader producer interval with positive same-worker CPU must be wholly inside an active consecutive playback period"
    );
    for (id, source) in final_sources.iter().enumerate() {
        let source = source.as_ref().unwrap();
        assert_eq!(source.1, if id == 3 { 60 } else { 3 } * RATE as usize);
        assert_eq!((source.2, source.3), (0, source.1));
        if id < 3 {
            assert_eq!(source, &initial[id]);
        }
    }
}
