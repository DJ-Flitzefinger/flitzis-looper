//! Isolated productive 200-pad cold/warm resource measurement, without CPAL.
//! Python runs the real LoaderController. The bridge supplies only stream format
//! and the real bounded producer in place of device startup.
use super::*;
use pyo3::prelude::*;

use serde_json::{Value, json};
use std::collections::HashSet;
use std::ffi::CString;

#[path = "c3_process_metrics.rs"]
mod process;

#[path = "c3_analysis_measurement.rs"]
mod analysis;

#[path = "c3_accepted_save_measurement.rs"]
mod accepted_save;

#[path = "c3_lifecycle_measurement.rs"]
mod lifecycle;

#[pyclass]
struct NativeBridge {
    #[pyo3(get)]
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    root: PathBuf,
    full: bool,
}

#[pymethods]
impl NativeBridge {
    #[pyo3(signature = (id, path, run_analysis=false, restore_automatic=false,
        replace_assignment=false, resident_loop_start_s=None, resident_loop_end_s=None,
        resident_key_lock=false))]
    #[allow(clippy::too_many_arguments)]
    fn load_sample_async(
        &self,
        py: Python<'_>,
        id: usize,
        path: String,
        run_analysis: bool,
        restore_automatic: bool,
        replace_assignment: bool,
        resident_loop_start_s: Option<f64>,
        resident_loop_end_s: Option<f64>,
        resident_key_lock: bool,
    ) -> PyResult<u64> {
        let hint = if self.full {
            None
        } else {
            resident_loop_start_s
                .zip(resident_loop_end_s)
                .map(|(start_s, end_s)| ResidentLoadHint {
                    start_s,
                    end_s,
                    key_lock: resident_key_lock,
                })
        };
        admit_for_format_selected(
            &self.engine.borrow(py),
            id,
            path,
            (run_analysis, restore_automatic, replace_assignment, hint),
            self.producer.clone(),
            (2, 48_000, self.root.clone()),
        )
    }

    fn loaded_sample_shape(&self, py: Python<'_>, id: usize) -> PyResult<(u32, usize, usize)> {
        let engine = self.engine.borrow(py);
        let samples = engine.sample_cache.lock().unwrap();
        let sample = samples[id]
            .as_ref()
            .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("no acknowledged source"))?;
        let generation = engine.loaded_source_generations.lock().unwrap()[id].0;
        if !engine
            .input_runtime_ownership
            .source_current(id, sample, 48_000)
            || generation == 0
        {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "source not current",
            ));
        }
        Ok((48_000, sample.channels, sample.frame_count()))
    }
}

fn call(probe: &Py<PyAny>, method: &str) {
    Python::attach(|py| {
        probe.call_method0(py, method).unwrap();
    });
}

fn python_json(probe: &Py<PyAny>, method: &str) -> Value {
    Python::attach(|py| {
        let result = probe.call_method0(py, method).unwrap();
        let text: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (result,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    })
}

fn python_runtime_identity() -> Value {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!("c3_controller_probe.py")).unwrap(),
            c"c3_controller_probe.py",
            c"c3_runtime_identity",
        )
        .unwrap();
        let identity = module
            .getattr("runtime_identity")
            .unwrap()
            .call1((repository.to_string_lossy().as_ref(),))
            .unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (identity,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

struct SamplingGuard(Arc<AtomicBool>);

impl Drop for SamplingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn measure(action: impl FnOnce() -> Value) -> Value {
    let before = process::snapshot();
    let sampling = Arc::new(AtomicBool::new(true));
    // An assertion failure must also stop the isolated probe's observer.
    let _stop_sampling = SamplingGuard(sampling.clone());
    let active = sampling.clone();
    let sampler = std::thread::spawn(move || {
        let mut count = 0_u64;
        let mut peaks = [0_u64; 3];
        while active.load(Ordering::Acquire) {
            let sample = process::snapshot();
            for (index, name) in [
                "working_set_bytes",
                "private_committed_bytes",
                "handle_count",
            ]
            .iter()
            .enumerate()
            {
                peaks[index] = peaks[index].max(sample[name].as_u64().unwrap());
            }
            count += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
        (count, peaks)
    });
    let started = Instant::now();
    let detail = action();
    let wall_ns = started.elapsed().as_nanos();
    let after = process::snapshot();
    sampling.store(false, Ordering::Release);
    let (samples, sampled) = sampler.join().unwrap();
    let mut peaks = serde_json::Map::new();
    for (index, name) in [
        "working_set_bytes",
        "private_committed_bytes",
        "handle_count",
    ]
    .iter()
    .enumerate()
    {
        peaks.insert(
            (*name).into(),
            json!(
                sampled[index]
                    .max(before[name].as_u64().unwrap())
                    .max(after[name].as_u64().unwrap())
            ),
        );
    }
    json!({"wall_ns":wall_ns, "before":before, "after":after,
        "sampled_peaks":peaks, "samples":samples, "sampling_interval_ms":1,
        "sampling_limit":"observed lower bound; one observer thread with bounded three-counter storage included in process counters",
        "process_delta":process::delta(&before, &after), "detail":detail})
}

fn pcm_ownership(engine: &AudioEngine, callback: &Callback) -> Value {
    let mut addresses = HashSet::new();
    let mut bytes = 0;
    let mut per_pad = 0;
    for sample in engine
        .sample_cache
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .chain(callback.mixer.bank_for_measurement().iter().flatten())
        .chain(
            callback
                .mixer
                .voices
                .iter()
                .filter_map(|voice| voice.sample.as_ref()),
        )
    {
        per_pad += sample.samples.len() * 4;
        if addresses.insert(sample.samples.as_ptr() as usize) {
            bytes += sample.samples.len() * 4;
        }
    }
    json!({"distinct_backings":addresses.len(), "unique_retained_pcm_bytes":bytes,
        "bank_control_voice_sum_bytes_not_deduplicated":per_pad})
}

fn drain(callback: &mut Callback, consumer: &mut rtrb::Consumer<ControlMessage>) {
    while callback.feedback_rx.pop().is_ok() {}
    callback.drain(consumer);
}

fn window(
    engine: &AudioEngine,
    callback: &mut Callback,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    request: super::super::resident_relocation::WindowRequest,
) -> Value {
    let old = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let old_voice = callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .map(|voice| {
            (
                voice.sample.as_ref().unwrap().clone(),
                voice.frame_pos,
                voice.paused,
            )
        });
    let ticket = super::super::resident_relocation::prepare_window_with_producer(
        engine,
        0,
        request,
        producer.clone(),
    )
    .unwrap();
    assert!(ticket.is_current());
    let deadline = Instant::now() + Duration::from_secs(300);
    while ticket.publication_status() != "accepted" {
        // Effective bank/old voice can change only at this actual command drain.
        if ticket.publication_status() == "preparing" {
            assert!(
                callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .same_window(&old)
            );
            if let Some((sample, frame, paused)) = &old_voice {
                let voice = callback
                    .mixer
                    .voices
                    .iter()
                    .find(|voice| voice.is_playing_sample(0))
                    .unwrap();
                assert!(voice.sample.as_ref().unwrap().same_window(sample));
                assert_eq!(voice.frame_pos, *frame);
                assert_eq!(voice.paused, *paused);
            }
        }
        drain(callback, consumer);
        assert!(
            matches!(
                ticket.publication_status(),
                "preparing" | "pending" | "accepted"
            ),
            "{:?}",
            ticket.error().unwrap()
        );
        assert!(Instant::now() < deadline, "C3 window timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
    super::super::resident_relocation::reconcile(engine).unwrap();
    let new = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    assert!(new.same_source(&old));
    assert!(
        callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .same_window(&new)
    );
    let effective_voice = callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0));
    if let Some(seconds) = ticket.effective_seek_seconds() {
        if let Some(voice) = effective_voice {
            assert_eq!(voice.frame_pos, (seconds * 48_000.0).round() as usize);
        }
    }
    let effective_voice_frame = effective_voice.map(|voice| voice.frame_pos);
    let effective_paused = effective_voice.map(|voice| voice.paused);
    let mut seek_pcm_proof = Value::Null;
    if let (Some(seconds), Some(paused)) = (ticket.effective_seek_seconds(), effective_paused) {
        let first = (seconds * 48_000.0).round() as usize;
        let mut output = [0.0; 1024];
        callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            0,
            &mut callback.retirement,
        );
        for (index, actual) in output.chunks_exact(2).enumerate() {
            let expected = if paused {
                0.0
            } else {
                (((first + index) * 211 + 37) % 16_384) as f32 / 32768.0 - 0.25
            };
            assert_eq!(actual, [expected; 2]);
        }
        seek_pcm_proof = json!({"frames":512,"source_first_frame":first,"paused_silence":paused,"independent_exact_pcm":true});
    }
    json!({"status":ticket.publication_status(), "current":ticket.is_current(),
        "old_voice":old_voice.as_ref().map(|(_, frame, paused)| json!({"frame":frame,"paused":paused})),
        "effective_voice_frame":effective_voice_frame,
        "seek_pcm_proof":seek_pcm_proof,
        "old_voice_preserved_during_preparation":true,
        "effective_seek_seconds":ticket.effective_seek_seconds(),
        "old_window_revision":old.window_revision(), "new_window_revision":new.window_revision(),
        "old_context":format!("{:?}",old.residency.as_ref().unwrap().context),
        "new_context":format!("{:?}",new.residency.as_ref().unwrap().context),
        "effective_key_lock":callback.mixer.key_lock_for_measurement(0),
        "resident_start":new.resident_start(), "resident_end":new.resident_end(),
        "full_frames":new.frame_count(), "resident_pcm_bytes":new.samples.len()*4})
}

fn render_cost(callback: &mut Callback, voices: usize, config: &Value) -> Value {
    for id in 0..32 {
        callback.mixer.stop_sample_rt(id, &mut callback.retirement);
    }
    for id in 0..voices {
        assert!(
            callback
                .mixer
                .play_sample_rt(id, 1.0, &mut callback.retirement)
        );
    }
    let mut output = [0.0; 1024];
    let mut peaks = [0.0; NUM_SAMPLES];
    let mut max_ns = 0;
    let before = process::snapshot();
    let start = Instant::now();
    for block in 0..1000 {
        let then = Instant::now();
        callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut peaks,
            block * 512,
            &mut callback.retirement,
        );
        max_ns = max_ns.max(then.elapsed().as_nanos());
        assert!(output.iter().all(|value| value.is_finite()));
    }
    let after = process::snapshot();
    let wall_ns = start.elapsed().as_nanos();
    for id in 0..32 {
        callback.mixer.stop_sample_rt(id, &mut callback.retirement);
    }
    for id in 0..voices {
        assert!(
            callback
                .mixer
                .play_sample_rt(id, 1.0, &mut callback.retirement)
        );
    }
    let mut proof = vec![0.0; 48_007 * 2];
    callback
        .mixer
        .render_rt_at_output_frame(&mut proof, &mut peaks, 0, &mut callback.retirement);
    let mut maximum_error = 0.0_f32;
    for (frame, samples) in proof.chunks_exact(2).enumerate() {
        let source_frame = 2_016_000 + frame % 24_000;
        let mut expected = 0.0_f32;
        for pad in 0..voices {
            let seed = if config["topology"] == "unique" {
                pad
            } else {
                0
            };
            let integer = ((source_frame * 211 + seed * 137 + 37) % 16_384) as i32 - 8192;
            expected += integer as f32 / 32768.0;
        }
        for sample in samples {
            maximum_error = maximum_error.max((sample - expected).abs());
        }
    }
    assert!(
        maximum_error <= 2e-6,
        "independent multi-voice loop PCM error {maximum_error}"
    );
    json!({"voices":voices, "output_frames":512000, "wall_ns":start.elapsed().as_nanos(),
        "render_wall_ns_excluding_oracle":wall_ns, "independent_loop_proof_frames":48007,
        "independent_pcm_max_error":maximum_error, "rendered_pcm_sha256":pcm_hash(&proof),
        "max_block_ns":max_ns, "process_delta":process::delta(&before, &after),
        "scope":"offline dry native renderer, no device deadline or hearing claim"})
}

#[test]
#[ignore = "isolated real 200-pad cold/warm controller and native resource matrix"]
fn c3_productive_200_pad_measurement() {
    let config_path =
        PathBuf::from(std::env::var_os("FLITZI_C3_CONFIG").expect("frozen C3 config"));
    let config_bytes = fs::read(&config_path).unwrap();
    let config: Value = serde_json::from_slice(&config_bytes).unwrap();
    let root = PathBuf::from(config["project_root"].as_str().unwrap());
    assert!(root.is_absolute());
    std::env::set_current_dir(&root).unwrap();
    let paths: Vec<String> = config["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(paths.len(), 200);
    let full = config["storage"] == "full";
    let warm = config["stage"] == "warm";
    let end_to_end_started = Instant::now();
    let baseline = process::snapshot();
    let observation_before = super::super::c3_observation::snapshot();
    Python::initialize();
    let python_runtime = python_runtime_identity();
    let engine = Python::attach(|py| Py::new(py, AudioEngine::new().unwrap()).unwrap());
    let setup_start = Instant::now();
    let before_setup = process::snapshot();
    let mut callback = Python::attach(|py| Callback::new(&engine.borrow(py), 48_000));
    let after_setup = process::snapshot();
    let observation_setup = super::super::c3_observation::snapshot();
    assert_eq!(observation_setup.0 - observation_before.0, 96);
    assert_eq!(observation_setup.1 - observation_before.1, 96);
    let setup = json!({"wall_ns":setup_start.elapsed().as_nanos(),
        "before":before_setup, "after":after_setup,
        "process_delta":process::delta(&before_setup, &after_setup),
        "voices":32, "native_handles_created":observation_setup.0-observation_before.0,
        "live_native_handles":observation_setup.1, "warmed_handles":64, "source_reserves":32});
    let (producer, mut consumer) = rtrb::RingBuffer::new(512);
    let producer = Arc::new(Mutex::new(producer));
    let bridge = Python::attach(|py| {
        Py::new(
            py,
            NativeBridge {
                engine: engine.clone_ref(py),
                producer: producer.clone(),
                root: root.join("samples"),
                full,
            },
        )
        .unwrap()
    });
    let before_load = process::snapshot();
    let started = Instant::now();
    let probe = Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(include_str!("c3_controller_probe.py")).unwrap(),
            c"c3_controller_probe.py",
            c"c3_controller_probe",
        )
        .unwrap();
        let options = pyo3::types::PyDict::new(py);
        options.set_item("warm", warm).unwrap();
        module
            .getattr("Probe")
            .unwrap()
            .call((bridge, paths.clone()), Some(&options))
            .unwrap()
            .unbind()
    });
    let mut source_ack_ns = vec![None; 200];
    let mut ready_ns = vec![None; 200];
    let mut loop_commands = [false; 200];
    let mut peak_counts = (0, 0, 0);
    let mut sampled_peak_private = 0;
    let mut peak_handles = 0;
    let deadline = started + Duration::from_secs(7200);
    let mut last_progress = Instant::now();
    loop {
        drain(&mut callback, &mut consumer);
        Python::attach(|py| {
            let owner = engine.borrow(py);
            let counts = owner.cold_jobs.counts_for_test();
            peak_counts = (
                peak_counts.0.max(counts.0),
                peak_counts.1.max(counts.1),
                peak_counts.2.max(counts.2),
            );
            for id in 0..200 {
                let Some(bank) = callback.mixer.bank_for_measurement()[id].as_ref() else {
                    continue;
                };
                if !owner
                    .input_runtime_ownership
                    .source_current(id, bank, 48_000)
                {
                    continue;
                }
                if source_ack_ns[id].is_none() {
                    source_ack_ns[id] = Some(started.elapsed().as_nanos());
                }
                if full && !loop_commands[id] {
                    producer
                        .lock()
                        .unwrap()
                        .push(ControlMessage::SetPadLoopRegion {
                            id,
                            start_s: 42.0,
                            end_s: Some(42.5),
                        })
                        .unwrap();
                    loop_commands[id] = true;
                }
                if owner.cold_loading[id].load(Ordering::Acquire) == 0
                    && callback.mixer.loop_region_frames(id) == (2_016_000, Some(2_040_000))
                    && ready_ns[id].is_none()
                {
                    let cache = owner.sample_cache.lock().unwrap();
                    assert!(cache[id].as_ref().unwrap().same_window(bank));
                    ready_ns[id] = Some(started.elapsed().as_nanos());
                }
            }
        });
        let done: bool =
            Python::attach(|py| probe.call_method0(py, "poll").unwrap().extract(py).unwrap());
        let now = process::snapshot();
        sampled_peak_private =
            sampled_peak_private.max(now["private_committed_bytes"].as_u64().unwrap());
        peak_handles = peak_handles.max(now["handle_count"].as_u64().unwrap());
        if last_progress.elapsed() >= Duration::from_secs(10) {
            println!(
                "C3 progress: ready={} workers={:?} private_bytes={} elapsed_s={:.1}",
                ready_ns.iter().filter(|value| value.is_some()).count(),
                peak_counts,
                now["private_committed_bytes"],
                started.elapsed().as_secs_f64()
            );
            last_progress = Instant::now();
        }
        if done && ready_ns.iter().all(Option::is_some) {
            break;
        }
        assert!(Instant::now() < deadline, "C3 200-pad readiness deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    let after_load = process::snapshot();
    let load_wall_ns = started.elapsed().as_nanos();
    let end_to_end_ready_ns = end_to_end_started.elapsed().as_nanos();
    assert!(peak_counts.0 <= 2 && peak_counts.1 <= 32 && peak_counts.2 <= 34);
    let controller = python_json(&probe, "summary");
    let manifests = Python::attach(|py| {
        let owner = engine.borrow(py);
        (0..200)
            .map(|id| {
                let actual: Value =
                    serde_json::from_str(&owner.cold_source_manifest(id).unwrap().unwrap())
                        .unwrap();
                let expected = &config["assignments"][id];
                assert_eq!(
                    actual["descriptor"]["decoder"]["original"]["sha256"],
                    expected["source_sha256"]
                );
                assert_eq!(
                    actual["descriptor"]["decoder"]["pcm"]["interleaved_sha256"],
                    expected["decoder_sha256"]
                );
                assert_eq!(
                    actual["descriptor"]["playback"]["pcm"]["interleaved_sha256"],
                    expected["playback_sha256"]
                );
                if warm {
                    assert_eq!(actual["integrity"]["warm"], true);
                }
                let cache = owner.sample_cache.lock().unwrap();
                let bank = callback.mixer.bank_for_measurement()[id].as_ref().unwrap();
                assert!(bank.same_window(cache[id].as_ref().unwrap()));
                assert_eq!(bank.frame_count(), 5_760_000);
                assert_eq!(
                    bank.samples.len() * 4,
                    if full { 46_080_000 } else { 192_000 }
                );
                if !full {
                    assert_eq!(pcm_hash(&bank.samples), expected["loop_sha256"]);
                }
                json!({"pad":id, "request":owner.loaded_source_generations.lock().unwrap()[id].0,
                "source_id":format!("{:?}", Arc::as_ptr(&bank.residency.as_ref().unwrap().source) as usize),
                "window_revision":bank.window_revision(), "manifest":actual})
            })
            .collect::<Vec<_>>()
    });
    let ownership = Python::attach(|py| pcm_ownership(&engine.borrow(py), &callback));
    let save = measure(|| {
        call(&probe, "save");
        json!({"config_bytes":fs::metadata(root.join("samples/flitzis_looper.config.json")).unwrap().len(),
        "accepted_timing_integrity_bytes":0, "intent":"legacy"})
    });
    let render: Vec<_> = [1, 2, 4, 6, 32]
        .into_iter()
        .map(|voices| render_cost(&mut callback, voices, &config))
        .collect();
    for id in 0..32 {
        callback.mixer.stop_sample_rt(id, &mut callback.retirement);
    }
    let mut exceptions = serde_json::Map::new();
    {
        use super::super::resident_relocation::WindowRequest;
        assert!(
            callback
                .mixer
                .play_sample_rt(0, 1.0, &mut callback.retirement)
        );
        let mut initial = [0.0; 1024];
        callback.mixer.render_rt_at_output_frame(
            &mut initial,
            &mut [0.0; NUM_SAMPLES],
            0,
            &mut callback.retirement,
        );
        for (name, request) in [
            (
                "active_outside_loop_tail_seek",
                WindowRequest {
                    seek_position_s: Some(100.0),
                    ..Default::default()
                },
            ),
            (
                "finite_return",
                WindowRequest {
                    loop_region: Some((42.0, Some(42.5))),
                    ..Default::default()
                },
            ),
            (
                "all",
                WindowRequest {
                    loop_region: Some((0.0, Some(120.0))),
                    ..Default::default()
                },
            ),
            (
                "short_return",
                WindowRequest {
                    loop_region: Some((42.0, Some(42.5))),
                    ..Default::default()
                },
            ),
            (
                "key_lock_full_context",
                WindowRequest {
                    key_lock: Some(true),
                    ..Default::default()
                },
            ),
            (
                "key_lock_off",
                WindowRequest {
                    key_lock: Some(false),
                    ..Default::default()
                },
            ),
        ] {
            let result = measure(|| {
                Python::attach(|py| {
                    window(
                        &engine.borrow(py),
                        &mut callback,
                        &producer,
                        &mut consumer,
                        request,
                    )
                })
            });
            exceptions.insert(name.into(), result);
        }
        callback.mixer.pause_sample(0);
        let paused = measure(|| {
            Python::attach(|py| {
                window(
                    &engine.borrow(py),
                    &mut callback,
                    &producer,
                    &mut consumer,
                    WindowRequest {
                        seek_position_s: Some(30.0),
                        ..Default::default()
                    },
                )
            })
        });
        exceptions.insert("paused_outside_loop_intro_seek".into(), paused);
        callback.mixer.stop_sample_rt(0, &mut callback.retirement);
        let stopped = measure(|| {
            Python::attach(|py| {
                window(
                    &engine.borrow(py),
                    &mut callback,
                    &producer,
                    &mut consumer,
                    WindowRequest {
                        seek_position_s: Some(100.0),
                        ..Default::default()
                    },
                )
            })
        });
        exceptions.insert("stopped_seek_noop".into(), stopped);
        let editor = measure(|| {
            Python::attach(|py| {
                let owner = engine.borrow(py);
                let deadline = Instant::now() + Duration::from_secs(300);
                loop {
                    if let Some((raw, xs, minima, maxima)) = owner
                        .get_waveform_render_data(py, 0, 1024, 0.0, 120.0)
                        .unwrap()
                    {
                        use numpy::PyArrayMethods;
                        let times = xs.bind(py).readonly();
                        let lows = minima.bind(py).readonly();
                        let highs = maxima.as_ref().unwrap().bind(py).readonly();
                        let times = times.as_slice().unwrap();
                        let lows = lows.as_slice().unwrap();
                        let highs = highs.as_slice().unwrap();
                        assert!(!raw);
                        assert_eq!(times.len(), 1024);
                        for bucket in 0..1024 {
                            let first = bucket * 5_760_000 / 1024;
                            let end = (bucket + 1) * 5_760_000 / 1024;
                            assert_eq!(times[bucket], first as f64 / 48_000.0);
                            let mut low = f32::MAX;
                            let mut high = f32::MIN;
                            for frame in first..end {
                                let value = ((frame * 211 + 37) % 16384) as i32 - 8192;
                                let value = value as f32 / 32768.0;
                                low = low.min(value);
                                high = high.max(value);
                            }
                            assert_eq!(lows[bucket], low);
                            assert_eq!(highs[bucket], high);
                        }
                        return json!({"points":times.len(), "first_x_seconds":times[0],
                        "last_x_seconds":times[1023], "independent_all_bucket_oracle":true,
                        "readiness":owner.waveform_readiness(0).unwrap().0});
                    }
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        });
        exceptions.insert("editor_whole_source".into(), editor);
        let tail = measure(|| {
            Python::attach(|py| {
                use numpy::PyArrayMethods;
                let owner = engine.borrow(py);
                let before = owner.sample_cache.lock().unwrap()[0].clone().unwrap();
                let deadline = Instant::now() + Duration::from_secs(300);
                loop {
                    if let Some((raw, xs, values, maxima)) = owner
                        .get_waveform_render_data(py, 0, 1024, 119.95, 119.96)
                        .unwrap()
                    {
                        assert!(raw && maxima.is_none());
                        let xs = xs.bind(py).readonly();
                        let values = values.bind(py).readonly();
                        let xs = xs.as_slice().unwrap();
                        let values = values.as_slice().unwrap();
                        assert_eq!(values.len(), 480);
                        for (offset, value) in values.iter().enumerate() {
                            let frame = 5_757_600 + offset;
                            assert_eq!(xs[offset], frame as f64 / 48_000.0);
                            assert_eq!(
                                *value,
                                (((frame * 211 + 37) % 16_384) as i32 - 8192) as f32 / 32768.0
                            );
                        }
                        assert!(
                            owner.sample_cache.lock().unwrap()[0]
                                .as_ref()
                                .unwrap()
                                .same_window(&before)
                        );
                        return json!({"points":values.len(),"independent_all_raw_samples":true,"native_window_unchanged":true});
                    }
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        });
        exceptions.insert("editor_distant_tail_raw".into(), tail);
        let returned = measure(|| {
            Python::attach(|py| {
                window(
                    &engine.borrow(py),
                    &mut callback,
                    &producer,
                    &mut consumer,
                    WindowRequest {
                        storage_range: Some((42.0, 42.5)),
                        loop_region: Some((42.0, Some(42.5))),
                        key_lock: Some(false),
                        ..Default::default()
                    },
                )
            })
        });
        assert_eq!(returned["detail"]["resident_pcm_bytes"], 192_000);
        exceptions.insert("explicit_return_to_finite_storage".into(), returned);
        let materialize = measure(|| {
            Python::attach(|py| {
                let owner = engine.borrow(py);
                let sample = owner.sample_cache.lock().unwrap()[0].clone().unwrap();
                let complete = owner
                    .complete_sample(0, &sample, super::super::cold_jobs::PCM_LIMIT_BYTES)
                    .unwrap();
                let weak = Arc::downgrade(&complete.samples);
                assert_eq!(
                    pcm_hash(&complete.samples),
                    config["assignments"][0]["playback_sha256"]
                );
                let bytes = complete.samples.len() * 4;
                let held = process::snapshot();
                let backing_shared = Arc::ptr_eq(&complete.samples, &sample.samples);
                drop(complete);
                assert_eq!(weak.upgrade().is_none(), !backing_shared);
                let no_temporary_strong_owner = weak.strong_count() == 0;
                drop(weak);
                json!({"actual_full_pcm_bytes":bytes, "process_while_full_owned":held,
                    "backing_shared_with_resident":backing_shared,
                    "temporary_strong_owners_zero":no_temporary_strong_owner,
                    "observer_weak_dropped":true,
                    "allocation_limit":"weak metadata and allocator arenas can retain storage; no zero-RAM recovery claim"})
            })
        });
        exceptions.insert("explicit_complete_materialization".into(), materialize);
    }
    call(&probe, "close");
    let analysis = if config["stage"] == "warm" {
        analysis::analyze(&engine, &root, &config["assignments"][0])
    } else {
        Value::Null
    };
    let final_ownership = Python::attach(|py| pcm_ownership(&engine.borrow(py), &callback));
    let before_shutdown = process::snapshot();
    Python::attach(|py| engine.borrow_mut(py).shut_down().unwrap());
    drop(callback);
    Python::attach(|_| {
        drop(probe);
        drop(engine);
    });
    Python::attach(|_| {});
    let after_shutdown = process::snapshot();
    let observation_final = super::super::c3_observation::snapshot();
    assert_eq!(observation_final.1, observation_before.1);
    let report = json!({"schema":"c3-productive-200-v1", "config_sha256":format!("{:x}", Sha256::digest(&config_bytes)),
        "configuration":config, "python_runtime":python_runtime,
        "native_runtime":process::runtime_identity().unwrap(), "baseline":baseline, "setup":setup,
        "load":{"before":before_load, "after":after_load, "wall_ns":load_wall_ns,
            "end_to_end_pre_python_setup_to_all_ready_ns":end_to_end_ready_ns,
            "end_to_end_process_delta":process::delta(&baseline,&after_load),
            "process_delta":process::delta(&before_load, &after_load),
            "source_ack_ns":source_ack_ns, "loop_ready_ns":ready_ns,
            "sampled_peak_private_bytes":sampled_peak_private, "sampled_peak_handles":peak_handles,
            "peak_active_workers":peak_counts.0, "peak_queued_jobs":peak_counts.1,
            "peak_admitted_jobs":peak_counts.2},
        "controller":controller, "ownership":ownership, "assignments":manifests,
        "legacy_save":save, "render":render, "exceptions":exceptions, "analysis":analysis,
        "before_shutdown":before_shutdown, "after_shutdown":after_shutdown,
        "pre_shutdown_ownership":final_ownership,
        "native_observation_before":observation_before,
        "native_observation_after_setup":observation_setup,
        "native_observation_final":observation_final,
        "largest_observed_per_operation_pcm_bytes":observation_final.3,
        "pcm_observation_scope":"single-operation decoder Vec capacity plus playback Arc at conversion return, or load Arc; maximum of declared points, not process-total simultaneous PCM, DSP workspace, allocator peak or admission bound",
        "limits":"Artifact cold/warm; OS page cache uncontrolled. Offline hardware-free only. Source ACK and effective loop readiness separately measured. Full storage is current-code baseline, not historical build. Win32 transfer bytes are logical process I/O. Human/device/hearing OPEN."});
    let output = PathBuf::from(std::env::var_os("FLITZI_C3_OUTPUT").expect("C3 output"));
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
