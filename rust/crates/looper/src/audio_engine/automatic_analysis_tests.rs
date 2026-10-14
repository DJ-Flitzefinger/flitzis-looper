//! Complete automatic analysis through immutable import and the actual source ACK.
//! Opt-in MP3 children read only the explicitly authorized input; no device or App.
use super::*;
use serde_json::{Value, json};
use std::io::Read;

use crate::audio_engine::c3_process_metrics as process;

const SYNTHETIC_TEST: &str = "audio_engine::cold_load::tests::automatic_analysis_tests::automatic_analysis_public_import_has_complete_grid_and_own_ack";
const PRIVATE_TEST: &str = "audio_engine::cold_load::tests::automatic_analysis_tests::automatic_analysis_private_mp3_complete_import_and_peak";
const CHILD: &str = "FLITZI_AUTOMATIC_ANALYSIS_CHILD_RATE";
const PRIVATE_MP3: &str =
    "C:/SCRATCHBEATS/Third_sight-rhymes_like_a_scientist_(instrumental)-ftd.mp3";

struct CurrentDirectory(PathBuf);

impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn child(test: &str, rate: u32, ignored: bool) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--nocapture", "--test-threads=1"]);
    if ignored {
        command.arg("--ignored");
    }
    let output = command.env(CHILD, rate.to_string()).output().unwrap();
    assert!(
        output.status.success(),
        "complete automatic analysis child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
}

fn owned_directory() -> (tempfile::TempDir, CurrentDirectory) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let directory = tempfile::tempdir_in(workspace.join("scratch")).unwrap();
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(crate::audio_engine::material_paths::ordinary_windows_path(
        directory.path(),
    ))
    .unwrap();
    (directory, previous)
}

fn hash_file(path: &Path) -> String {
    let mut input = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0; 65_536];
    loop {
        let count = input.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    format!("{:x}", hash.finalize())
}

// Independently encoded eighteen-second stereo attack train, as in the existing
// productive incoming history. No productive audio decoder generates this input.
fn pulse_track(path: &Path, rate: u32) {
    let frames = 18 * rate as usize;
    let data = u32::try_from(frames * 4).unwrap();
    let mut bytes = Vec::with_capacity(data as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for frame in 0..frames {
        let beat_frame = frame.saturating_sub(1024) % (rate as usize / 2);
        let words = if frame < 1024 {
            [0_i16, 0]
        } else if beat_frame < 256 {
            let value = ((beat_frame * 37 % 31) as i32 - 15) * (256 - beat_frame) as i32;
            [value as i16, (-value / 2) as i16]
        } else {
            [
                ((frame * 13 % 511) as i16) - 255,
                ((frame * 19 % 503) as i16) - 251,
            ]
        };
        for value in words {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    fs::write(path, bytes).unwrap();
}

fn assert_grid(values: &[f32], duration: f64, name: &str) {
    assert!(!values.is_empty(), "empty complete {name}");
    assert!(
        values
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0 && f64::from(*value) <= duration),
        "invalid complete {name}"
    );
    assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
}

fn import_and_ack(source: &Path, output_rate: u32, public_incoming: bool) -> Value {
    Python::initialize();
    let mut engine = AudioEngine::new().unwrap();
    assert!(engine.stream_handle.is_none());
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    if output_rate != 48_000 {
        // old()/Callback::new establish a genuine 48k predecessor. Rebind that
        // fixture to the alternate format before attempting its own START;
        // source_current must still enforce the productive rate identity.
        engine.loaded_source_generations.lock().unwrap()[0] = (7, output_rate);
        engine
            .input_runtime_ownership
            .publish_test_source(0, &previous, output_rate, 7);
        callback.mixer = RtMixer::new(2, output_rate as f32);
        callback
            .mixer
            .set_input_runtime_ownership(engine.input_runtime_ownership.clone());
        callback
            .mixer
            .set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
        callback
            .mixer
            .load_sample_rt(0, previous.clone(), &mut callback.retirement);
        assert!(callback.mixer.play_sample(0, 1.0));
    }
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let (_feedback, feedback_consumer) = rtrb::RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels =
        Some((producer.clone(), Arc::new(Mutex::new(feedback_consumer))));
    let runtime = process::runtime_identity().unwrap();
    let before = process::snapshot();
    let stop = Arc::new(AtomicBool::new(false));
    let sampler_stop = stop.clone();
    let sampler = std::thread::spawn(move || {
        let mut peak_private = 0;
        while !sampler_stop.load(Ordering::Acquire) {
            peak_private = peak_private.max(
                process::snapshot()["private_committed_bytes"]
                    .as_u64()
                    .unwrap(),
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        peak_private
    });
    let started = Instant::now();
    let request = if public_incoming {
        assert_eq!(output_rate, 48_000, "actual headless public API format");
        engine
            .load_incoming_sample_async(0, source.to_string_lossy().into_owned(), false)
            .unwrap()
    } else {
        admit_for_format(
            &engine,
            0,
            source.to_string_lossy().into_owned(),
            true,
            false,
            true,
            producer,
            2,
            output_rate,
            std::env::current_dir().unwrap().join("samples"),
        )
        .unwrap()
    };
    // Full CPU CQT/BPM work may precede publication for minutes. This deadline
    // bounds only the opt-in integration fixture; the native ACK stays unchanged.
    wait_until_for(Duration::from_secs(3600), || {
        while let Ok(event) = engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
            .map(LoaderEvent::into_test_payload)
        {
            assert!(
                !matches!(
                    event,
                    LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
                ),
                "terminal before its actual source ACK: {event:?}"
            );
        }
        consumer.peek().is_ok()
    });
    callback.assert_old_voice(&previous);
    assert_eq!(
        engine.input_runtime_ownership.cold_status(0, request),
        Some(0)
    );
    let (adoption, expected_defaults) = match consumer.peek().unwrap() {
        ControlMessage::LoadColdSample {
            id,
            source_generation,
            adoption,
            incoming_defaults,
            ..
        } => {
            assert_eq!((*id, *source_generation), (0, request));
            assert_eq!(adoption.load(Ordering::Acquire), 0);
            (adoption.clone(), *incoming_defaults)
        }
        _ => panic!("missing productive source command"),
    };
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    let event = terminal(&engine, request);
    let LoaderEvent::Success {
        id,
        request_id,
        duration_s,
        analysis,
        analysis_error,
        incoming_defaults,
        ..
    } = event
    else {
        panic!("actual import failed: {event:?}");
    };
    assert_eq!((id, request_id), (0, request));
    assert!(analysis_error.is_none(), "{analysis_error:?}");
    assert_eq!(incoming_defaults.is_some(), public_incoming);
    if let Some(defaults) = incoming_defaults {
        let expected = expected_defaults.unwrap();
        assert_eq!(defaults.revision, expected.revision);
        assert_eq!(defaults.sample_rate_hz, output_rate);
        assert_eq!(defaults.period_seconds, expected.period_seconds);
        assert_eq!(defaults.start_frame, expected.start_frame);
        assert_eq!(defaults.end_frame, expected.end_frame);
    }
    let analysis = analysis.expect("complete automatic analysis result");
    assert!(analysis.bpm.is_finite() && analysis.bpm > 0.0);
    assert!(
        !analysis.key.is_empty(),
        "key result must retain truthful unknown"
    );
    assert_grid(&analysis.beat_grid.beats, duration_s, "beats");
    assert_grid(&analysis.beat_grid.downbeats, duration_s, "downbeats");
    assert_grid(&analysis.beat_grid.bars, duration_s, "bars");
    wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(
        engine.input_runtime_ownership.cold_status(0, request),
        Some(2)
    );
    assert_eq!(
        engine.loaded_source_generations.lock().unwrap()[0],
        (request, output_rate)
    );
    let after = process::snapshot();
    stop.store(true, Ordering::Release);
    let sampled_peak_private = sampler.join().unwrap();
    let leases = engine.cold_leases.lock().unwrap();
    let lease = leases[0].as_ref().unwrap();
    let descriptor = &lease.manifest.descriptor;
    let decoder = &descriptor["decoder"]["pcm"];
    let playback = &descriptor["playback"]["pcm"];
    let frames = playback["full_frames"].as_u64().unwrap();
    assert_eq!(playback["rate_hz"], output_rate);
    assert_eq!(playback["channels"], 2);
    assert_eq!(duration_s, frames as f64 / f64::from(output_rate));
    for (name, pcm) in [("decoder", decoder), ("playback", playback)] {
        let path = lease.cache_path.join(format!("{name}.f32le"));
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            pcm["full_frames"].as_u64().unwrap() * pcm["channels"].as_u64().unwrap() * 4,
        );
        assert_eq!(hash_file(&path), pcm["interleaved_sha256"]);
    }
    assert_eq!(hash_file(&lease.original_path), hash_file(source));
    let result = json!({
        "test":"complete-automatic-analysis-own-import-ack-v1",
        "pid":std::process::id(), "public_incoming_api":public_incoming,
        "request_id":request, "own_source_ack":true, "analysis_error":null,
        "elapsed_seconds":started.elapsed().as_secs_f64(), "duration_seconds":duration_s,
        "actual_symphonia_decoder_pcm":decoder, "actual_playback_pcm":playback,
        "bpm":analysis.bpm, "key":analysis.key,
        "key_scope":"actual result; unknown remains unknown; no model substitution or musical label acceptance",
        "beats":analysis.beat_grid.beats, "downbeats":analysis.beat_grid.downbeats,
        "bars":analysis.beat_grid.bars, "runtime":runtime,
        "process_before":before, "process_after":after, "process_delta":process::delta(&before, &after),
        "sampled_peak_private_bytes":sampled_peak_private,
        "private_sampling_interval_ms":1,
        "memory_scope":"fresh isolated process lifetime peak RSS/commit; sampled private is an observed lower bound, not an allocation proof",
    });
    drop(leases);
    engine.shut_down().unwrap();
    result
}

#[test]
fn automatic_analysis_public_import_has_complete_grid_and_own_ack() {
    if std::env::var_os(CHILD).is_none() {
        child(SYNTHETIC_TEST, 48_000, false);
        return;
    }
    let (directory, _cwd) = owned_directory();
    for source_rate in [44_100, 48_000] {
        let source = directory.path().join(format!("pulse-{source_rate}.wav"));
        pulse_track(&source, source_rate);
        let evidence = import_and_ack(&source, 48_000, true);
        assert_eq!(
            evidence["actual_symphonia_decoder_pcm"]["rate_hz"],
            source_rate
        );
        assert_eq!(
            evidence["actual_symphonia_decoder_pcm"]["full_frames"],
            18 * u64::from(source_rate)
        );
        assert_eq!(evidence["duration_seconds"], 18.0);
        println!("AUTOMATIC_ANALYSIS {}", evidence);
    }
}

#[test]
#[ignore = "explicit exact authorized private MP3 via FLITZI_ANALYSIS_MP3; complete CPU analysis"]
fn automatic_analysis_private_mp3_complete_import_and_peak() {
    let source =
        PathBuf::from(std::env::var_os("FLITZI_ANALYSIS_MP3").expect("explicit private input"));
    assert_eq!(
        source,
        Path::new(PRIVATE_MP3),
        "only the exact authorized MP3 is permitted"
    );
    assert!(source.is_absolute());
    let Some(rate) = std::env::var_os(CHILD) else {
        for rate in [44_100, 48_000] {
            child(PRIVATE_TEST, rate, true);
        }
        return;
    };
    let rate: u32 = rate.to_string_lossy().parse().unwrap();
    assert!(matches!(rate, 44_100 | 48_000));
    let (_directory, _cwd) = owned_directory();
    let evidence = import_and_ack(&source, rate, rate == 48_000);
    assert_eq!(evidence["actual_symphonia_decoder_pcm"]["rate_hz"], 44_100);
    assert_eq!(evidence["actual_symphonia_decoder_pcm"]["channels"], 2);
    assert!(evidence["duration_seconds"].as_f64().unwrap() > 376.0);
    assert!(evidence["duration_seconds"].as_f64().unwrap() < 377.0);
    println!("AUTOMATIC_ANALYSIS {}", evidence);
}
