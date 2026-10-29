//! Real canonical pair preparation and stopped component residency join the
//! original captured GLOBAL group and its final-output capture. The independent
//! oracle retains authored PCM16 component words, not a productive source plan.
//! No codec, CPAL, device, App selection or hearing acceptance is claimed.

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::material_migration::{
    PreparedMigrationMaterial, prepare_material_complete_for_test,
};
use crate::audio_engine::material_migration_control::{adopt_for_format, prepared_for_test};
use crate::audio_engine::material_paths;
use crate::audio_engine::resident_relocation::{
    WindowRequest, prepare_window_with_producer, reconcile,
};
use crate::audio_engine::source_reader::{FrameRange, StemRenderSelection};
use crate::audio_engine::stem_cache::STEM_FILE_NAMES;
use crate::messages::{LoaderEvent, StemMixMode};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

const CHANNELS: usize = 2;
const SOURCE_FRAMES: usize = 8_192;
const RECORDED_FRAMES: usize = 5_003;
const REGIONS: [(usize, usize); 2] = [(17, 1_517), (731, 2_528)];
const MASKS: [u8; 2] = [0b0101, 0b1010];
const READY_GENERATION: &str = "d82f7101c45b4e4eb97b21f0084e1503";

fn wait_for(description: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + CAPTURE_WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "{description} timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn authored_components() -> [Vec<i16>; 4] {
    std::array::from_fn(|component| {
        (0..SOURCE_FRAMES)
            .flat_map(|frame| {
                (0..CHANNELS).map(move |channel| {
                    let phase =
                        frame as f64 * (0.031 + component as f64 * 0.017) + channel as f64 * 0.41;
                    (phase.sin() * (1_700.0 + component as f64 * 500.0)).round() as i16
                })
            })
            .collect()
    })
}

fn sum_words(components: &[Vec<i16>; 4], mask: u8) -> Vec<i16> {
    (0..SOURCE_FRAMES * CHANNELS)
        .map(|sample| {
            let sum: i32 = components
                .iter()
                .enumerate()
                .filter(|(component, _)| mask & (1 << *component) != 0)
                .map(|(_, words)| i32::from(words[sample]))
                .sum();
            i16::try_from(sum).expect("the independent source sum fits PCM16")
        })
        .collect()
}

fn wav_words(words: &[i16]) -> Vec<u8> {
    assert_eq!(words.len(), SOURCE_FRAMES * CHANNELS);
    let data_bytes = u32::try_from(words.len() * 2).unwrap();
    let align = u16::try_from(CHANNELS * 2).unwrap();
    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * u32::from(align)).to_le_bytes());
    bytes.extend_from_slice(&align.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn write_stem_artifacts(path: &Path, version: &str, components: &[Vec<i16>; 4]) {
    fs::create_dir_all(path).unwrap();
    let mut hashes = serde_json::Map::new();
    for (component, name) in STEM_FILE_NAMES.iter().enumerate() {
        let encoded = if component < components.len() {
            wav_words(&components[component])
        } else {
            // The existing pair contract includes Instrumental on disk, while
            // only the four component readers may belong to an audible voice.
            wav_words(&sum_words(components, 0b1110))
        };
        fs::write(path.join(format!("{name}.wav")), &encoded).unwrap();
        hashes.insert(
            (*name).to_owned(),
            json!(format!("{:x}", Sha256::digest(&encoded))),
        );
    }
    fs::write(
        path.join(".complete.json"),
        serde_json::to_vec(&json!({
            "schema": "stem-set-sha256-v1", "source_version": version, "stems": hashes,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn independent_selected_mix(components: &[Vec<i16>; 4], frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|frame| {
            (0..CHANNELS).map(move |channel| {
                let mut mix = 0.0_f32;
                for (pad, (start, end)) in REGIONS.into_iter().enumerate() {
                    let source_frame = start + frame % (end - start);
                    let mut selected = 0.0_f32;
                    for (component, words) in components.iter().enumerate() {
                        if MASKS[pad] & (1 << component) != 0 {
                            let word = words[source_frame * CHANNELS + channel];
                            // Independent PCM16 decoder policy: signed minimum
                            // is exactly -1; other values use the signed maximum.
                            selected += if word == i16::MIN {
                                -1.0
                            } else {
                                f32::from(word) / f32::from(i16::MAX)
                            };
                        }
                    }
                    mix += selected;
                }
                mix
            })
        })
        .collect()
}

fn capture_current_regions(fixture: &Fixture) -> CapturedGlobalStart {
    Python::attach(|py| {
        let entries = REGIONS
            .into_iter()
            .enumerate()
            .map(|(id, (start, end))| {
                let binding = fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                assert!(binding.current() && binding.available());
                (
                    Py::new(py, binding).unwrap(),
                    start as f64 / f64::from(RATE),
                    Some(end as f64 / f64::from(RATE)),
                    None,
                )
            })
            .collect();
        fixture
            .control
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    })
}

fn run_prepared_stem_group(multi_loop: bool) {
    let mut rig = CaptureRig::with_fixture(Fixture::new_format(false, RATE, CHANNELS), 8);
    let root = rig.directory.as_ref().unwrap().path().join("samples");
    fs::create_dir(&root).unwrap();
    let components = authored_components();
    let original = root.join("known-component-sum.wav");
    fs::write(&original, wav_words(&sum_words(&components, 0b1111))).unwrap();
    let material =
        prepare_material_complete_for_test(&root, &original, RATE, CHANNELS, &|| false).unwrap();
    let metadata = material.metadata();
    let version = format!(
        "{}|sha256-v1:{}",
        metadata["new_reference"].as_str().unwrap(),
        metadata["original"]["sha256"].as_str().unwrap()
    );
    let material_reference = material_paths::canonical_original_root_reference(
        Path::new(metadata["new_reference"].as_str().unwrap()),
        metadata["material_id"].as_str().unwrap(),
    )
    .unwrap();
    let wav_reference = format!(
        "{}/stems/.ready-{READY_GENERATION}",
        material_reference.to_string_lossy().replace('\\', "/")
    );
    let wav_path = root.parent().unwrap().join(&wav_reference);
    write_stem_artifacts(&wav_path, &version, &components);
    let preparation = prepared_for_test(
        &rig.fixture.control.engine,
        PreparedMigrationMaterial::from_current(
            &root,
            material.sample.clone(),
            material.lease.clone(),
        )
        .unwrap(),
        &root,
    )
    .unwrap();
    let adoptions = [0, 1].map(|id| {
        adopt_for_format(
            &rig.fixture.control.engine,
            id,
            &preparation,
            rig.fixture.control.producer.clone(),
            (CHANNELS, RATE, root.clone()),
        )
        .unwrap()
    });
    wait_for("both original source publications", || {
        rig.fixture.control.consumer.slots() >= 2 && rig.fixture.control.producer.try_lock().is_ok()
    });
    assert_eq!(fixed_capture_callback(&mut rig.fixture, &mut []), 2);
    let mut source_successes = [false; 2];
    wait_for("both actual source adoption results", || {
        while let Ok(event) = rig
            .fixture
            .control
            .engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
        {
            match event.into_test_payload() {
                LoaderEvent::Success { request_id, .. } => {
                    for (id, ticket) in adoptions.iter().enumerate() {
                        if request_id == ticket.request_id() {
                            source_successes[id] = true;
                        }
                    }
                }
                LoaderEvent::Error {
                    request_id, error, ..
                } => {
                    assert!(
                        adoptions
                            .iter()
                            .all(|ticket| ticket.request_id() != request_id),
                        "actual source adoption failed: {error:?}"
                    );
                }
                _ => {}
            }
        }
        source_successes.into_iter().all(|seen| seen)
            && [0, 1]
                .into_iter()
                .all(|id| rig.fixture.control.engine.cold_loading[id].load(Ordering::Acquire) == 0)
    });
    for ticket in &adoptions {
        assert_eq!(ticket.phase().unwrap(), "acknowledged");
        assert!(ticket.is_current().unwrap());
    }
    preparation.release_preparation().unwrap();
    let original_owner = rig
        .fixture
        .control
        .engine
        .project_assets
        .acquire(&root, &material.lease.original_path)
        .unwrap();

    // Each actual resident edit owns its independent finite source window. The
    // immutable original sample and complete pair remain available off realtime.
    let mut windows = Vec::with_capacity(2);
    for (id, (start, end)) in REGIONS.into_iter().enumerate() {
        let window = prepare_window_with_producer(
            &rig.fixture.control.engine,
            id,
            WindowRequest {
                loop_region: Some((
                    start as f64 / f64::from(RATE),
                    Some(end as f64 / f64::from(RATE)),
                )),
                key_lock: Some(false),
                ..WindowRequest::default()
            },
            rig.fixture.control.producer.clone(),
        )
        .unwrap();
        wait_for("original finite resident window preparation", || {
            (rig.fixture.control.consumer.peek().is_ok() || window.error().unwrap().is_some())
                && rig.fixture.control.producer.try_lock().is_ok()
        });
        assert_eq!(
            window.publication_status(),
            "pending",
            "{:?}",
            window.error()
        );
        assert_eq!(fixed_capture_callback(&mut rig.fixture, &mut []), 1);
        assert_eq!(
            window.publication_status(),
            "accepted",
            "{:?}",
            window.error()
        );
        reconcile(&rig.fixture.control.engine).unwrap();
        assert!(window.is_current());
        let source = rig.fixture.mixer.bank_for_measurement()[id]
            .as_ref()
            .unwrap();
        assert_eq!(
            (source.resident_start(), source.resident_end()),
            (start, end)
        );
        assert_eq!(source.frame_count(), SOURCE_FRAMES);
        windows.push(window);
    }

    // Publish MULTI LOOP and both new current source/loop rows through the
    // existing runtime owner before taking any preparation or press snapshots.
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    for (id, (start, end)) in REGIONS.into_iter().enumerate() {
        loaded[id] = true;
        starts[id] = start as f64 / f64::from(RATE);
        ends[id] = Some(end as f64 / f64::from(RATE));
        bindings[id] = rig
            .fixture
            .control
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap();
    }
    let before_mode_voices: Vec<_> = rig
        .fixture
        .mixer
        .voices
        .iter()
        .map(|voice| (voice.active, voice.generation, voice.frame_pos))
        .collect();
    assert!(before_mode_voices.iter().all(|voice| !voice.0));
    let before_mode_frame = rig.fixture.transport.output_frame();
    let before_mode_starts = (rig.fixture.starts(0), rig.fixture.starts(1));
    let transition = rig
        .fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_pitch_projection(
            multi_loop,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![PitchPadProjection::default(); NUM_SAMPLES]),
        )
        .unwrap();
    if multi_loop {
        assert!(
            transition.is_none(),
            "the unchanged ON image has no collapse effect"
        );
    } else {
        let transition =
            transition.expect("the initial ON image must admit exactly one OFF collapse");
        assert_eq!(transition.publication_status(), "pending");
        assert_eq!(fixed_capture_callback(&mut rig.fixture, &mut []), 1);
        assert_eq!(transition.publication_status(), "accepted");
        assert!(rig.fixture.control.consumer.is_empty());
        assert_eq!(rig.fixture.transport.output_frame(), before_mode_frame);
        assert_eq!(
            (rig.fixture.starts(0), rig.fixture.starts(1)),
            before_mode_starts
        );
        assert_eq!(
            rig.fixture
                .mixer
                .voices
                .iter()
                .map(|voice| (voice.active, voice.generation, voice.frame_pos))
                .collect::<Vec<_>>(),
            before_mode_voices,
            "settling the initial silent collapse must not create or retrigger a voice"
        );
    }

    let mut source_tickets = Vec::with_capacity(2);
    let mut pairs = Vec::with_capacity(2);
    let mut saved = Vec::with_capacity(2);
    let mut registrations = Vec::with_capacity(2);
    let mut residencies = Vec::with_capacity(2);
    for (id, mask) in MASKS.into_iter().enumerate() {
        let engine = &rig.fixture.control.engine;
        let source = engine.capture_prepared_source(id, version.clone()).unwrap();
        let pair = engine
            .prepare_stem_pair_at_root(&root, id, &version, &wav_reference, &source, false, None)
            .unwrap();
        assert!(
            !pair.has_components(),
            "disk preparation must not invent a live mask ACK"
        );
        assert_eq!(
            pair.pair_reader.descriptor.content.alignment.offset_frames,
            0
        );
        let selection: serde_json::Value = serde_json::from_str(pair.selection_json()).unwrap();
        let selected_wav = material_paths::resolve(
            &root,
            Path::new(selection["wav_generation"].as_str().unwrap()),
        )
        .unwrap()
        .path;
        saved.push(engine.project_assets.acquire(&root, &selected_wav).unwrap());
        pair.select();
        let registration = engine
            .register_committed_stem_pair_with_producer(
                &pair,
                &source,
                &rig.fixture.control.producer,
            )
            .unwrap();
        assert_eq!(registration.publication_status(), "pending");
        assert_eq!(fixed_capture_callback(&mut rig.fixture, &mut []), 1);
        assert_eq!(
            registration.publication_status(),
            "accepted",
            "{:?}",
            registration.error()
        );
        assert!(registration.is_current());
        assert!(rig.fixture.mixer.stems_for_measurement()[id].is_none());
        let residency = rig
            .fixture
            .control
            .engine
            .request_committed_stem_residency_with_producer(
                &registration,
                "all_stems",
                mask,
                &rig.fixture.control.producer,
            )
            .unwrap();
        assert_eq!(residency.effective_mode_status(), "waiting");
        assert_eq!(fixed_capture_callback(&mut rig.fixture, &mut []), 1);
        wait_for("actual component residency worker", || {
            (rig.fixture.control.consumer.peek().is_ok() || residency.error().is_some())
                && rig.fixture.control.producer.try_lock().is_ok()
        });
        assert_eq!(
            residency.residency_status(),
            "pending",
            "{:?}",
            residency.error()
        );
        assert_eq!(fixed_capture_callback(&mut rig.fixture, &mut []), 1);
        // The ordinary callback service, rather than a test-owned ACK setter,
        // finishes the stopped preparation's bookkeeping on a real silent frame.
        let mut silent = [0.0; CHANNELS];
        fixed_capture_callback(&mut rig.fixture, &mut silent);
        assert_eq!(silent, [0.0; CHANNELS]);
        reconcile(&rig.fixture.control.engine).unwrap();
        assert_eq!(
            residency.residency_status(),
            "accepted",
            "{:?}",
            residency.error()
        );
        assert!(residency.is_current());
        assert_eq!(residency.effective_mode_status(), "waiting");
        assert_eq!(
            rig.fixture.mixer.stem_demand_for_measurement(id),
            (
                StemMixMode::AllStems,
                mask,
                crate::audio_engine::stem_cache::source_version_hash(&version)
            )
        );
        source_tickets.push(source);
        pairs.push(pair);
        registrations.push(registration);
        residencies.push(residency);
    }
    let adopted: [_; 2] = std::array::from_fn(|id| {
        rig.fixture.mixer.stems_for_measurement()[id]
            .clone()
            .unwrap()
    });
    let captured_sources: [_; 2] = std::array::from_fn(|id| {
        rig.fixture.mixer.bank_for_measurement()[id]
            .clone()
            .unwrap()
    });
    for id in [0, 1] {
        assert!(registrations[id].is_current());
        assert!(residencies[id].is_current());
        assert_eq!(adopted[id].frame_count, SOURCE_FRAMES);
        assert_eq!(adopted[id].channels, CHANNELS);
        assert!(
            adopted[id]
                .stems
                .iter()
                .all(|stem| stem.same_window(&captured_sources[id]))
        );
    }
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    assert!(
        rig.fixture
            .render_to(1_000)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    let captured = capture_current_regions(&rig.fixture);
    drain_grid(&mut rig.fixture);
    let execution = 24_000;
    let stop = execution + RECORDED_FRAMES as u64;
    assert_eq!(captured.original_frame(), Some(execution));
    let mut expected = vec![0.0; 7 * CHANNELS];
    expected.extend(independent_selected_mix(&components, RECORDED_FRAMES + 60));
    let mut around = vec![0.0; expected.len()];
    let mut lead = [0.0; 512 * CHANNELS];
    rig.fixture.feedback.reserve(128);
    fixed_capture_callback(&mut rig.fixture, &mut []);
    let feedback_capacity = rig.fixture.feedback.capacity();
    let feedback_address = rig.fixture.feedback.as_ptr();

    // Actual retirement pressure must retain this same original FIFO group and
    // take without admitting either prefix. Its close worker is released off RT.
    let mut pressure = RetirementPause::install(&mut rig.fixture);
    pressure.fill(&mut rig.fixture);
    let batch = rig.publish(&captured);
    assert_eq!(
        measured_capture_callback(&mut rig.fixture, &mut [], "stem group retirement refusal"),
        0
    );
    assert!(rig.fixture.scheduler.is_empty());
    assert_eq!(batch.publication_status(), "pending");
    assert!(!rig.status().started);
    assert_eq!(rig.status().first_frame, None);
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (0, 0));
    pressure.release();
    wait_for("original stem group retirement admission", || {
        measured_capture_callback(&mut rig.fixture, &mut [], "stem group FIFO admission") == 1
    });
    assert_eq!(
        rig.fixture.scheduler.len(),
        1,
        "one existing PitchAction, no capture-start event"
    );
    assert_eq!(
        rig.fixture.scheduler.peek_next_target_frame(),
        Some(execution)
    );
    assert!(rig.status().start_queued && !rig.status().started);
    wait_for("actual prepared stem group plans", || {
        measured_capture_callback(
            &mut rig.fixture,
            &mut [],
            "stem group prepared-result polling",
        );
        rig.fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .ordinary_group_ready(2)
    });
    while rig.fixture.transport.output_frame() < execution - 7 {
        let frames = (execution - 7 - rig.fixture.transport.output_frame()).min(512) as usize;
        measured_capture_callback(
            &mut rig.fixture,
            &mut lead[..frames * CHANNELS],
            "stem group silent lead",
        );
        assert!(
            lead[..frames * CHANNELS]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
    assert!(
        rig.fixture
            .control
            .engine
            .stop_mix_capture_internal(&rig.ticket, stop)
            .unwrap()
    );
    let mut cursor = 0;
    let mut callback = 0;
    let mut admitted_stop = 0;
    let total_frames = around.len() / CHANNELS;
    while cursor < total_frames {
        let frames = [19, 127, 509, 31, 1_024][callback % 5].min(total_frames - cursor);
        admitted_stop += measured_capture_callback(
            &mut rig.fixture,
            &mut around[cursor * CHANNELS..(cursor + frames) * CHANNELS],
            "prepared stems attached claim/commit/loop/capture stop",
        );
        cursor += frames;
        callback += 1;
        if callback == 1 {
            assert_eq!(batch.publication_status(), "accepted");
            assert_eq!(captured.state(), "applied");
            assert_eq!(rig.status().first_frame, Some(execution));
            assert_eq!(
                rig.fixture
                    .scheduler
                    .capture_stop_target_frame(rig.ticket.session_id()),
                Some(stop)
            );
            for member in &captured.owner.members {
                assert_eq!(member.start.effective_frame(), Some(execution));
            }
            for id in [0, 1] {
                let index = rig
                    .fixture
                    .mixer
                    .voices
                    .iter()
                    .position(|voice| voice.active && voice.sample_id == id)
                    .unwrap();
                let snapshot = rig
                    .fixture
                    .mixer
                    .capture_pitch_voice_snapshot(index)
                    .unwrap();
                assert_eq!(
                    snapshot.plan.loop_region,
                    FrameRange {
                        start: REGIONS[id].0,
                        end: REGIONS[id].1
                    }
                );
                assert_eq!(snapshot.plan.frame_pos, REGIONS[id].0 + 12);
                assert_eq!(
                    snapshot.plan.selection,
                    StemRenderSelection::for_stems(&adopted[id], MASKS[id])
                );
                assert!(!snapshot.plan.transition.is_active());
                assert!(snapshot.sample.same_source(&captured_sources[id]));
                assert!(snapshot.sample.same_window(&captured_sources[id]));
                let held = snapshot.stems.as_ref().unwrap();
                assert!(Arc::ptr_eq(
                    &held.complete_set_identity,
                    &adopted[id].complete_set_identity
                ));
                assert!(Arc::ptr_eq(
                    &held.reference_samples,
                    &adopted[id].reference_samples
                ));
                assert_eq!(snapshot.playback.tempo_ratio(), 1.0);
                assert!(!rig.fixture.mixer.key_lock_for_measurement(id));
            }
        }
    }
    assert_eq!(
        admitted_stop, 1,
        "the immutable own stop S is admitted exactly once"
    );
    assert_eq!(rig.fixture.feedback.capacity(), feedback_capacity);
    assert_eq!(rig.fixture.feedback.as_ptr(), feedback_address);
    assert_eq!((rig.fixture.starts(0), rig.fixture.starts(1)), (1, 1));
    assert_eq!(captured.original_frame(), Some(execution));
    let maximum = around
        .iter()
        .zip(&expected)
        .map(|(actual, wanted)| (*actual - *wanted).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        maximum < 2.0e-6,
        "multi_loop={multi_loop}: every-frame independent selected-stem PCM mismatch {maximum}"
    );
    // Both unequal loop boundaries occur multiple times within the compared
    // complete extent; no attack prefix or boundary neighborhood is discarded.
    for (start, end) in REGIONS {
        assert!(2 * (end - start) < RECORDED_FRAMES);
    }
    let snapshot = rig.joined();
    assert!(snapshot.successful_spool);
    assert_eq!(snapshot.failure, None);
    assert_eq!(
        (snapshot.first_frame, snapshot.end_frame),
        (Some(execution), Some(stop))
    );
    assert_eq!(
        (snapshot.submitted_frames, snapshot.written_frames),
        (RECORDED_FRAMES as u64, RECORDED_FRAMES as u64)
    );
    let recorded = &around[7 * CHANNELS..(7 + RECORDED_FRAMES) * CHANNELS];
    assert_eq!(
        rig.bits(),
        recorded
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
    assert!(recorded.iter().all(|sample| sample.is_finite()));
    assert!(recorded.iter().any(|sample| sample.abs() > 0.04));
    assert_eq!(
        rig.fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        2,
        "capture stop must preserve both independently looping voices"
    );
    // All original preparation/reader owners survive the measured render and
    // actual RT release + exclusive-writer join. They grant no replacement ACK.
    drop((
        source_tickets,
        pairs,
        registrations,
        residencies,
        windows,
        saved,
        original_owner,
        adoptions,
        material,
    ));
}

#[test]
fn multi_loop_on_prepared_stem_group_and_capture_share_exact_birth_and_both_loop_boundaries() {
    run_prepared_stem_group(true);
}

#[test]
fn multi_loop_off_prepared_stem_group_and_capture_share_exact_birth_and_both_loop_boundaries() {
    run_prepared_stem_group(false);
}
