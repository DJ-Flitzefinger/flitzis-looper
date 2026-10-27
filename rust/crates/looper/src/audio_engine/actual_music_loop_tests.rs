//! Opt-in real music through productive import, custody, launch and scheduled reads.
//! Retained NN observations are a diagnostic oracle, not accepted timing or hearing.
use super::*;
use crate::audio_engine::audio_stream::{drain_parameter_messages, render_scheduled_audio};
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::sync_diagnostics::{HEADER_WORDS, ROW_WORDS};
use crate::audio_engine::{NUM_SAMPLES, resident_relocation};
use crate::messages::ControlParameterMessage;
use pyo3::types::PyModule;

mod maps {
    use super::*;
    include!("actual_music_map_tests.rs");
}

mod mapped_playback {
    use super::*;
    include!("actual_mapped_music_playback_tests.rs");
}

struct Path {
    engine: AudioEngine,
    callback: Callback,
    consumer: rtrb::Consumer<ControlMessage>,
    parameters: rtrb::Consumer<ControlParameterMessage>,
}

impl Path {
    fn new() -> Self {
        let mut engine = AudioEngine::new().unwrap();
        assert!(engine.stream_handle.is_none());
        engine.test_output_format = Some((48_000, 2));
        let previous = old(&engine);
        let callback = Callback::new(&engine, &previous);
        let (producer, consumer) = rtrb::RingBuffer::new(32);
        let (_feedback, feedback_consumer) = rtrb::RingBuffer::<AudioMessage>::new(32);
        engine.test_control_channels = Some((
            Arc::new(Mutex::new(producer)),
            Arc::new(Mutex::new(feedback_consumer)),
        ));
        let (producer, parameters) = rtrb::RingBuffer::new(32);
        engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
        Self {
            engine,
            callback,
            consumer,
            parameters,
        }
    }

    fn drain(&mut self) {
        drain_parameter_messages(
            &mut self.parameters,
            &mut self.callback.mixer,
            &mut self.callback.transport,
        );
        self.callback.drain(&mut self.consumer);
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        let frame = self.callback.transport.output_frame();
        render_scheduled_audio(
            &mut self.callback.mixer,
            &mut self.callback.scheduler,
            &mut out,
            &mut [0.0; NUM_SAMPLES],
            frame,
            2,
            &mut self.callback.transport,
            &mut self.callback.feedback,
            &mut self.callback.retirement,
        );
        self.callback.transport.advance_by_rendered_frames(frames);
        out
    }

    fn generation(&self, id: usize) -> u64 {
        self.engine
            .current_input_runtime_pad_binding(id)
            .unwrap()
            .unwrap()
            .source_generation
    }

    fn wait_start(&mut self, captured: &CapturedPadStart) {
        let frame = self.callback.transport.output_frame();
        wait_until(|| {
            self.drain();
            assert_ne!(captured.state(), "rejected", "{:?}", captured.reason());
            if captured.state() == "applied" {
                return true;
            }
            let runtime = self.callback.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            runtime.has_ready_action(captured.sequence())
        });
        assert_eq!(self.callback.transport.output_frame(), frame);
    }
}

fn neural_phase(source: &Value, seconds: f64) -> Option<f64> {
    let beats = source["beats"].as_array()?;
    let next = beats.partition_point(|v| v.as_f64().unwrap() <= seconds);
    if next == 0 || next == beats.len() {
        return None;
    }
    let (a, b) = (beats[next - 1].as_f64()?, beats[next].as_f64()?);
    let period = 60.0 / source["bpm"].as_f64()?;
    // No interpolation through a missing-count/extra-event-sized interval.
    if ((b - a) / period - 1.0).abs() > 0.15 {
        return None;
    }
    Some((next - 1) as f64 + (seconds - a) / (b - a))
}

fn complete_neural_coverage(source: &Value, start: f64, end: f64) -> bool {
    let Some(beats) = source["beats"].as_array() else {
        return false;
    };
    let period = 60.0 / source["bpm"].as_f64().unwrap();
    neural_phase(source, start).is_some()
        && neural_phase(source, end).is_some()
        && beats
            .windows(2)
            .filter(|pair| pair[1].as_f64().unwrap() > start && pair[0].as_f64().unwrap() < end)
            .all(|pair| {
                ((pair[1].as_f64().unwrap() - pair[0].as_f64().unwrap()) / period - 1.0).abs()
                    <= 0.15
            })
}

fn snapshot_json(path: &Path, request: u64) -> Value {
    Python::attach(|py| {
        let snapshot = path
            .engine
            .sync_diagnostic_snapshot(py, request)
            .unwrap()
            .unwrap();
        let wire: String = PyModule::import(py, "json")
            .unwrap()
            .call_method1("dumps", (snapshot,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&wire).unwrap()
    })
}

#[test]
#[ignore = "explicit source-bound authorized music; headless real 8/32/64-bar rendering"]
fn authorized_music_8_against_32_and_64_bars() {
    const TEST: &str = "audio_engine::cold_load::tests::automatic_analysis_tests::actual_music_loop_tests::authorized_music_8_against_32_and_64_bars";
    if std::env::var_os(CHILD).is_none() {
        child(TEST, 48_000, true);
        return;
    }
    Python::initialize();
    let root = PathBuf::from(std::env::var_os("FLITZI_MUSIC_LOOP_ROOT").expect("owned root"));
    let input_path =
        PathBuf::from(std::env::var_os("FLITZI_MUSIC_LOOP_INPUTS").expect("explicit inputs"));
    assert!(root.is_absolute() && input_path.is_absolute());
    let input_bytes = fs::read(&input_path).unwrap();
    let inputs: Value = serde_json::from_slice(&input_bytes).unwrap();
    let sources = inputs["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    let (_directory, _cwd) = owned_directory_at(&root.join("tmp"));
    let mut path = Path::new();
    let mut source_events = Vec::new();
    let mut fingerprints = Vec::new();
    let mut oracle = [false; 2];
    for (id, source) in sources.iter().enumerate() {
        assert_eq!(source["pad_id"], id);
        let original = PathBuf::from(source["path"].as_str().unwrap());
        let expected = source["source_sha256"].as_str().unwrap();
        assert_eq!(
            expected,
            [
                "339911dd7a54af5efc80fdcc05e1432648de9640348803e9b4c0657f59c1a5ea",
                "fadd1534a3a51cf6de7fcd4d3ebefdc3e596a38c451e3c71cb6dc8c9350f8497",
            ][id]
        );
        assert!(original.is_absolute());
        assert_eq!(hash_file(&original), expected);
        let request = path
            .engine
            .load_sample_async(
                id,
                original.to_str().unwrap().into(),
                Some(false),
                false,
                true,
                None,
                None,
                false,
                "import",
            )
            .unwrap();
        wait_until_for(Duration::from_secs(120), || {
            path.drain();
            path.engine.input_runtime_ownership.cold_status(id, request) == Some(2)
        });
        let event = terminal(&path.engine, request);
        assert!(
            matches!(&event, LoaderEvent::Success { id: actual, analysis: None, .. } if *actual == id)
        );
        source_events.push(event); // Retain actual original leases through all reads.
        let leases = path.engine.cold_leases.lock().unwrap();
        let lease = leases[id].as_ref().unwrap();
        assert_eq!(hash_file(&lease.original_path), expected);
        let playback = &lease.manifest.descriptor["playback"]["pcm"];
        let decoder = &lease.manifest.descriptor["decoder"]["pcm"];
        assert_eq!(
            hash_file(&lease.cache_path.join("playback.f32le")),
            playback["interleaved_sha256"]
        );
        assert_eq!(
            hash_file(&lease.cache_path.join("decoder.f32le")),
            decoder["interleaved_sha256"]
        );
        let expected_pcm = &source["expected_playback_pcm"];
        let mandatory = [
            "rate_hz",
            "channels",
            "full_frames",
            "interleaved_sha256",
            "mono_sha256",
            "source_zero_bits",
        ];
        oracle[id] = mandatory.iter().all(|key| !expected_pcm[key].is_null())
            && source["expected_playback_pcm"]
                .as_object()
                .unwrap()
                .iter()
                .all(|(key, value)| &playback[key] == value)
            && (source["expected_decoder_pcm"].is_null()
                || &source["expected_decoder_pcm"] == decoder);
        fingerprints.push(
            json!({"pad_id":id,"source_sha256":expected,"decoder":decoder,
            "playback":playback,"retained_nn_oracle_available":oracle[id]}),
        );
    }
    path.engine.stop_all().unwrap();
    path.drain();
    for (id, source) in sources.iter().enumerate() {
        path.engine
            .set_pad_bpm_with_source(
                id,
                Some(source["bpm"].as_f64().unwrap()),
                Some(path.generation(id)),
            )
            .unwrap();
        path.drain();
        path.engine
            .set_pad_timing_metadata_with_source(
                id,
                source["grid_origin"].as_f64().unwrap(),
                Some(path.generation(id)),
            )
            .unwrap();
        path.drain();
    }
    path.engine.set_master_bpm(94.0).unwrap();
    path.drain();
    path.engine.set_speed(1.0).unwrap();
    path.drain();
    path.engine.set_bpm_lock_with_binding(true, None).unwrap();
    path.drain();
    let slot = path.engine.loop_acceptance.diagnostics.register().unwrap();
    let mut cases = Vec::new();
    for origin_key in ["saved_start", "aligned_start"] {
        for long_bars in [32, 64] {
            path.engine.stop_all().unwrap();
            path.drain();
            let bars = [long_bars, 8];
            let starts: [f64; 2] =
                std::array::from_fn(|id| sources[id][origin_key].as_f64().unwrap());
            let ends: [f64; 2] = std::array::from_fn(|id| {
                starts[id] + f64::from(bars[id]) * 240.0 / sources[id]["bpm"].as_f64().unwrap()
            });
            let mut windows = Vec::new();
            for id in 0..2 {
                let frames = fingerprints[id]["playback"]["full_frames"]
                    .as_u64()
                    .unwrap();
                assert!(
                    ends[id] < frames as f64 / 48_000.0,
                    "requested complete loop crosses EOF"
                );
                let window = path
                    .engine
                    .prepare_resident_control_with_source(
                        id,
                        Some(starts[id]),
                        Some(ends[id]),
                        None,
                        Some(false),
                        Some(path.generation(id)),
                    )
                    .unwrap();
                wait_until_for(Duration::from_secs(120), || {
                    path.drain();
                    resident_relocation::reconcile(&path.engine).unwrap();
                    match window.publication_status() {
                        "accepted" => true,
                        "preparing" | "pending" | "adopting" => false,
                        other => panic!("real resident ACK failed: {other}: {:?}", window.error()),
                    }
                });
                windows.push(window);
            }
            let captured: Vec<_> = (0..2)
                .map(|id| {
                    path.engine
                        .capture_pad_start(
                            id,
                            starts[id],
                            Some(ends[id]),
                            0.25,
                            false,
                            None,
                            Some(path.generation(id)),
                        )
                        .unwrap()
                })
                .collect();
            for start in &captured {
                assert!(path.engine.publish_pad_start(start, None).unwrap());
                path.drain();
            }
            for start in &captured {
                path.wait_start(start);
            }
            let due = captured
                .iter()
                .map(|c| c.execution_frame().unwrap())
                .max()
                .unwrap();
            while path.callback.transport.output_frame() < due {
                let n = (due - path.callback.transport.output_frame()).min(512) as usize;
                path.render(n);
            }
            path.render(0);
            for c in &captured {
                assert_eq!(c.state(), "applied", "{:?}", c.reason());
            }
            let start_frame = path.callback.transport.output_frame();
            let main_horizon = (64.0_f64 * 240.0 / 94.0 * 48_000.0).ceil() as u64;
            let total = main_horizon + 512; // Explicit post-64 tail observes the long seam.
            let mut previous: [f64; 2] = starts.map(|s| (s * 48_000.0).round());
            let mut wraps = [0_u64; 2];
            let initial: [Option<f64>; 2] = std::array::from_fn(|id| {
                oracle[id]
                    .then(|| neural_phase(&sources[id], starts[id]))
                    .flatten()
            });
            let mut first_relative: Option<f64> = None;
            let mut last_relative: Option<f64> = None;
            let mut maximum_relative = 0.0_f64;
            let mut maximum_drift = 0.0_f64;
            let mut segment_origin: Option<f64> = None;
            let mut maximum_within_loop_change = 0.0_f64;
            let mut maximum_seam = 0.0_f64;
            let mut observed_seams = 0;
            let mut unavailable = 0;
            let mut energy = 0.0;
            let mut checkpoints = Vec::new();
            let mut next_checkpoint = 0;
            let mut last_physical = [[0_u64; 2]; 2];
            let mut loop_oracle = [None; 2];
            let mut first_observed_initial_phase = None;
            let initial_requested_phase = initial[0].zip(initial[1]).map(|(a, b)| {
                let difference = a - b;
                (difference - difference.round()) * 60.0 / 94.0
            });
            let generations = [path.generation(0), path.generation(1)];
            while path.callback.transport.output_frame() - start_frame < total {
                let elapsed_before = path.callback.transport.output_frame() - start_frame;
                let next_boundary = if elapsed_before < main_horizon {
                    main_horizon
                } else {
                    total
                };
                let frames = (next_boundary - elapsed_before).min(512) as usize;
                let request = path.engine.request_sync_diagnostic_snapshot().unwrap();
                path.callback.mixer.begin_sync_diagnostic_capture(request);
                let output = path.render(frames);
                assert!(output.iter().all(|s| s.is_finite()));
                energy += output.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>();
                slot.observe(
                    &path.callback.mixer,
                    &path.callback.transport,
                    path.engine.input_clock.capture_ns(),
                    frames,
                );
                let snapshot = path
                    .callback
                    .mixer
                    .sync_diagnostic_snapshot(
                        request,
                        slot.birth,
                        &path.callback.transport,
                        path.engine.input_clock.capture_ns(),
                        frames,
                    )
                    .unwrap();
                let words = &snapshot.0;
                assert_eq!(words[11], 2, "both real voices must remain active");
                assert!(
                    (f64::from_bits(words[8]) - 60.0 / 94.0).abs() < 1e-12,
                    "actual master did not apply94 BPM"
                );
                let mut progress = [None; 2];
                let mut observed_nn = [None; 2];
                let mut unwrapped_source = [0.0; 2];
                let mut seen = [false; 2];
                let mut seam = false;
                for row in words[HEADER_WORDS..].chunks_exact(ROW_WORDS).take(2) {
                    let id = row[0] as usize;
                    assert!(id < 2);
                    assert!(
                        !seen[id],
                        "two voices of the same PAD are not the intended pair"
                    );
                    seen[id] = true;
                    assert_eq!(row[4], generations[id]);
                    assert_eq!(
                        row[40],
                        fingerprints[id]["playback"]["full_frames"]
                            .as_u64()
                            .unwrap()
                    );
                    assert_eq!(row[17], 0, "single SYNC must be OFF / Legacy");
                    let expected_rate = 94.0 / sources[id]["bpm"].as_f64().unwrap();
                    for field in [14, 15] {
                        let rate = f64::from_bits(row[field]);
                        assert!(
                            rate.is_finite() && (rate / expected_rate - 1.0).abs() < 0.00002,
                            "actual source rate {rate} vs {expected_rate}"
                        );
                    }
                    assert_ne!(row[16] & (1 << 7), 0, "missing actual complete read");
                    assert_eq!(row[33], 1, "NativeInput is not dry audible phase");
                    assert_eq!(
                        row[16] & ((1 << 1) | (1 << 12)),
                        0,
                        "KL/Native must remain off"
                    );
                    assert_eq!(row[30], path.callback.transport.output_frame());
                    let current = row[31] as f64 + f64::from_bits(row[32]);
                    assert!(current.is_finite());
                    last_physical[id] = [row[34], row[35]];
                    let ideal_period = f64::from(bars[id]) * 240.0
                        / sources[id]["bpm"].as_f64().unwrap()
                        * 48_000.0;
                    assert!(
                        ((row[35] - row[34]) as f64 - ideal_period).abs() <= 1.000001,
                        "requested bars lost physical extent"
                    );
                    let complete = *loop_oracle[id].get_or_insert_with(|| {
                        oracle[id]
                            && complete_neural_coverage(
                                &sources[id],
                                row[34] as f64 / 48_000.0,
                                row[35] as f64 / 48_000.0,
                            )
                    });
                    if current < previous[id] {
                        wraps[id] += 1;
                        seam = true;
                    }
                    previous[id] = current;
                    unwrapped_source[id] =
                        current - row[34] as f64 + wraps[id] as f64 * (row[35] - row[34]) as f64;
                    observed_nn[id] = oracle[id]
                        .then(|| neural_phase(&sources[id], current / 48_000.0))
                        .flatten();
                    progress[id] = complete
                        .then_some(initial[id])
                        .flatten()
                        .zip(observed_nn[id])
                        .map(|(a, b)| b + wraps[id] as f64 * f64::from(bars[id]) * 4.0 - a);
                }
                assert!(seen.iter().all(|v| *v));
                if first_observed_initial_phase.is_none()
                    && let (Some(a), Some(b)) = (observed_nn[0], observed_nn[1])
                {
                    let difference = a - b;
                    first_observed_initial_phase = Some(
                        json!({"output_frame":path.callback.transport.output_frame(),"beat_phase_seconds":(difference-difference.round())*60.0/94.0,
                        "scope":"one initial estimated-beat wrap at common actual read boundary, includes actual birth E difference; unknown musical count/meter"}),
                    );
                }
                if let (Some(a), Some(b)) = (progress[0], progress[1]) {
                    let relative = (a - b) * 60.0 / 94.0;
                    first_relative.get_or_insert(relative);
                    maximum_relative = maximum_relative.max(relative.abs());
                    maximum_drift = maximum_drift.max((relative - first_relative.unwrap()).abs());
                    if seam && let Some(last) = last_relative {
                        maximum_seam = maximum_seam.max((relative - last).abs());
                        observed_seams += 1;
                    }
                    if seam {
                        segment_origin = None;
                    }
                    let origin = *segment_origin.get_or_insert(relative);
                    maximum_within_loop_change =
                        maximum_within_loop_change.max((relative - origin).abs());
                    last_relative = Some(relative);
                } else {
                    unavailable += 1;
                    last_relative = None;
                    segment_origin = None;
                }
                let elapsed = path.callback.transport.output_frame() - start_frame;
                if elapsed >= next_checkpoint || elapsed == main_horizon || elapsed == total || seam
                {
                    checkpoints.push(json!({"elapsed_frames":elapsed,"wraps":wraps,"unwrapped_source_frames":unwrapped_source,"pointwise_nn_ordinals":observed_nn,
                        "relative_nn_advancement_seconds":last_relative,"actual":snapshot_json(&path, request)}));
                    next_checkpoint = elapsed + (4.0 * 240.0 / 94.0 * 48_000.0) as u64;
                }
                path.callback.mixer.end_sync_diagnostic_capture();
            }
            assert!(energy > 0.0);
            let physical_rounding: Vec<_> = (0..2).map(|id| {
                let [a,b] = last_physical[id];
                let ideal = f64::from(bars[id]) * 240.0 / sources[id]["bpm"].as_f64().unwrap() * 48_000.0;
                json!({"pad_id":id,"physical_a":a,"physical_b":b,"period_rounding_frames":(b-a) as f64-ideal,"wraps":wraps[id]})
            }).collect();
            cases.push(json!({"origin_hypothesis":origin_key,"bars":bars,"starts_seconds":starts,"ends_seconds":ends,
                "master_bpm":94.0,"speed":1.0,"bpm_lock":true,"sync":false,"key_lock":false,"read_mode":"original_mix_dry",
                "source_ids":[0,1],"initial_nn_ordinals":initial,"requested_initial_estimated_beat_phase_seconds":initial_requested_phase,
                "first_observed_initial_estimated_beat_phase":first_observed_initial_phase,
                "output_start_frame":start_frame,"rendered_frames":total,"horizon_master_bars":64,"post_horizon_tail_frames":512,
                "births":captured.iter().map(|c| json!({"original":c.original_frame(),"execution":c.execution_frame(),"effective":c.effective_frame()})).collect::<Vec<_>>(),
                "max_abs_relative_nn_advancement_seconds":first_relative.map(|_|maximum_relative),"first_relative_nn_advancement_seconds":first_relative,
                "max_drift_from_first_nn_observation_seconds":first_relative.map(|_|maximum_drift),
                "max_phase_change_between_loop_seams_seconds":first_relative.map(|_|maximum_within_loop_change),
                "last_relative_nn_advancement_seconds":last_relative,"max_observed_seam_jump_seconds":(observed_seams>0).then_some(maximum_seam),
                "nn_oracle_complete":unavailable==0 && oracle.iter().all(|v|*v),
                "complete_loop_nn_oracles":loop_oracle,
                "nn_oracle_unavailable_chunks":unavailable,"physical_rounding":physical_rounding,"output_energy":energy,
                "checkpoints":checkpoints}));
        }
    }
    for source in sources {
        assert_eq!(
            hash_file(std::path::Path::new(source["path"].as_str().unwrap())),
            source["source_sha256"]
        );
    }
    path.engine.stop_all().unwrap();
    path.drain();
    path.engine.shut_down().unwrap();
    let result = json!({"scope":"Actual headless source/read/launch evidence; retained NN ordinal phase is estimated, unknown meter/count and no hearing/device acceptance",
        "inputs_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"runtime":process::runtime_identity().unwrap(),"fingerprints":fingerprints,"cases":cases});
    fs::write(
        root.join("actual-music-loops.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!(
        "ACTUAL_MUSIC_LOOPS {}",
        root.join("actual-music-loops.json").display()
    );
}
