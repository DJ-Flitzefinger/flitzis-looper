//! Actual postcommit action custody through a superseding public Unload.
//! Only the later config fsync fails; pair workers, writer, ACKs and cleanup run.

use super::*;

const CHILD: &str = "FLITZI_L4_DISK_SETTLEMENT_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::l4_disk_settlement_tests::actual_disk_settlement_keeps_both_generations_until_unload_is_durable";

struct PairFiles {
    areas: [PathBuf; 3],
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl PairFiles {
    fn capture(root: &Path, selection: &Value) -> Self {
        let areas = ["wav_generation", "pcm_generation", "descriptor_reference"]
            .map(|field| selected_path(root, selection, field));
        let mut files = Vec::new();
        for area in &areas {
            if area.is_file() {
                files.push((area.clone(), fs::read(area).unwrap()));
            } else {
                for entry in fs::read_dir(area).unwrap() {
                    let path = entry.unwrap().path();
                    assert!(path.is_file());
                    files.push((path.clone(), fs::read(path).unwrap()));
                }
            }
        }
        assert_eq!(files.len(), 13, "complete five WAV/five PCM plus metadata");
        Self { areas, files }
    }

    fn assert_retained(&self) {
        for (path, bytes) in &self.files {
            assert_eq!(&fs::read(path).unwrap(), bytes, "{}", path.display());
        }
    }
}

pub(super) fn deferred_after_v1(
    engine: &Arc<AudioEngine>,
    root: &Path,
    source: (&str, &str, usize),
    playback: &mut ControllerPlayback,
    lanes: (
        &mut rtrb::Consumer<ControlMessage>,
        &mut rtrb::Consumer<ControlParameterMessage>,
    ),
    locals: &Py<PyDict>,
    old: &FinalReaderProbe,
) {
    let (consumer, parameters) = lanes;
    assert!(playback.callback.retirement.actual.is_some());
    assert!(playback.callback.retirement.retired.is_empty());
    prepare_public_unload_loader(locals);
    let reference = version_wav_files(root, source.0, source.1, source.2, FixtureSet::Second);
    // Only WAV fixture setup runs here. The real pool first creates and seals
    // V2's complete pair; no setup PreparedStemPair or selected flag owns it.
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("settlement_reference", reference)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
import os
import threading
from flitzis_looper.controller.stem_pair_preparation import StemPairIntentBinding
settlement_entry = StemCacheEntry(source_version=fixture['source_version'],
    cache_dir=settlement_reference, stems=expected_stem_files(settlement_reference))
controller._assets.sync_assignments()
controller.bind_disk_selection_commit(persistence.commit_stem_selection, persistence.flush)
settlement_workers = audio.worker_call_count()
settlement_other_path, settlement_other_content = project.sample_paths[215], project.pad_content[215]
controller._pair_preparations.submit(0, settlement_entry,
    audio.capture_prepared_source(0, fixture['source_version']), project.pad_content[0].instance_id,
    project.stem_cache[0], components=False,
    binding=StemPairIntentBinding(source_path=project.sample_paths[0]))
"#,
    );
    wait_until(|| {
        Python::attach(|py| {
            py.eval(
                c"controller._pair_preparations.pending[0].prepared is not None",
                Some(locals.bind(py)),
                None,
            )
            .unwrap()
            .extract::<bool>()
            .unwrap()
        })
    });
    controller_python(
        locals,
        r#"
settlement_replace, settlement_fsync = os.replace, os.fsync
settlement_written = []
settlement_failures = 0
settlement_control = threading.get_ident()
settlement_atomic_write = persistence._atomic_write_text
settlement_config_depth = 0
def actual_settlement_config_write(content, *, before_replace=None):
    global settlement_config_depth
    assert threading.get_ident() == settlement_control
    settlement_config_depth += 1
    try:
        return settlement_atomic_write(content, before_replace=before_replace)
    finally:
        settlement_config_depth -= 1
def unload_after_actual_config_replace(source, destination):
    global disk_v2
    settlement_replace(source, destination)
    if Path(destination).absolute() == persistence.config_path.absolute():
        settlement_written.append(ProjectPersistence.from_config_path(persistence.config_path).project)
        if len(settlement_written) == 1:
            disk_v2 = settlement_written[0].stem_cache[0].pair
            assert disk_v2 is not None and disk_v2 != disk_v1
            assert disk_v2 == controller._disk_selection_settlements[0].entry.pair
            loader.unload_sample(0)
            assert project.sample_paths[0] is None and project.stem_cache[0] is None
def reject_superseding_config_fsync(descriptor):
    global settlement_failures
    if settlement_written and settlement_config_depth and threading.get_ident() == settlement_control:
        settlement_failures += 1
        raise OSError('injected superseding Unload config fsync failure')
    settlement_fsync(descriptor)
os.replace, os.fsync = unload_after_actual_config_replace, reject_superseding_config_fsync
persistence._atomic_write_text = actual_settlement_config_write
"#,
    );
    old.wait_for("actual returned pair and deferred disk settlement", || {
        controller_python(locals, "controller.on_frame_render()");
        Python::attach(|py| {
            py.eval(
                c"0 in controller._disk_selection_settlements",
                Some(locals.bind(py)),
                None,
            )
            .unwrap()
            .extract::<bool>()
            .unwrap()
        })
    });
    let pair_reader = Python::attach(|py| {
        let pair = py
            .eval(
                c"controller._disk_selection_settlements[0].request.prepared",
                Some(locals.bind(py)),
                None,
            )
            .unwrap();
        pair.extract::<PyRef<PreparedStemPair>>()
            .unwrap()
            .reader_lifetime_for_test()
    });
    let selection: Value = Python::attach(|py| {
        serde_json::from_str(
            &locals
                .bind(py)
                .get_item("disk_v2")
                .unwrap()
                .unwrap()
                .call_method0("model_dump_json")
                .unwrap()
                .extract::<String>()
                .unwrap(),
        )
        .unwrap()
    });
    assert_complete_selection(root, &selection);
    let current = PairFiles::capture(root, &selection);
    controller_python(
        locals,
        r#"
assert len(settlement_written) == 1 and settlement_failures > 0
assert settlement_config_depth == 0
assert not controller._pair_preparations.pending
assert audio.worker_call_count() == settlement_workers + 1 and audio.prepared_only_on_pool_threads()
assert not controller._committed_residency.contains(0)
assert not any(key[1] == 0 for key in controller._assets._assignments)
assert 'owner settlement deferred' in session.stem_generation_errors[0]
del first, previous_control
"#,
    );
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let assert_other = |playback: &ControllerPlayback| {
        let other = other_voice(&playback.callback.mixer);
        let phase = (playback.frame as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
    };
    // The public intent has changed, but no callback ACK has stopped V1 yet.
    // Its genuine complete PCM/Native/FIFO/EQ oracle remains productive.
    for frames in [1, 137, 257] {
        playback.render(frames, false);
        old.assert_retained();
        current.assert_retained();
        assert_other(playback);
    }
    let feedback_before = playback.callback.feedback.events.len();
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert_eq!(
        playback.drain(consumer),
        4,
        "own Unload and normal defaults"
    );
    assert!(consumer.is_empty());
    assert_eq!(playback.callback.feedback.events.len(), feedback_before + 1);
    assert!(matches!(
        playback.callback.feedback.events[feedback_before],
        AudioMessage::SampleStopped { id: 0 }
    ));
    assert!(playback.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    old.wait_for("actual Unload audio worker completion", || {
        completion.strong_count() == 0
    });
    old.wait_for("actual predecessor final Native/history readers", || {
        let frames = 137;
        let mut output = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        assert_other(playback);
        controller_python(
            locals,
            "loader.poll_loader_events()\ncontroller.on_frame_render()",
        );
        old.assert_files_retained();
        current.assert_retained();
        old.identity.strong_count() == 0 && old.pcm.iter().all(|pcm| pcm.strong_count() == 0)
    });
    assert_eq!(
        pair_reader.strong_count(),
        1,
        "only actual settlement owns its complete V2 reader"
    );
    for path in [&old.areas[1], &current.areas[1]] {
        assert!(fs::write(path.join("vocals.f32le"), b"settlement leased overwrite").is_err());
    }
    let sentinel = root.join("materials/M88888888888888888888888888888888/original/sentinel.wav");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    fs::write(&sentinel, stereo_wav(64)).unwrap();
    let mut pin = engine.project_assets.acquire_pin(root, &sentinel).unwrap();
    engine
        .project_assets
        .retire(root, &sentinel, false)
        .unwrap();
    pin.release();
    drop(pin);
    old.wait_for(
        "shared asset worker progresses while actual settlement holds both sets",
        || {
            old.assert_files_retained();
            current.assert_retained();
            !sentinel.exists()
        },
    );
    controller_python(
        locals,
        r#"
def assert_actual_settlement():
    action = controller._disk_selection_settlements[0]
    assert not action.lease.released and action.previous_lease is not None
    assert not action.previous_lease.released
    assert action.request.lease.released
    assert action.retirement.remaining > 0 and controller._assets._reserved == action.retirement.remaining
    assert action.request.prepared.has_components() is False
    assert project.sample_paths[0] is None and project.stem_cache[0] is None
    restored = ProjectPersistence.from_config_path(persistence.config_path)
    assert restored.load_error is None and restored.project.stem_cache[0].pair == disk_v2
    assert restored.project.sample_paths[0] is not None
    assert restored.project.sample_paths[215] == settlement_other_path
    assert restored.project.pad_content[215] == settlement_other_content
    assert len(settlement_written) == 1
assert_actual_settlement()
# End no action/request/prepared alias here: the function's local owners returned.
os.fsync = settlement_fsync
controller.on_frame_render()
assert not controller._disk_selection_settlements
assert controller._assets._reserved == 0 and not persistence._dirty
assert len(settlement_written) == 2
assert settlement_written[1].sample_paths[0] is None and settlement_written[1].stem_cache[0] is None
assert not controller._committed_residency.contains(0)
assert audio.worker_call_count() == settlement_workers + 1
os.replace = settlement_replace
persistence._atomic_write_text = settlement_atomic_write
settlement_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert settlement_reopened.load_error is None
assert settlement_reopened.project.sample_paths[0] is None and settlement_reopened.project.stem_cache[0] is None
assert settlement_reopened.project.sample_paths[215] == settlement_other_path
assert settlement_reopened.project.pad_content[215] == settlement_other_content
"#,
    );
    old.wait_for(
        "actual settlement final complete reader and both generation cleanup",
        || {
            controller_python(
                locals,
                "loader.poll_loader_events()\ncontroller.on_frame_render()",
            );
            pair_reader.strong_count() == 0
                && old
                    .areas
                    .iter()
                    .chain(&current.areas)
                    .all(|path| !path.exists())
        },
    );
    assert_eq!(old.identity.strong_count(), 0);
    assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
    assert!(engine.project_assets.status().unwrap().3.is_empty());
    assert!(consumer.is_empty() && parameters.is_empty());
    assert_other(playback);
    controller_python(
        locals,
        "assert not controller._assets.retirement_errors and not controller._assets._pending\nassert not session.sample_load_errors\nassert audio.worker_call_count() == settlement_workers + 1",
    );
}

#[test]
fn actual_disk_settlement_keeps_both_generations_until_unload_is_durable() {
    if std::env::var_os(CHILD).is_some() {
        round_trip(
            true,
            None,
            true,
            None,
            false,
            CandidateTrace::DiskSettlementFinalReader,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L4 disk settlement child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}
