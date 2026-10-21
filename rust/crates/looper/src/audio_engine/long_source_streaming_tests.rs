//! Opt-in real complete-extent preparation and fresh-process integrity evidence.
//! Parent owns every child and requires its receipt/PID. No stream/device/app.
use super::*;
use crate::audio_engine::cold_store::sealed_reader;
use crate::audio_engine::material_migration::PreparedMigrationMaterial;
use crate::audio_engine::prepared_source::PreparedSourceTicket;
use crate::audio_engine::project_assets::ProjectAssetLease;
use crate::audio_engine::stem_cache::STEM_FILE_NAMES;
use crate::audio_engine::stem_pair::open_verified_pair;
use crate::audio_engine::stem_pair_preparation::PreparedStemPair;
use pyo3::types::PyDict;
use serde_json::{Value, json};
use std::io::{Read, Seek, SeekFrom, Write};

const CHILD: &str =
    "audio_engine::cold_load::tests::long_source_streaming_tests::x11_long_productive_child";
const RATE: u32 = 48_000;

#[path = "long_source_oracle_tests.rs"]
mod independent;

// Independent Python fixture pattern, no productive parser/decoder writes this oracle.
fn rf64_oracle(path: &Path, frames: usize) -> Value {
    let pattern = [i16::MIN, i16::MAX, 1, -1, 0, 12_345, -23_456, -32_767];
    let mut source = Vec::with_capacity(65_536);
    let mut native = Vec::with_capacity(65_536);
    let mut mono = Vec::with_capacity(32_768);
    let mut pcm_hash = Sha256::new();
    let mut native_hash = Sha256::new();
    let mut mono_hash = Sha256::new();
    for first in (0..frames).step_by(8192) {
        source.clear();
        native.clear();
        mono.clear();
        for frame in first..(first + 8192).min(frames) {
            let mut values = [0.0_f32; 2];
            for channel in 0..2 {
                let word = pattern[(frame * 2 + channel) % pattern.len()];
                values[channel] = f32::from(word) / 32768.0;
                source.extend_from_slice(&values[channel].to_le_bytes());
                let asym = if word == i16::MIN {
                    -1.0_f32
                } else {
                    f32::from(word) / 32767.0
                };
                native.extend_from_slice(&asym.to_le_bytes());
            }
            mono.extend_from_slice(
                &(((f64::from(values[0]) + f64::from(values[1])) / 2.0) as f32).to_le_bytes(),
            );
        }
        pcm_hash.update(&source);
        native_hash.update(&native);
        mono_hash.update(&mono);
    }
    json!({"original":file_hash(path),"pcm":format!("{:x}",pcm_hash.finalize()),
        "mono":format!("{:x}",mono_hash.finalize()),"native_pcm":format!("{:x}",native_hash.finalize()),"frames":frames})
}

fn verify_native_rf64(path: &Path, frames: usize, expected: &Value) {
    let reader = sealed_reader(path).unwrap();
    let wav = crate::audio_engine::stem_cache::wav::Pcm16Wav::inspect(&reader, &|| false).unwrap();
    assert_eq!(wav.frames, frames);
    assert_eq!(wav.format.channels, 2);
    assert_eq!(wav.format.sample_rate_hz, RATE);
    let mut values = vec![0.0_f32; 16_382];
    let mut encoded = vec![0_u8; 16_382 * 4];
    let mut digest = Sha256::new();
    for first in (0..frames).step_by(values.len() / 2) {
        let length = (frames - first).min(values.len() / 2) * 2;
        wav.read_frames(&reader, first, &mut values[..length], &|| false)
            .unwrap();
        for (value, bytes) in values[..length].iter().zip(encoded.chunks_exact_mut(4)) {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        digest.update(&encoded[..length * 4]);
    }
    assert_eq!(format!("{:x}", digest.finalize()), expected["native_pcm"]);
}

fn file_hash(path: &Path) -> String {
    let mut file = sealed_reader(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0; 64 * 1024];
    loop {
        let count = file.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    format!("{:x}", hash.finalize())
}

fn memory() -> Value {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged_pool: usize,
        paged_pool: usize,
        peak_nonpaged_pool: usize,
        nonpaged_pool: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            cb: u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    let mut value = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // SAFETY: current pseudo-handle and live correctly sized PROCESS_MEMORY_COUNTERS_EX.
    assert_ne!(
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut value, value.cb) },
        0
    );
    json!({"rss":value.working_set,"peak_rss":value.peak_working_set,
        "commit":value.pagefile,"peak_commit":value.peak_pagefile,"private":value.private})
}

fn raw(frame: usize, channel: usize) -> i16 {
    if frame < 17 {
        return 0;
    }
    if frame == 17 {
        return if channel == 0 { i16::MIN } else { i16::MAX };
    }
    let word = (frame as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(channel as u64 * 1442695040888963407);
    ((word >> 32) as i16) / 2
}

fn component(frame: usize, channel: usize, index: usize) -> i16 {
    let value = raw(frame, channel);
    match index {
        0 => value / 8,
        1 => value / 4,
        2 => value / 16,
        3 => value / 2,
        _ => value / 3,
    }
}

fn header(frames: usize) -> Vec<u8> {
    let data = u32::try_from(frames * 4).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes
}

// Independent known-integer source oracle; generator itself retains one block.
fn source_fixture(path: &Path, frames: usize, partition: usize) -> Value {
    let mut file = fs::File::create(path).unwrap();
    file.write_all(&header(frames)).unwrap();
    let mut pcm = Sha256::new();
    let mut mono = Sha256::new();
    let mut bytes = Vec::with_capacity(partition * 4);
    let mut pcm_bytes = Vec::with_capacity(partition * 8);
    let mut mono_bytes = Vec::with_capacity(partition * 4);
    for first in (0..frames).step_by(partition) {
        bytes.clear();
        pcm_bytes.clear();
        mono_bytes.clear();
        for frame in first..(first + partition).min(frames) {
            let values = [raw(frame, 0), raw(frame, 1)];
            let floats = values.map(|n| f32::from(n) / 32768.0);
            for (integer, value) in values.into_iter().zip(floats) {
                bytes.extend_from_slice(&integer.to_le_bytes());
                pcm_bytes.extend_from_slice(&value.to_le_bytes());
            }
            let mean = ((f64::from(floats[0]) + f64::from(floats[1])) / 2.0) as f32;
            mono_bytes.extend_from_slice(&mean.to_le_bytes());
        }
        pcm.update(&pcm_bytes);
        mono.update(&mono_bytes);
        file.write_all(&bytes).unwrap();
    }
    file.sync_all().unwrap();
    drop(file);
    json!({"original":file_hash(path),"pcm":format!("{:x}",pcm.finalize()),
        "mono":format!("{:x}",mono.finalize()),"frames":frames,"partition":partition})
}

fn generated_stems(path: &Path, frames: usize, partition: usize, version: &str) -> Value {
    fs::create_dir_all(path).unwrap();
    let mut hashes = serde_json::Map::new();
    let mut pcm_hashes = serde_json::Map::new();
    for (index, name) in STEM_FILE_NAMES.into_iter().enumerate() {
        let target = path.join(format!("{name}.wav"));
        let mut file = fs::File::create(&target).unwrap();
        file.write_all(&header(frames)).unwrap();
        let mut digest = Sha256::new();
        let mut bytes = Vec::with_capacity(partition * 4);
        let mut pcm_bytes = Vec::with_capacity(partition * 8);
        for first in (0..frames).step_by(partition) {
            bytes.clear();
            pcm_bytes.clear();
            for frame in first..(first + partition).min(frames) {
                for channel in 0..2 {
                    let integer = component(frame, channel, index);
                    bytes.extend_from_slice(&integer.to_le_bytes());
                    let value = if integer == i16::MIN {
                        -1.0_f32
                    } else {
                        f32::from(integer) / 32767.0
                    };
                    pcm_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            digest.update(&pcm_bytes);
            file.write_all(&bytes).unwrap();
        }
        file.sync_all().unwrap();
        drop(file);
        hashes.insert(name.into(), json!(file_hash(&target)));
        pcm_hashes.insert(name.into(), json!(format!("{:x}", digest.finalize())));
    }
    fs::write(
        path.join(".complete.json"),
        serde_json::to_vec(&json!({
            "schema":"stem-set-sha256-v1","source_version":version,"stems":hashes,
        }))
        .unwrap(),
    )
    .unwrap();
    json!(pcm_hashes)
}

fn copy_sealed(source: &Path, target: &Path) {
    let mut input = sealed_reader(source).unwrap();
    let mut output = fs::File::create(target).unwrap();
    let mut bytes = [0; 64 * 1024];
    loop {
        let count = input.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        output.write_all(&bytes[..count]).unwrap();
    }
    output.sync_all().unwrap();
}

fn verify_pair_files(
    project: &Path,
    pair: &crate::audio_engine::stem_pair::VerifiedStemPair,
    oracle: &Value,
) {
    for artifact in &pair.descriptor.content.artifacts {
        assert_eq!(
            file_hash(
                &project
                    .join(&pair.descriptor.wav_generation)
                    .join(format!("{}.wav", artifact.name))
            ),
            artifact.wav_sha256
        );
        let pcm = project
            .join(&pair.descriptor.pcm_generation)
            .join(format!("{}.f32le", artifact.name));
        assert_eq!(file_hash(&pcm), artifact.pcm_sha256);
        assert_eq!(fs::metadata(pcm).unwrap().len(), artifact.pcm_bytes);
        if !oracle.is_null() {
            assert_eq!(oracle[&artifact.name], artifact.pcm_sha256);
        }
    }
}

#[test]
#[ignore = "explicit productive multi-GB parent; set FLITZI_LONG_EVIDENCE_ROOT"]
fn x11_long_productive_parent() {
    let evidence = PathBuf::from(
        std::env::var_os("FLITZI_LONG_EVIDENCE_ROOT").expect("explicit evidence root"),
    );
    fs::create_dir_all(&evidence).unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let mut cases = vec![
        ("small", 144_007_usize, 4093_usize),
        ("medium", 8_000_013, 8191),
        ("large", 134_300_001, 65_521),
        ("boris", 16_103_550, 4093),
    ];
    let rf64 = PathBuf::from(
        std::env::var_os("FLITZI_LONG_RF64_SOURCE").expect("Python productive RF64 fixture"),
    );
    assert!(rf64.metadata().unwrap().len() > u64::from(u32::MAX));
    let rf64_frames = (rf64.metadata().unwrap().len() - 80) / 4;
    cases.push(("rf64", usize::try_from(rf64_frames).unwrap(), 16_381));
    let mut receipts = Vec::new();
    for (name, frames, partition) in cases {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let mut prior_pid = 0;
        for phase in ["prepare", "warm"] {
            let log_path = directory.join(format!("{phase}.log"));
            let log = fs::File::create(&log_path).unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", CHILD, "--nocapture"])
                .current_dir(&directory)
                .env("FLITZI_LONG_CHILD", &directory)
                .env("FLITZI_LONG_PHASE", phase)
                .env("FLITZI_LONG_CASE", name)
                .env("FLITZI_LONG_FRAMES", frames.to_string())
                .env("FLITZI_LONG_PARTITION", partition.to_string())
                .env("FLITZI_LONG_WORKSPACE", workspace)
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap();
            // Complete source + five aligned derivatives require many full I/O
            // passes. This isolated large-I/O limit is separate from normal tests.
            let deadline = Instant::now() + Duration::from_secs(1800);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    // A timeout is a failed proof, never permission to interrupt
                    // an owned child inside flush, rename or retained-reader I/O.
                    // Persist its exact ownership for controlled runner handling;
                    // dropping Child does not forcibly terminate that process.
                    let timeout = json!({
                        "case": name,
                        "phase": phase,
                        "pid": child.id(),
                        "parent_pid": std::process::id(),
                        "log": log_path,
                        "deadline_seconds": 1800,
                        "forced_termination": false,
                        "requires_controlled_owner_handling": true,
                    });
                    fs::write(
                        directory.join(format!("{phase}-timeout.json")),
                        serde_json::to_vec_pretty(&timeout).unwrap(),
                    )
                    .unwrap();
                    panic!(
                        "owned large-I/O child {} timed out; controlled owner handling required, no forced termination: {}",
                        child.id(),
                        fs::read_to_string(&log_path).unwrap()
                    );
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            assert!(
                status.success(),
                "{name}/{phase}: {}",
                fs::read_to_string(&log_path).unwrap()
            );
            let mut receipt: Value =
                serde_json::from_slice(&fs::read(directory.join(format!("{phase}.json"))).unwrap())
                    .unwrap();
            assert_eq!(receipt["pid"], child.id());
            assert_ne!(child.id(), std::process::id());
            assert_ne!(child.id(), prior_pid);
            prior_pid = child.id();
            assert_eq!(receipt["actual_public_load"], true);
            assert_eq!(receipt["own_callback_ack"], true);
            assert_eq!(receipt["full_frames"], frames);
            assert_eq!(receipt["actual_full_integrity"], true);
            assert_eq!(receipt["runtime_pcm_and_jobs_returned"], true);
            // Saved project ownership legitimately pins sealed cold files until
            // this child's process exits. Physical handle closure is separate
            // from its earlier runtime PCM/job/ACK checks.
            let owned_root = fs::canonicalize(&directory).unwrap();
            for value in receipt["sealed_owned_files"].as_array().unwrap() {
                let path = PathBuf::from(value.as_str().unwrap());
                let resolved = fs::canonicalize(&path).unwrap();
                assert!(
                    resolved.starts_with(&owned_root),
                    "foreign handle proof target"
                );
                assert!(resolved.is_file());
                let moved = path.with_extension("returned-proof");
                assert!(!moved.exists());
                fs::rename(&path, &moved).unwrap();
                fs::rename(&moved, &path).unwrap();
            }
            receipt["post_process_sealed_handles_returned"] = json!(true);
            receipts.push(receipt);
        }
    }
    fs::write(evidence.join("parent.json"),serde_json::to_vec_pretty(&json!({
        "pid":std::process::id(),"fresh_processes":true,"receipts":receipts,
        "scope":"Native/PyO3 actual public load, productive callback and fresh full sealed reopening; no devices/model inference",
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "helper executes only through configured productive parent"]
fn x11_long_productive_child() {
    let directory =
        PathBuf::from(std::env::var_os("FLITZI_LONG_CHILD").expect("parent supplies child root"));
    assert_eq!(
        fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
        fs::canonicalize(&directory).unwrap()
    );
    let phase = std::env::var("FLITZI_LONG_PHASE").unwrap();
    let case = std::env::var("FLITZI_LONG_CASE").unwrap();
    eprintln!(
        "LONG child {case}/{phase} pid {} started",
        std::process::id()
    );
    let frames: usize = std::env::var("FLITZI_LONG_FRAMES")
        .unwrap()
        .parse()
        .unwrap();
    let partition: usize = std::env::var("FLITZI_LONG_PARTITION")
        .unwrap()
        .parse()
        .unwrap();
    let source = directory.join("input.wav");
    let secondary_path = directory.join("overlap.wav");
    let state_path = directory.join("state.json");
    let mut state = if phase == "warm" {
        serde_json::from_slice::<Value>(&fs::read(&state_path).unwrap()).unwrap()
    } else {
        json!({})
    };
    if phase == "prepare" {
        if case == "rf64" {
            let input = PathBuf::from(std::env::var_os("FLITZI_LONG_RF64_SOURCE").unwrap());
            state["source"] = json!(input);
            state["source_oracle"] = rf64_oracle(&input, frames);
        } else if case == "boris" {
            let workspace = PathBuf::from(std::env::var_os("FLITZI_LONG_WORKSPACE").unwrap());
            let original = workspace.join("repo/samples/materials/Mb00c1c5936bc78609737f331467886e5/original/Boris Divider - boris_divider-metal_machine_combat.mp3");
            assert_eq!(
                file_hash(&original),
                "64d318544c17b649080bea55889b7e154be1c2cda35a481e21f99bd1a5b8165a"
            );
            let target = directory.join("Boris Divider - boris_divider-metal_machine_combat.mp3");
            copy_sealed(&original, &target);
            state["source"] = json!(target);
            let old_cache = workspace.join("repo/samples/materials/Mb00c1c5936bc78609737f331467886e5/.pcm-cache/v1/.ready-cefe5da8f44e685407aab5e313f5d567");
            state["source_oracle"] = json!({"original":file_hash(&original),
                "pcm":file_hash(&old_cache.join("playback.f32le")),"decoder":file_hash(&old_cache.join("decoder.f32le"))});
        } else {
            state["source_oracle"] = source_fixture(&source, frames, partition);
            state["source"] = json!(source);
        }
    }
    if phase == "prepare" && case == "large" {
        source_fixture(&secondary_path, 8_000_019, 8191);
        let mut second = fs::OpenOptions::new()
            .write(true)
            .open(&secondary_path)
            .unwrap();
        second.seek(SeekFrom::Start(44)).unwrap();
        second.write_all(&1_i16.to_le_bytes()).unwrap();
        second.sync_all().unwrap();
    }
    Python::initialize();
    let mut engine = AudioEngine::new().unwrap();
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(16);
    let producer = Arc::new(Mutex::new(producer));
    let (_, feedback_consumer) = rtrb::RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels =
        Some((producer.clone(), Arc::new(Mutex::new(feedback_consumer))));
    let baseline = crate::test_memory::reset_phase();
    let before_memory = memory();
    let started = Instant::now();
    let engine = Python::attach(|py| Py::new(py, engine).unwrap());
    let path = if phase == "warm" {
        state["cached_path"].as_str().unwrap()
    } else {
        state["source"].as_str().unwrap()
    };
    let request = Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs
            .set_item("run_analysis", case == "small" || case == "large")
            .unwrap();
        kwargs.set_item("replace_assignment", true).unwrap();
        kwargs.set_item("resident_loop_start_s", 0.25).unwrap();
        kwargs.set_item("resident_loop_end_s", 0.5).unwrap();
        kwargs
            .set_item(
                "source_intent",
                if phase == "warm" { "restore" } else { "import" },
            )
            .unwrap();
        engine
            .call_method(py, "load_sample_async", (1, path), Some(&kwargs))
            .unwrap()
            .extract::<u64>(py)
            .unwrap()
    });
    let secondary_request = if phase == "prepare" && case == "large" {
        Some(Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs.set_item("run_analysis", false).unwrap();
            kwargs.set_item("resident_loop_start_s", 0.25).unwrap();
            kwargs.set_item("resident_loop_end_s", 0.5).unwrap();
            kwargs.set_item("source_intent", "import").unwrap();
            engine
                .call_method(
                    py,
                    "load_sample_async",
                    (2, secondary_path.to_string_lossy().as_ref()),
                    Some(&kwargs),
                )
                .unwrap()
                .extract::<u64>(py)
                .unwrap()
        }))
    } else {
        None
    };
    let deadline = Instant::now() + Duration::from_secs(1200);
    let mut event = None;
    let mut callbacks = 0_u64;
    let mut secondary_finished = secondary_request.is_none();
    let mut worker_overlap = 0_usize;
    let mut delivered_owners = Vec::new();
    while event.is_none() || !secondary_finished {
        assert!(
            Instant::now() < deadline,
            "productive complete load timeout"
        );
        callback.assert_old_voice(&previous);
        let mut output = [0.0; 64];
        callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
            0,
            &mut callback.retirement,
        );
        assert!(output.iter().all(|n| n.is_finite()));
        assert!(output.iter().any(|n| *n != 0.0));
        callbacks += 1;
        callback.drain(&mut consumer);
        Python::attach(|py| {
            let value = engine.borrow(py);
            worker_overlap = worker_overlap.max(value.cold_jobs.counts_for_test().0);
            while let Ok(message) = value
                .loader_rx
                .lock()
                .unwrap()
                .try_recv()
                .map(LoaderEvent::into_test_payload)
            {
                match message {
                    LoaderEvent::Success {
                        id: 1, request_id, ..
                    } if request_id == request => {
                        event = Some(message);
                    }
                    LoaderEvent::Success {
                        id: 2,
                        request_id,
                        cached_path,
                        original_lease,
                        ..
                    } if Some(request_id) == secondary_request => {
                        let owner = original_lease.expect("secondary delivered original owner");
                        owner
                            .acknowledge(directory.join(cached_path).to_string_lossy().into_owned())
                            .unwrap();
                        delivered_owners.push(owner);
                        secondary_finished = true;
                    }
                    LoaderEvent::Error { error, .. } => {
                        panic!("actual public load failed: {error}")
                    }
                    _ => (),
                }
            }
        });
        std::thread::sleep(Duration::from_millis(2));
    }
    let LoaderEvent::Success {
        cached_path,
        analysis,
        analysis_error,
        original_lease,
        ..
    } = event.unwrap()
    else {
        unreachable!()
    };
    let owner = original_lease.expect("primary delivered original owner");
    owner
        .acknowledge(directory.join(&cached_path).to_string_lossy().into_owned())
        .unwrap();
    delivered_owners.push(owner);
    let settle_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let settled = Python::attach(|py| {
            let value = engine.borrow(py);
            let loading = value.loading_sample_ids.lock().unwrap();
            value.cold_loading[1].load(Ordering::Acquire) == 0
                && !loading.contains(&1)
                && (secondary_request.is_none()
                    || (value.cold_loading[2].load(Ordering::Acquire) == 0
                        && !loading.contains(&2)))
        });
        if settled {
            break;
        }
        assert!(
            Instant::now() < settle_deadline,
            "delivered loading guards did not return"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let (sample, lease, manifest) = Python::attach(|py| {
        let value = engine.borrow(py);
        // End each temporary guard before the public manifest method reacquires
        // the same source/cache locks.
        let sample = value.sample_cache.lock().unwrap()[1].clone().unwrap();
        let lease = value.cold_leases.lock().unwrap()[1].clone().unwrap();
        let manifest =
            serde_json::from_str::<Value>(&value.cold_source_manifest(1).unwrap().unwrap())
                .unwrap();
        (sample, lease, manifest)
    });
    eprintln!(
        "LONG child {case}/{phase} own load ACK and guards returned at {:?}",
        started.elapsed()
    );
    if phase == "warm" {
        assert!(
            lease.integrity.warm,
            "fresh reopen regenerated instead of validating warm source"
        );
        assert_eq!(state["cold_cache_path"], json!(lease.cache_path));
        assert_eq!(state["cold_identity"], lease.manifest.identity);
        assert_eq!(
            state["cold_decoder_identity"],
            lease.manifest.decoder_identity
        );
    } else {
        state["cold_cache_path"] = json!(lease.cache_path);
        state["cold_identity"] = json!(lease.manifest.identity);
        state["cold_decoder_identity"] = json!(lease.manifest.decoder_identity);
    }
    assert_eq!(sample.frame_count(), frames);
    assert_eq!(sample.resident_start(), 12_000);
    assert_eq!(sample.resident_end(), 24_000);
    assert_eq!(sample.samples.len() * 4, 96_000);
    callback.assert_old_voice(&previous);
    assert_eq!(
        manifest["descriptor"]["decoder"]["original"]["sha256"],
        state["source_oracle"]["original"]
    );
    assert_eq!(
        file_hash(&lease.cache_path.join("playback.f32le")),
        state["source_oracle"]["pcm"]
    );
    if case == "boris" {
        assert_eq!(
            file_hash(&lease.cache_path.join("decoder.f32le")),
            state["source_oracle"]["decoder"]
        );
    }
    if case == "boris" {
        state["source_oracle"]["mono"] = json!(independent::mono_hash(
            &lease.cache_path.join("playback.f32le"),
            2
        ));
    }
    assert_eq!(
        manifest["descriptor"]["playback"]["pcm"]["mono_sha256"],
        state["source_oracle"]["mono"]
    );
    if case == "rf64" {
        assert!(
            serde_json::to_string(&manifest)
                .unwrap()
                .contains("rf64-wave-ds64-flitzi-demux-v1")
        );
    }
    if case == "large" {
        assert!(analysis.is_none());
        assert!(
            analysis_error
                .as_ref()
                .is_some_and(|e| e.contains("analysis"))
        );
    }
    if case == "small" {
        assert!(
            analysis.is_some(),
            "automatic analysis was silently omitted: {analysis_error:?}"
        );
    }
    if secondary_request.is_some() {
        assert_eq!(worker_overlap, 2, "actual two-worker load overlap absent");
    }
    let load_metrics = crate::test_memory::snapshot(baseline);
    let load_memory = memory();
    assert!(
        load_metrics.1 < 128 * 1024 * 1024,
        "unbounded Rust upstream allocations: {load_metrics:?}"
    );
    let mut all_error = None;
    if case == "large" {
        // This scenario deliberately tests a user-selected residency ceiling;
        // automatic mode now follows actual host headroom rather than duration.
        Python::attach(|py| {
            let value = engine.borrow(py);
            value
                .project_assets
                .set_resident_budget_mib(&value.resident_account, 128)
                .unwrap();
        });
        assert!(
            lease
                .read_complete_cancellable(&sample, 1024 * 1024 * 1024, &|| false)
                .unwrap_err()
                .to_string()
                .contains("transient PCM limit")
        );
        let all_request = Python::attach(|py| {
            let kwargs = PyDict::new(py);
            kwargs.set_item("run_analysis", false).unwrap();
            kwargs.set_item("source_intent", "restore").unwrap();
            engine
                .call_method(
                    py,
                    "load_sample_async",
                    (3, cached_path.as_str()),
                    Some(&kwargs),
                )
                .unwrap()
                .extract::<u64>(py)
                .unwrap()
        });
        let all_deadline = Instant::now() + Duration::from_secs(1200);
        while all_error.is_none() {
            assert!(Instant::now() < all_deadline, "explicit ALL did not settle");
            callback.assert_old_voice(&previous);
            callback.drain(&mut consumer);
            Python::attach(|py| {
                let value = engine.borrow(py);
                while let Ok(message) = value
                    .loader_rx
                    .lock()
                    .unwrap()
                    .try_recv()
                    .map(LoaderEvent::into_test_payload)
                {
                    match message {
                        LoaderEvent::Error {
                            id: 3,
                            request_id,
                            error,
                            ..
                        } if request_id == all_request => all_error = Some(error),
                        LoaderEvent::Success {
                            id: 3, request_id, ..
                        } if request_id == all_request => {
                            panic!("ALL bypassed live residency budget")
                        }
                        _ => (),
                    }
                }
            });
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(all_error.as_ref().unwrap().contains("PCM"));
    }
    let pair_baseline = crate::test_memory::reset_phase();
    let pair_started = Instant::now();
    let old_live_bytes = previous.samples.len() * 4;
    let old_weak = Arc::downgrade(&previous.samples);
    let selected_weak = Arc::downgrade(&sample.samples);
    let source_handle_probe = lease.reader_lifetime_probe_for_test();
    let mut returned_files = vec![
        lease.original_path.clone(),
        lease.cache_path.join("decoder.f32le"),
        lease.cache_path.join("playback.f32le"),
        lease.cache_path.join("manifest.json"),
    ];
    let (secondary_weak, secondary_info) = if secondary_request.is_some() {
        let (other, other_lease) = Python::attach(|py| {
            let value = engine.borrow(py);
            let other = value.sample_cache.lock().unwrap()[2].clone().unwrap();
            let other_lease = value.cold_leases.lock().unwrap()[2].clone().unwrap();
            (other, other_lease)
        });
        assert_eq!(other.frame_count(), 8_000_019);
        assert_eq!(
            (other.resident_start(), other.resident_end()),
            (12_000, 24_000)
        );
        assert_eq!(other.samples.len() * 4, 96_000);
        assert_ne!(other_lease.manifest.identity, lease.manifest.identity);
        returned_files.push(other_lease.original_path.clone());
        for name in ["decoder.f32le", "playback.f32le", "manifest.json"] {
            returned_files.push(other_lease.cache_path.join(name));
        }
        let info = json!({"frames":other.frame_count(),"live_bytes":other.samples.len()*4,
            "request_id":secondary_request,"identity":other_lease.manifest.identity,
            "original_sha256":file_hash(&other_lease.original_path),
            "playback_sha256":file_hash(&other_lease.cache_path.join("playback.f32le")),
            "full_decoder_verify_bytes":other_lease.integrity.decoder_verify_bytes,
            "full_playback_verify_bytes":other_lease.integrity.playback_verify_bytes});
        (Some(Arc::downgrade(&other.samples)), info)
    } else {
        (None, Value::Null)
    };
    let integrity = json!({
        "source_copied_bytes":lease.integrity.source_copied_bytes,
        "incoming_verify_bytes":lease.integrity.incoming_verify_bytes,
        "snapshot_verify_bytes":lease.integrity.snapshot_verify_bytes,
        "original_verify_bytes":lease.integrity.original_verify_bytes,
        "original_copied_bytes":lease.integrity.original_copied_bytes,
        "decoder_verify_bytes":lease.integrity.decoder_verify_bytes,
        "playback_verify_bytes":lease.integrity.playback_verify_bytes,
        "manifest_verify_bytes":lease.integrity.manifest_verify_bytes,
        "cumulative_playback_read_bytes":lease.integrity.playback_read_bytes,
        "stream_scratch_reserved_bytes":lease.integrity.stream_scratch_reserved_bytes,
        "warm":lease.integrity.warm,
    });
    let alignment_scratch_bound =
        crate::audio_engine::stem_cache::streamed::alignment_scratch_upper_bound(RATE, frames)
            .unwrap();
    let (pair_descriptor, component_live_bytes, _saved_pair_assets) = if case == "rf64" {
        verify_native_rf64(&lease.original_path, frames, &state["source_oracle"]);
        (Value::Null, 0, Vec::new())
    } else {
        let material = PreparedMigrationMaterial::from_current(
            &directory.join("samples"),
            sample.clone(),
            lease.clone(),
        )
        .unwrap();
        let version = format!(
            "{cached_path}|sha256-v1:{}",
            state["source_oracle"]["original"].as_str().unwrap()
        );
        let wav_reference = if phase == "prepare" {
            let material_reference =
                crate::audio_engine::material_paths::canonical_original_root_reference(
                    Path::new(&cached_path),
                    lease.material_id.as_ref().unwrap(),
                )
                .unwrap();
            format!(
                "{}/stems/.ready-0123456789abcdef0123456789abcdef",
                material_reference.to_string_lossy().replace('\\', "/")
            )
        } else {
            state["wav_reference"].as_str().unwrap().into()
        };
        let wav_directory = directory.join(&wav_reference);
        let mut stem_oracle = if phase == "prepare" {
            if case == "boris" {
                fs::create_dir_all(&wav_directory).unwrap();
                let workspace = PathBuf::from(std::env::var_os("FLITZI_LONG_WORKSPACE").unwrap());
                let old=workspace.join("repo/samples/materials/Mb00c1c5936bc78609737f331467886e5/stems/.ready-1f1cfdb25ef14154b7fe30f78254dfeb");
                let mut hashes = serde_json::Map::new();
                for name in STEM_FILE_NAMES {
                    let target = wav_directory.join(format!("{name}.wav"));
                    copy_sealed(&old.join(format!("{name}.wav")), &target);
                    hashes.insert(name.into(), json!(file_hash(&target)));
                }
                fs::write(wav_directory.join(".complete.json"),serde_json::to_vec(&json!({"schema":"stem-set-sha256-v1","source_version":version,"stems":hashes})).unwrap()).unwrap();
                Value::Null
            } else {
                generated_stems(&wav_directory, frames, partition, &version)
            }
        } else {
            state["stem_oracle"].clone()
        };
        // Both producer and fresh warm subscriber enter through the actual
        // public source ticket and pair receiver after their own callback ACK.
        let (ticket, prepared) = Python::attach(|py| {
            let ticket = engine
                .call_method1(py, "capture_prepared_source", (1, version.clone()))
                .unwrap()
                .extract::<Py<PreparedSourceTicket>>(py)
                .unwrap();
            assert_eq!(ticket.borrow(py).publication_status(), "captured");
            let descriptor_reference = (phase == "warm")
                .then(|| state["descriptor_reference"].as_str().unwrap().to_owned());
            let prepared = engine
                .call_method1(
                    py,
                    "prepare_stem_pair",
                    (
                        1,
                        version.clone(),
                        wav_reference.clone(),
                        ticket.bind(py),
                        true,
                        descriptor_reference,
                    ),
                )
                .unwrap()
                .extract::<Py<PreparedStemPair>>(py)
                .unwrap();
            (ticket, prepared)
        });
        let (selection, bytes, component_weak) = Python::attach(|py| {
            let value = prepared.borrow(py);
            assert!(value.has_components());
            let components = value.component_views_for_test().unwrap();
            assert!(components.iter().all(|component| {
                component.samples.len() == sample.samples.len()
                    && component.channels == sample.channels
                    && component.resident_start() == sample.resident_start()
                    && component.resident_end() == sample.resident_end()
                    && component.frame_count() == sample.frame_count()
            }));
            for index in 0..components.len() {
                assert!(!Arc::ptr_eq(&components[index].samples, &sample.samples));
                assert!(components[..index].iter().all(|previous| {
                    !Arc::ptr_eq(&previous.samples, &components[index].samples)
                }));
            }
            let bytes = components
                .iter()
                .map(|component| component.samples.len() * std::mem::size_of::<f32>())
                .sum::<usize>();
            assert_eq!(bytes, 4 * sample.samples.len() * std::mem::size_of::<f32>());
            let weak: [_; crate::messages::STEM_BUFFER_COUNT] =
                std::array::from_fn(|index| Arc::downgrade(&components[index].samples));
            (
                serde_json::from_str::<Value>(value.selection_json()).unwrap(),
                bytes,
                weak,
            )
        });
        // A saved selection must acquire all three real output owners before
        // selecting the result. These owners contain paths, never resident PCM.
        let saved_pair_assets = Python::attach(|py| {
            let owners = ["wav_generation", "pcm_generation", "descriptor_reference"].map(|key| {
                engine
                    .call_method1(
                        py,
                        "acquire_project_asset_lease",
                        (directory
                            .join(selection[key].as_str().unwrap())
                            .to_string_lossy()
                            .into_owned(),),
                    )
                    .unwrap()
                    .extract::<Py<ProjectAssetLease>>(py)
                    .unwrap()
            });
            prepared.call_method0(py, "select").unwrap();
            assert_eq!(ticket.borrow(py).publication_status(), "captured");
            Vec::from(owners)
        });
        // The independent full-file oracle reads an ordinary verified sealed
        // reader; it does not substitute for the public preparation above.
        let pair = open_verified_pair(
            &directory.join("samples"),
            selection["descriptor_reference"].as_str().unwrap(),
            &material,
            &|| false,
        )
        .unwrap();
        assert_eq!(selection["wav_generation"], pair.descriptor.wav_generation);
        assert_eq!(selection["pcm_generation"], pair.descriptor.pcm_generation);
        assert_eq!(
            selection["stem_set_identity"],
            pair.descriptor.stem_set_identity
        );
        if case != "boris" {
            assert_eq!(pair.descriptor.content.alignment.offset_frames, 0);
        }
        if case == "boris" {
            let offset = independent::independent_boris_offset(
                &lease.cache_path.join("playback.f32le"),
                &wav_directory,
                frames,
            );
            assert_eq!(
                pair.descriptor.content.alignment.offset_frames,
                offset as i64
            );
            let mut hashes = serde_json::Map::new();
            for name in STEM_FILE_NAMES {
                hashes.insert(
                    name.into(),
                    json!(independent::aligned_pcm_hash(
                        &wav_directory.join(format!("{name}.wav")),
                        frames,
                        offset
                    )),
                );
            }
            stem_oracle = json!(hashes);
        }
        verify_pair_files(&directory, &pair, &stem_oracle);
        let descriptor = serde_json::to_value(&pair.descriptor).unwrap();
        for name in STEM_FILE_NAMES {
            returned_files.push(
                directory
                    .join(&pair.descriptor.wav_generation)
                    .join(format!("{name}.wav")),
            );
            returned_files.push(
                directory
                    .join(&pair.descriptor.pcm_generation)
                    .join(format!("{name}.f32le")),
            );
        }
        returned_files.push(
            directory
                .join(&pair.descriptor.wav_generation)
                .join(".complete.json"),
        );
        returned_files.push(
            directory
                .join(&pair.descriptor.pcm_generation)
                .join("manifest.json"),
        );
        returned_files.push(directory.join(&pair.descriptor_reference));
        state["wav_reference"] = selection["wav_generation"].clone();
        state["descriptor_reference"] = json!(pair.descriptor_reference);
        state["pair_selection"] = selection;
        state["stem_oracle"] = stem_oracle;
        drop(pair);
        drop(material);
        // Drop PyO3 results under an attached interpreter so neither a deferred
        // decref nor the source ticket masks runtime PCM ownership ending.
        Python::attach(|_py| {
            drop(prepared);
            drop(ticket);
        });
        assert!(component_weak.iter().all(|pcm| pcm.upgrade().is_none()));
        (descriptor, bytes, saved_pair_assets)
    };
    eprintln!(
        "LONG child {case}/{phase} complete public pair/integrity returned at {:?}",
        started.elapsed()
    );
    let pair_metrics = crate::test_memory::snapshot(pair_baseline);
    assert!(
        pair_metrics.1 < 128 * 1024 * 1024,
        "complete pair arrays: {pair_metrics:?}"
    );
    let pair_memory = memory();
    let saved_owner_status =
        Python::attach(|py| engine.borrow(py).project_assets.status().unwrap());
    drop(sample);
    drop(lease);
    drop(delivered_owners);
    Python::attach(|py| {
        let mut value = engine.borrow_mut(py);
        value.shut_down().unwrap();
        assert_eq!(value.cold_jobs.counts_for_test(), (0, 0, 0));
    });
    drop(callback);
    drop(previous);
    Python::attach(|_py| drop(engine));
    assert!(old_weak.upgrade().is_none());
    assert!(selected_weak.upgrade().is_none());
    assert!(
        secondary_weak
            .as_ref()
            .is_none_or(|pcm| pcm.upgrade().is_none())
    );
    let mut receipt = json!({"pid":std::process::id(),"phase":phase,"case":case,"full_frames":frames,"full_pcm_bytes":frames*8,
        "source_oracle":state["source_oracle"],"stem_oracle":state["stem_oracle"],"partition":partition,
        "actual_public_load":true,"own_callback_ack":true,"actual_full_integrity":true,"old_voice_render_callbacks":callbacks,
        "maximum_actual_cold_workers":worker_overlap,"secondary_actual_load_success":secondary_request.map(|_|secondary_finished),
        "secondary_source":secondary_info,"secondary_runtime_pcm_returned":secondary_weak.as_ref().map(|_|true),
        "source_manifest":manifest,"pair":pair_descriptor,"analysis_error":analysis_error,"analysis_present":analysis.is_some(),"explicit_ALL_error":all_error,
        "actual_public_pair":case != "rf64","pair_selection":state["pair_selection"],"retained_saved_pair_asset_owners":_saved_pair_assets.len(),
    });
    let observations = json!({
        "selected_live_bytes":96_000,"old_voice_live_bytes":old_live_bytes,
        "selected_component_live_bytes":component_live_bytes,"post_shutdown_workers_queue_admitted":[0,0,0],"runtime_pcm_and_jobs_returned":true,
        "sealed_owned_files":returned_files,"durable_project_owner_status_before_shutdown":saved_owner_status,
        "source_sealed_handles_closed_before_process_exit":source_handle_probe(),
        "sealed_handle_lifetime":"acknowledged ProjectAssets saved_claim retains complete cold sealed files until project registry/process exit; no runtime PCM or cold jobs retained",
        "source_integrity_work":integrity,"alignment_scratch_upper_bound":alignment_scratch_bound,
        "load_allocator_live_peak_largest":load_metrics,"pair_allocator_live_peak_largest":pair_metrics,
        "memory_before":before_memory,"memory_after_load":load_memory,"memory_after_pair":pair_memory,
        "seconds":started.elapsed().as_secs_f64(),"pair_seconds":pair_started.elapsed().as_secs_f64(),
        "scope":"actual Rust System allocations and Windows process counters separate; no Demucs/model/device/hearing claim"});
    receipt
        .as_object_mut()
        .unwrap()
        .extend(observations.as_object().unwrap().clone());
    state["cached_path"] = json!(cached_path);
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    fs::write(
        directory.join(format!("{phase}.json")),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
