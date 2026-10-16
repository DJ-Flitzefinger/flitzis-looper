//! Selected ALL/wet Hold + Pair uses real source, disk-selection and component ACKs.
//! A second independently admitted renderer retains the unmoved chronology. This
//! bounded device-free fixture does not establish musical, resource or Human gates.
#![cfg(windows)]

use super::{
    EMPTY, LaneEvidence, OTHER, RATE, RetirementGate, SOURCE, create_audio_buffer_retirement,
    create_stream_mixer, drain_control_messages, render_scheduled_audio,
};
use crate::audio_engine::AudioEngine;
use crate::audio_engine::buffer_retirement::AudioBufferRetirementWorker;
use crate::audio_engine::committed_stem_residency::{CommittedStemSetTicket, ResidentStemTicket};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::content_placement::{PairPlacementKind, PlacementObservation};
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket};
use crate::audio_engine::input_runtime_binding;
use crate::audio_engine::material_migration::{
    PreparedMigrationMaterial, prepare_material_complete_for_test,
};
use crate::audio_engine::material_migration_control;
use crate::audio_engine::material_paths;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::pad_pair::{PadPairTicket, control};
use crate::audio_engine::prepared_source::PreparedSourceTicket;
use crate::audio_engine::project_assets::ProjectAssetLease;
use crate::audio_engine::resident_relocation::{
    WindowRequest, prepare_window_with_producer, reconcile,
};
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::stem_cache::{STEM_FILE_NAMES, source_version_hash};
use crate::audio_engine::stem_pair_preparation::PreparedStemPair;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ControlMessage, LoaderEvent, PreparedStemSet, StemMixMode};
use pyo3::Python;
use pyo3::types::PyInt;
use rtrb::{Consumer, Producer, RingBuffer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FRAMES: usize = 4096;
const LOOP_START: usize = 17;
const LOOP_END: usize = 1517;
const ENABLED_MASK: u8 = 0b1010;
const OTHER_ENABLED_MASK: u8 = 0b0101;
const RATIO: f64 = 0.73;
const GENERATION: &str = "0123456789abcdef0123456789abcdef";

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "actual preparation did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn signal_wav(layers: &[usize]) -> Vec<u8> {
    let data_bytes = u32::try_from(FRAMES * 4).unwrap();
    let mut encoded = Vec::with_capacity(44 + data_bytes as usize);
    encoded.extend_from_slice(b"RIFF");
    encoded.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    encoded.extend_from_slice(b"WAVEfmt ");
    encoded.extend_from_slice(&16_u32.to_le_bytes());
    encoded.extend_from_slice(&1_u16.to_le_bytes());
    encoded.extend_from_slice(&2_u16.to_le_bytes());
    encoded.extend_from_slice(&RATE.to_le_bytes());
    encoded.extend_from_slice(&(RATE * 4).to_le_bytes());
    encoded.extend_from_slice(&4_u16.to_le_bytes());
    encoded.extend_from_slice(&16_u16.to_le_bytes());
    encoded.extend_from_slice(b"data");
    encoded.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..FRAMES {
        for channel in 0..2 {
            let sum: i32 = layers
                .iter()
                .map(|layer| {
                    let phase =
                        frame as f64 * (0.031 + *layer as f64 * 0.017) + channel as f64 * 0.41;
                    (phase.sin() * (1700.0 + *layer as f64 * 500.0)).round() as i32
                })
                .sum();
            encoded.extend_from_slice(&i16::try_from(sum).unwrap().to_le_bytes());
        }
    }
    encoded
}

fn write_verified_wavs(path: &Path, version: &str) {
    fs::create_dir_all(path).unwrap();
    let mut hashes = serde_json::Map::new();
    for (index, name) in STEM_FILE_NAMES.iter().enumerate() {
        let encoded = if index == 4 {
            signal_wav(&[1, 2, 3])
        } else {
            signal_wav(&[index])
        };
        fs::write(path.join(format!("{name}.wav")), &encoded).unwrap();
        hashes.insert(
            (*name).to_owned(),
            json!(format!("{:x}", Sha256::digest(encoded))),
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

struct SelectedWetFixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: Consumer<ControlMessage>,
    mixer: Box<RtMixer>,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: crate::messages::TriggerQuantization,
    feedback: Vec<AudioMessage>,
    retirement: RetirementGate,
    _worker: AudioBufferRetirementWorker,
    selected: CommittedStemSetTicket,
    all: ResidentStemTicket,
    pair: PreparedStemPair,
    selection: Value,
    source: PreparedSourceTicket,
    other_selected: CommittedStemSetTicket,
    other_all: ResidentStemTicket,
    other_pair: PreparedStemPair,
    other_selection: Value,
    other_source: PreparedSourceTicket,
    version: String,
    _saved: ProjectAssetLease,
    _other_saved: ProjectAssetLease,
    _material: PreparedMigrationMaterial,
    _temp: tempfile::TempDir,
}

impl SelectedWetFixture {
    fn new() -> Self {
        Python::initialize();
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap()
            .join("scratch/goal-r2-native-composition-20261014");
        fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("selected-all-wet-")
            .tempdir_in(scratch)
            .unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let original = root.join("original.wav");
        // The source is the actual sum of the four component signals. Alignment
        // therefore verifies the physical loop rather than moving it into padding.
        fs::write(&original, signal_wav(&[0, 1, 2, 3])).unwrap();
        let material =
            prepare_material_complete_for_test(&root, &original, RATE, 2, &|| false).unwrap();
        let metadata = material.metadata();
        let version = format!(
            "{}|sha256-v1:{}",
            metadata["new_reference"].as_str().unwrap(),
            metadata["original"]["sha256"].as_str().unwrap()
        );
        let original_root = material_paths::canonical_original_root_reference(
            Path::new(metadata["new_reference"].as_str().unwrap()),
            metadata["material_id"].as_str().unwrap(),
        )
        .unwrap();
        let wav_reference = format!(
            "{}/stems/.ready-{GENERATION}",
            original_root.to_string_lossy().replace('\\', "/")
        );
        let wav_path = root.parent().unwrap().join(&wav_reference);
        write_verified_wavs(&wav_path, &version);

        let mut engine = AudioEngine::new().unwrap();
        let (producer, mut consumer) = RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let (_, feedback) = RingBuffer::new(32);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
        engine.test_output_format = Some((RATE, 2));
        let mut mixer = create_stream_mixer(
            2,
            RATE,
            engine.current_timing_acknowledgements.clone(),
            engine.input_runtime_ownership.clone(),
            engine.prepared_source_epochs.clone(),
        )
        .unwrap();
        let (retirement, worker) = create_audio_buffer_retirement();
        let mut retirement = RetirementGate {
            available: usize::MAX,
            inner: retirement,
        };
        let mut scheduler = FixedCapacityScheduler::new();
        let mut transport = TransportTimeline::new(RATE);
        let mut quantization = crate::messages::TriggerQuantization::Immediate;
        let mut feedback = Vec::new();
        let preparation = material_migration_control::prepared_for_test(
            &engine,
            PreparedMigrationMaterial::from_current(
                &root,
                material.sample.clone(),
                material.lease.clone(),
            )
            .unwrap(),
            &root,
        )
        .unwrap();
        let requests = [SOURCE, OTHER].map(|id| {
            material_migration_control::adopt_for_format(
                &engine,
                id,
                &preparation,
                producer.clone(),
                (2, RATE, root.clone()),
            )
            .unwrap()
        });
        wait_until(|| consumer.slots() >= 2 && producer.try_lock().is_ok());
        assert_eq!(
            drain_control_messages(
                &mut consumer,
                &mut scheduler,
                0,
                &mut quantization,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut retirement,
            ),
            2
        );
        let request_ids = requests.each_ref().map(|request| request.request_id());
        let mut seen = Vec::new();
        wait_until(|| {
            while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
                match event.into_test_payload() {
                    LoaderEvent::Success { request_id, .. }
                        if request_ids.contains(&request_id) =>
                    {
                        seen.push(request_id);
                    }
                    LoaderEvent::Error {
                        request_id, error, ..
                    } => {
                        assert!(
                            !request_ids.contains(&request_id),
                            "source adoption: {error}"
                        );
                    }
                    _ => {}
                }
            }
            seen.len() == 2
                && [SOURCE, OTHER]
                    .into_iter()
                    .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
        });
        assert!(requests.iter().all(|request| request.is_current().unwrap()));
        preparation.release_preparation().unwrap();

        for id in [SOURCE, OTHER] {
            let window = prepare_window_with_producer(
                &engine,
                id,
                WindowRequest {
                    key_lock: Some(true),
                    loop_region: Some((
                        LOOP_START as f64 / f64::from(RATE),
                        Some(LOOP_END as f64 / f64::from(RATE)),
                    )),
                    ..WindowRequest::default()
                },
                producer.clone(),
            )
            .unwrap();
            wait_until(|| {
                (consumer.peek().is_ok() || window.error().unwrap().is_some())
                    && producer.try_lock().is_ok()
            });
            assert_eq!(
                drain_control_messages(
                    &mut consumer,
                    &mut scheduler,
                    0,
                    &mut quantization,
                    &mut transport,
                    &mut mixer,
                    &mut feedback,
                    &mut retirement,
                ),
                1
            );
            assert_eq!(
                window.publication_status(),
                "accepted",
                "{:?}",
                window.error()
            );
            reconcile(&engine).unwrap();
            assert!(mixer.key_lock_for_measurement(id));
        }

        let source = engine
            .capture_prepared_source(SOURCE, version.clone())
            .unwrap();
        let pair = engine
            .prepare_stem_pair_at_root(
                &root,
                SOURCE,
                &version,
                &wav_reference,
                &source,
                false,
                None,
            )
            .unwrap();
        assert!(!pair.has_components());
        assert_eq!(
            pair.pair_reader.descriptor.content.alignment.offset_frames,
            0
        );
        let selection = serde_json::from_str::<Value>(pair.selection_json()).unwrap();
        let saved = engine.project_assets.acquire(&root, &wav_path).unwrap();
        pair.select();
        let selected = engine
            .register_committed_stem_pair_with_producer(&pair, &source, &producer)
            .unwrap();
        assert_eq!(selected.publication_status(), "pending");
        assert_eq!(
            drain_control_messages(
                &mut consumer,
                &mut scheduler,
                0,
                &mut quantization,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut retirement,
            ),
            1
        );
        assert_eq!(selected.publication_status(), "accepted");
        assert!(selected.is_current());
        assert!(mixer.stems_for_measurement()[SOURCE].is_none());

        // B verifies the same immutable disk selection through its own current
        // source admission and readers. Material sharing grants no registration,
        // component, mask, voice or history authority to the other content.
        let other_source = engine
            .capture_prepared_source(OTHER, version.clone())
            .unwrap();
        assert!(!source.same_source_request(&other_source));
        // Numeric request/generation counters are slot-local. Match each own
        // accepted source birth instead of inventing global scalar uniqueness.
        assert_eq!(source.request_id, requests[0].request_id());
        assert_eq!(other_source.request_id, requests[1].request_id());
        assert_eq!(
            source.admission.source_generation,
            engine.loaded_source_generations.lock().unwrap()[SOURCE].0
        );
        assert_eq!(
            other_source.admission.source_generation,
            engine.loaded_source_generations.lock().unwrap()[OTHER].0
        );
        assert!(!Arc::ptr_eq(
            &source.publication.epoch,
            &other_source.publication.epoch
        ));
        assert_ne!(
            source.admission.binding.placement.unwrap().content_key(),
            other_source
                .admission
                .binding
                .placement
                .unwrap()
                .content_key()
        );
        let other_pair = engine
            .prepare_stem_pair_at_root(
                &root,
                OTHER,
                &version,
                &wav_reference,
                &other_source,
                false,
                Some(selection["descriptor_reference"].as_str().unwrap()),
            )
            .unwrap();
        assert!(!other_pair.has_components());
        assert!(!Arc::ptr_eq(&pair.pair_reader, &other_pair.pair_reader));
        assert_ne!(
            pair.source_lease.assignment_id(),
            other_pair.source_lease.assignment_id()
        );
        let other_selection = serde_json::from_str::<Value>(other_pair.selection_json()).unwrap();
        assert_eq!(other_selection, selection);
        let other_saved = engine.project_assets.acquire(&root, &wav_path).unwrap();
        other_pair.select();
        let other_selected = engine
            .register_committed_stem_pair_with_producer(&other_pair, &other_source, &producer)
            .unwrap();
        assert_eq!(other_selected.publication_status(), "pending");
        assert_eq!(
            drain_control_messages(
                &mut consumer,
                &mut scheduler,
                0,
                &mut quantization,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut retirement,
            ),
            1
        );
        assert_eq!(other_selected.publication_status(), "accepted");
        assert!(other_selected.is_current());
        assert!(mixer.stems_for_measurement()[OTHER].is_none());

        // Genuine mixer configuration drives the DSP. Source, mode and selection
        // ownership above were acquired by their productive control transactions.
        mixer.set_speed(RATIO);
        mixer.set_pad_eq(SOURCE, -12.0, -3.0, 4.0);
        mixer.set_pad_eq(OTHER, 1.0, -3.0, -9.0);
        let captured_at = transport.output_frame();
        assert_eq!(captured_at, 0);
        let mut drain_start = |mixer: &mut RtMixer| {
            drain_control_messages(
                &mut consumer,
                &mut scheduler,
                captured_at,
                &mut quantization,
                &mut transport,
                mixer,
                &mut feedback,
                &mut retirement,
            )
        };
        let starts = [SOURCE, OTHER].map(|id| {
            let start = engine
                .capture_pad_start(
                    id,
                    LOOP_START as f64 / f64::from(RATE),
                    Some(LOOP_END as f64 / f64::from(RATE)),
                    1.0,
                    false,
                    None,
                    None,
                )
                .unwrap();
            assert_eq!(start.target_frame(), None);
            assert_eq!(drain_start(&mut mixer), 1);
            assert_eq!(start.target_frame(), Some(captured_at));
            assert!(engine.publish_pad_start(&start, None).unwrap());
            assert_eq!(drain_start(&mut mixer), 1);
            assert!(start.accepted(), "reason={:?}", start.reason());
            start
        });
        let execution = starts[0].execution_frame().unwrap();
        assert!(execution > captured_at);
        assert!(
            starts
                .iter()
                .all(|start| start.execution_frame() == Some(execution))
        );
        // Host readiness retains both original ordinary owners at the same frozen
        // output coordinate. Only actual rendering consumes their lead and first PCM.
        wait_until(|| {
            let runtime = mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            starts.iter().all(|start| {
                assert_ne!(start.state(), "rejected", "reason={:?}", start.reason());
                runtime.has_ready_action(start.sequence())
            })
        });
        assert_eq!(transport.output_frame(), captured_at);
        let frames = usize::try_from(execution - captured_at + 1).unwrap();
        let mut output = vec![0.0; frames * 2];
        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            captured_at,
            2,
            &mut transport,
            &mut feedback,
            &mut retirement,
        );
        transport.advance_by_rendered_frames(frames);
        assert!(
            output[..(frames - 1) * 2]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert_eq!(transport.output_frame(), execution + 1);
        let native = starts.each_ref().map(|start| {
            assert_eq!(start.original_frame(), Some(captured_at));
            assert_eq!(start.execution_frame(), Some(execution));
            assert_eq!(start.effective_frame(), Some(execution));
            assert_eq!(start.state(), "applied", "reason={:?}", start.reason());
            let voice = mixer
                .voices
                .iter()
                .find(|voice| voice.is_playing_sample(start.diagnostic_slot().unwrap()))
                .unwrap();
            assert_eq!(
                voice.source_admission.as_ref().unwrap().content_key(),
                Some(start.content_key())
            );
            assert_eq!(voice.stretch.adopted_request_id(), Some(start.sequence()));
            voice.stretch.native_state_address()
        });
        assert!(native.into_iter().all(|address| address != 0));
        assert_ne!(native[0], native[1]);
        // Capture the already playing cohort so ALL's own effect receipt remains
        // current; a stopped receipt cannot certify a subsequent public launch.
        let all = engine
            .request_committed_stem_residency_with_producer(
                &selected,
                "all_stems",
                ENABLED_MASK,
                &producer,
            )
            .unwrap();
        let other_all = engine
            .request_committed_stem_residency_with_producer(
                &other_selected,
                "all_stems",
                OTHER_ENABLED_MASK,
                &producer,
            )
            .unwrap();
        let mut fixture = Self {
            engine,
            producer,
            consumer,
            mixer,
            scheduler,
            transport,
            quantization,
            feedback,
            retirement,
            _worker: worker,
            selected,
            all,
            pair,
            selection,
            source,
            other_selected,
            other_all,
            other_pair,
            other_selection,
            other_source,
            version,
            _saved: saved,
            _other_saved: other_saved,
            _material: material,
            _temp: temp,
        };
        // The original request is a captured proposal. Its queued callback
        // capture must supply the real voice pin before the worker can prepare
        // components and arm this same ticket's actual pending publication.
        assert_eq!(fixture.all.residency_status(), "captured");
        assert_eq!(fixture.all.effective_mode_status(), "waiting");
        assert_eq!(fixture.other_all.residency_status(), "captured");
        assert_eq!(fixture.other_all.effective_mode_status(), "waiting");
        assert_eq!(
            fixture.mixer.stem_demand_for_measurement(SOURCE).0,
            StemMixMode::FullMix
        );
        assert_eq!(
            fixture.mixer.stem_demand_for_measurement(OTHER).0,
            StemMixMode::FullMix
        );
        assert_eq!(fixture.drain(), 2);
        wait_until(|| {
            (fixture.consumer.slots() >= 2
                || fixture.all.error().is_some()
                || fixture.other_all.error().is_some())
                && fixture.producer.try_lock().is_ok()
        });
        assert_eq!(
            fixture.all.residency_status(),
            "pending",
            "{:?}",
            fixture.all.error()
        );
        assert_eq!(fixture.all.effective_mode_status(), "waiting");
        assert_eq!(
            fixture.other_all.residency_status(),
            "pending",
            "{:?}",
            fixture.other_all.error()
        );
        assert_eq!(fixture.other_all.effective_mode_status(), "waiting");
        assert_eq!(
            fixture.mixer.stem_demand_for_measurement(SOURCE).0,
            StemMixMode::FullMix
        );
        assert_eq!(
            fixture.mixer.stem_demand_for_measurement(OTHER).0,
            StemMixMode::FullMix
        );
        assert_eq!(fixture.drain(), 2);
        assert_eq!(
            fixture.all.residency_status(),
            "accepted",
            "{:?}",
            fixture.all.error()
        );
        assert_eq!(fixture.all.effective_mode_status(), "transitioning");
        assert_eq!(
            fixture.other_all.residency_status(),
            "accepted",
            "{:?}",
            fixture.other_all.error()
        );
        assert_eq!(fixture.other_all.effective_mode_status(), "transitioning");
        reconcile(&fixture.engine).unwrap();
        // Source-specific Native preparation correctly refuses a transitioning
        // read plan. Consume the real source-frame ramp first; elapsed time or a
        // worker-ready flag cannot advance its 128-source-frame ownership.
        for _ in 0..16 {
            if [SOURCE, OTHER].into_iter().all(|id| {
                !fixture
                    .mixer
                    .stem_transition_for_measurement(id)
                    .is_active()
            }) {
                break;
            }
            fixture.render(31);
        }
        assert!([SOURCE, OTHER].into_iter().all(|id| {
            !fixture
                .mixer
                .stem_transition_for_measurement(id)
                .is_active()
        }));
        // This stable callback creates the real current-source preparation.
        fixture.render(13);
        fixture.settle_current_native();
        fixture.assert_current_all(SOURCE);
        fixture.assert_current_other_all(OTHER);
        fixture.assert_productive_wet(SOURCE, &fixture.all);
        fixture.assert_productive_wet(OTHER, &fixture.other_all);
        fixture.assert_control_readers(SOURCE, OTHER);
        assert_ne!(
            fixture.mixer.voices[fixture.source_lane()]
                .stretch
                .native_state_address(),
            fixture.mixer.voices[fixture.lane(OTHER)]
                .stretch
                .native_state_address()
        );
        // Both actual Native processors have emitted their own selected masked
        // signal before B's independent public pause. H must never acquire B.
        fixture.engine.pause_sample(OTHER).unwrap();
        assert_eq!(fixture.drain(), 1);
        fixture.assert_current_all(SOURCE);
        fixture.assert_current_other_all(OTHER);
        fixture
    }

    fn drain(&mut self) -> usize {
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        let frame = self.transport.output_frame();
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            2,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        // Production stream ownership advances this clock after rendering;
        // render_scheduled_audio itself only walks the callback's segments.
        self.transport.advance_by_rendered_frames(frames);
        output
    }

    fn source_lane(&self) -> usize {
        self.lane(SOURCE)
    }

    fn lane(&self, slot: usize) -> usize {
        self.mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(slot))
            .unwrap()
    }

    fn settle_current_native(&mut self) {
        for _ in 0..4 {
            let lanes = [self.source_lane(), self.lane(OTHER)];
            wait_until(|| {
                lanes
                    .into_iter()
                    .all(|lane| self.mixer.voices[lane].stretch.source_preparation_ready())
            });
            let targets = lanes.map(|lane| {
                self.mixer.voices[lane]
                    .stretch
                    .prepared_target_output_frame()
                    .unwrap()
            });
            // The equally paced actual loops clear their source-frame ramps at
            // the same callback, while each worker owns a distinct Native state.
            assert_eq!(targets[0], targets[1]);
            let target = targets[0];
            let prepared =
                lanes.map(|lane| self.mixer.voices[lane].stretch.prepared_native_address());
            assert!(prepared.into_iter().all(|native| native != 0));
            assert_ne!(prepared[0], prepared[1]);
            assert!(target >= self.transport.output_frame());
            while self.transport.output_frame() < target {
                self.render(257.min((target - self.transport.output_frame()) as usize));
            }
            let mut output = self.render(137);
            let block = self.mixer.voices[lanes[0]].stretch.rubberband_block_size();
            for _ in 0..8 {
                output.extend(self.render(block));
            }
            if lanes.into_iter().zip(prepared).all(|(lane, native)| {
                self.mixer.voices[lane].stretch.native_state_address() == native
                    && self.mixer.voices[lane]
                        .stretch
                        .wet_current_source_output_ready()
            }) && self.all.effective_mode_status() == "effective"
                && self.other_all.effective_mode_status() == "effective"
            {
                assert!(output.iter().any(|sample| sample.abs() > 0.001));
                return;
            }
            self.render(13);
        }
        panic!(
            "actual selected source Native adoptions did not become effective: {:?}, {:?}",
            self.all.error(),
            self.other_all.error()
        );
    }

    fn assert_current_all(&self, slot: usize) {
        self.assert_content_all(
            slot,
            &self.source,
            (&self.selected, &self.all),
            (&self.pair, &self.selection),
            ENABLED_MASK,
        );
    }

    fn assert_current_other_all(&self, slot: usize) {
        self.assert_content_all(
            slot,
            &self.other_source,
            (&self.other_selected, &self.other_all),
            (&self.other_pair, &self.other_selection),
            OTHER_ENABLED_MASK,
        );
    }

    fn assert_content_all(
        &self,
        slot: usize,
        source: &PreparedSourceTicket,
        receipts: (&CommittedStemSetTicket, &ResidentStemTicket),
        saved: (&PreparedStemPair, &Value),
        mask: u8,
    ) {
        let (selected, all) = receipts;
        let (pair, selection) = saved;
        // The immutable original snapshot names a surviving accepted effect,
        // while its fixed-slot admission is fenced by Pair reservation/placement.
        // Use the callback's real retained-source kernel, not a new-admission bit.
        let ownership = &self.engine.input_runtime_ownership;
        assert!(Arc::ptr_eq(&source.admission.ownership, ownership));
        let binding = source.admission.binding;
        let key = binding.placement.unwrap().content_key().unwrap();
        let placement = match ownership.observe_content_key(key) {
            PlacementObservation::Current(placement) => placement,
            other => panic!("owned source content is unavailable: {other:?}"),
        };
        assert_eq!(placement.slot(), slot);
        assert_eq!(placement.content_key(), Some(key));
        assert_eq!(placement.lifetime(), binding.placement.unwrap().lifetime());
        assert_eq!(ownership.callback_key_slot(key), Some(slot));
        assert!(self.mixer.admitted_source_binding_current(slot, binding));
        assert!(source.admission.admitted_current());
        assert_eq!(
            ownership.binding_source_generation(slot, binding),
            Some(source.admission.source_generation)
        );
        assert_eq!(source.publication.content_key(), Some(key));
        assert_eq!(
            source.publication.callback_slot(ownership, source.id),
            Some(slot)
        );
        assert!(Arc::ptr_eq(
            &source.publication.epoch,
            self.engine.prepared_source_epoch(slot).unwrap()
        ));
        assert_eq!(
            source.publication.epoch.load(Ordering::Acquire),
            source.publication.expected
        );
        assert!(source.admission.available());
        // Prepared source and selected receipts use content-bound source/window/
        // timing owners. Their kernels do not request another slot admission.
        assert!(source.publication.current());
        if ownership.pair_reservation(slot) != 0 {
            assert!(!ownership.admission_source_current(source.id, binding));
        } else {
            assert!(
                matches!(ownership.observe_control_key(key), PlacementObservation::Current(current) if current.slot() == slot)
            );
            let fresh = input_runtime_binding::capture(&self.engine, slot)
                .unwrap()
                .unwrap();
            assert!(fresh.current());
            assert!(fresh.available());
            assert_eq!(fresh.binding.placement.unwrap().content_key(), Some(key));
            assert_eq!(fresh.source_generation, source.admission.source_generation);
            assert_eq!(fresh.binding.resident, binding.resident);
            assert_eq!(fresh.binding.accepted, binding.accepted);
        }
        assert_eq!(selected.publication_status(), "accepted");
        assert!(selected.is_current());
        assert_eq!(all.residency_status(), "accepted");
        assert!(all.is_current(), "{:?}", all.error());
        // A caused pause fences source-preparation epochs. Accepted component/
        // mask ownership survives; paused ownership is no new wet-output proof.
        assert_ne!(all.effective_mode_status(), "rejected");
        assert_eq!(all.effective_failure_reason(), None);
        assert_eq!(all.error(), None);
        assert_eq!(
            &serde_json::from_str::<Value>(pair.selection_json()).unwrap(),
            selection
        );
        assert_eq!(
            self.mixer.stem_demand_for_measurement(slot),
            (
                StemMixMode::AllStems,
                mask,
                source_version_hash(&self.version)
            )
        );
        let stems = self.mixer.stems_for_measurement()[slot].as_ref().unwrap();
        assert_eq!(*stems.complete_set_identity, *pair._identity);
        assert_eq!(stems.available_mask, 0b1111);
        assert!(
            stems
                .stems
                .iter()
                .all(|stem| stem.resident_start() == LOOP_START && stem.resident_end() == LOOP_END)
        );
        assert!(
            stems
                .stems
                .iter()
                .all(|stem| stem.samples.iter().any(|sample| sample.abs() > 0.04))
        );
        let voice = self
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(slot))
            .unwrap();
        let sample = voice.sample.as_ref().unwrap();
        assert_eq!(
            ownership.callback_source_generation(slot, sample, RATE),
            Some(source.admission.source_generation)
        );
        assert!(sample.same_source(&source.sample));
        assert!(sample.same_window(&source.sample));
        assert!(Arc::ptr_eq(&sample.samples, &source.sample.samples));
        assert_eq!(voice.source_timing.accepted, binding.accepted);
        assert_eq!(
            voice.source_admission.as_ref().unwrap().content_key(),
            source.admission.binding.placement.unwrap().content_key()
        );
        assert!(self.mixer.key_lock_for_measurement(slot));
        assert!((voice.source_playback.tempo_ratio() - RATIO).abs() < 1e-12);
        assert_ne!(voice.stretch.native_state_address(), 0);
        assert!(
            voice
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames
                > 0
        );
        assert!(
            self.mixer
                .dsp_source_history_for_test(slot)
                .unwrap()
                .fed_output_frames
                > 0
        );
    }

    fn assert_productive_wet(&self, slot: usize, all: &ResidentStemTicket) {
        let voice = &self.mixer.voices[self.lane(slot)];
        assert!(!voice.paused);
        assert_eq!(all.effective_mode_status(), "effective");
        assert!(voice.stretch.wet_render_ready());
        assert!(voice.stretch.wet_output_ready());
        assert!(voice.stretch.wet_current_source_output_ready());
        assert!(
            voice
                .stretch
                .output_buffers()
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 0.001)
        );
    }

    fn assert_control_readers(&self, source_slot: usize, other_slot: usize) {
        let owners = self.engine.resident_stem_cache.lock().unwrap();
        for (slot, prepared) in [(source_slot, &self.pair), (other_slot, &self.other_pair)] {
            let selected = owners[slot].accepted.as_ref().unwrap();
            let pair = selected.pair.as_ref().unwrap();
            assert!(Arc::ptr_eq(&pair.reader, &prepared.pair_reader));
            assert_eq!(pair.descriptor_reference, prepared.descriptor_reference);
            assert_eq!(
                pair.source_lease.assignment_id(),
                prepared.source_lease.assignment_id()
            );
        }
        assert!(!Arc::ptr_eq(
            &self.pair.pair_reader,
            &self.other_pair.pair_reader
        ));
    }

    fn hold(&self) -> HoldActionTicket {
        Python::attach(|py| {
            self.engine
                .pause_waveform_hold(SOURCE, PyInt::new(py, 1701).as_any(), None)
                .unwrap()
        })
    }

    fn prepare_pair(&self, target: usize) -> PadPairTicket {
        let captured = self.engine._capture_pad_pair(SOURCE, target).unwrap();
        control::prepare(&self.engine, &captured, RATE, 2).unwrap()
    }

    fn finish(&self, ticket: &PadPairTicket) {
        let ack = ticket.own_acknowledgement().unwrap();
        let receipt = ticket.native_acknowledgement().unwrap();
        assert!(self.engine._reconcile_pad_pair(ticket).unwrap());
        assert!(
            self.engine
                ._validate_pad_pair_acknowledgement(ticket, &ack)
                .unwrap()
        );
        self.engine._finish_pad_pair(ticket, &ack).unwrap();
        for slot in [receipt.source_after.slot, receipt.target_after.slot] {
            assert_eq!(
                self.engine.input_runtime_ownership.pair_reservation(slot),
                0
            );
        }
    }
}

fn assert_selected_owners_retained(actual: &PreparedStemSet, before: &PreparedStemSet) {
    assert!(Arc::ptr_eq(
        &actual.complete_set_identity,
        &before.complete_set_identity
    ));
    assert!(Arc::ptr_eq(
        &actual.reference_samples,
        &before.reference_samples
    ));
    match (&actual.reference_residency, &before.reference_residency) {
        (Some(actual), Some(before)) => {
            assert!(Arc::ptr_eq(&actual.source, &before.source));
            assert_eq!(actual.start_frame, before.start_frame);
            assert_eq!(actual.window_revision, before.window_revision);
            assert_eq!(actual.context, before.context);
        }
        (None, None) => {}
        _ => panic!("selected reference source owner was replaced"),
    }
    assert_eq!(actual.source_version_hash, before.source_version_hash);
    assert_eq!(actual.accepted_timing, before.accepted_timing);
    for (current, prior) in actual.stems.iter().zip(before.stems.iter()) {
        assert!(current.same_window(prior));
        assert!(Arc::ptr_eq(&current.samples, &prior.samples));
    }
}

fn assert_selected_all_wet_composition(target: usize) {
    for release_first in [false, true] {
        let mut actual = SelectedWetFixture::new();
        let mut chronology = SelectedWetFixture::new();
        assert_eq!(
            actual.transport.output_frame(),
            chronology.transport.output_frame()
        );
        assert_eq!(actual.render(31), chronology.render(31));
        actual.assert_current_all(SOURCE);
        actual.assert_productive_wet(SOURCE, &actual.all);
        actual.assert_current_other_all(OTHER);
        actual.assert_control_readers(SOURCE, OTHER);
        let hold = actual.hold();
        let reference_hold = chronology.hold();
        assert_eq!(actual.drain(), 1);
        assert_eq!(chronology.drain(), 1);
        assert_eq!(hold.publication_status(), "paused_owned");
        let source = LaneEvidence::capture(&actual.mixer, SOURCE);
        let other = LaneEvidence::capture(&actual.mixer, OTHER);
        assert_eq!(source.key, hold.action.key());
        assert_ne!(source.key, other.key);
        assert_ne!(source.native, other.native);
        assert!(!Arc::ptr_eq(&source.lifetime, &other.lifetime));
        assert!(!Arc::ptr_eq(&source.epoch, &other.epoch));
        assert!(actual.mixer.voices[source.lane].paused);
        assert!(actual.mixer.voices[other.lane].paused);
        let stems = actual.mixer.stems_for_measurement()[SOURCE]
            .clone()
            .unwrap();
        let other_stems = actual.mixer.stems_for_measurement()[OTHER].clone().unwrap();
        assert_eq!(
            actual
                .engine
                .input_runtime_ownership
                .pair_reservation(SOURCE),
            0
        );
        assert_eq!(
            actual
                .engine
                .input_runtime_ownership
                .pair_reservation(target),
            0
        );
        let ticket = actual.prepare_pair(target);
        assert!(ticket.own_acknowledgement().is_none());
        assert_eq!(ticket.publication_status(), "captured");
        let reservation = actual
            .engine
            .input_runtime_ownership
            .pair_reservation(SOURCE);
        assert_ne!(reservation, 0);
        assert_eq!(
            actual
                .engine
                .input_runtime_ownership
                .pair_reservation(target),
            reservation
        );
        assert_eq!(hold.current_control_slot(), Some(SOURCE));
        assert_eq!(hold.action.state(), HoldActionState::PausedOwned);
        actual.assert_current_all(SOURCE);
        actual.assert_current_other_all(OTHER);
        if release_first {
            assert!(actual.engine.release_waveform_hold(&hold).unwrap());
        }
        actual.engine._publish_pad_pair(&ticket).unwrap();
        if !release_first {
            assert!(actual.engine.release_waveform_hold(&hold).unwrap());
        }
        assert!(
            chronology
                .engine
                .release_waveform_hold(&reference_hold)
                .unwrap()
        );
        assert_eq!(actual.drain(), 2);
        assert_eq!(chronology.drain(), 1);
        assert_eq!(hold.action.state(), HoldActionState::Resumed);
        let effect = hold.action.effect().unwrap();
        assert_eq!(effect.key, source.key);
        assert_eq!(effect.cohort_mask, 1 << source.lane);
        assert_eq!(effect.changed_mask, 1 << source.lane);
        assert_eq!(effect.generations[source.lane], source.generation);
        let receipt = ticket.native_acknowledgement().unwrap();
        assert_eq!(
            receipt.kind,
            if target == EMPTY {
                PairPlacementKind::Move
            } else {
                PairPlacementKind::Swap
            }
        );
        assert_eq!(receipt.target_after.lifetime, source.key.lifetime());
        assert_eq!(
            receipt.source_after.lifetime,
            if target == EMPTY {
                0
            } else {
                other.key.lifetime()
            }
        );
        if release_first {
            assert!(receipt.holds[0].is_none());
        } else {
            let carried = receipt.holds[0].unwrap();
            assert_eq!(carried.action_id, hold.action.action_id());
            assert_eq!(carried.effect, Some(effect));
            assert!(carried.effect_current);
        }
        assert!(receipt.holds[1].is_none());
        source.assert_retained(&actual.mixer, target, 1);
        let other_slot = if target == EMPTY { OTHER } else { SOURCE };
        other.assert_retained(&actual.mixer, other_slot, 0);
        actual.assert_current_other_all(other_slot);
        assert_selected_owners_retained(
            actual.mixer.stems_for_measurement()[other_slot]
                .as_ref()
                .unwrap(),
            &other_stems,
        );
        assert!(!actual.mixer.voices[source.lane].paused);
        assert!(actual.mixer.voices[other.lane].paused);
        assert_selected_owners_retained(
            actual.mixer.stems_for_measurement()[target]
                .as_ref()
                .unwrap(),
            &stems,
        );
        actual.assert_current_all(target);
        actual.finish(&ticket);
        actual.assert_current_all(target);
        actual.assert_current_other_all(other_slot);
        actual.assert_control_readers(target, other_slot);
        assert_eq!(hold.current_control_slot(), None);
        assert_selected_owners_retained(
            actual.mixer.stems_for_measurement()[target]
                .as_ref()
                .unwrap(),
            &stems,
        );
        // The reference owns independent source/selection, Native/FIFO and EQ
        // histories. It experiences the same caused pause/resume without Pair.
        // Successive audible callbacks prove the moved wet state continues.
        let mut audible = Vec::new();
        for frames in [1_usize, 31, 137, 257, 512, 777, 1024, 1, 137] {
            let output = actual.render(frames);
            assert_eq!(output, chronology.render(frames));
            audible.extend(output);
            assert_eq!(
                actual.mixer.voices[source.lane]
                    .stretch
                    .native_state_address(),
                source.native
            );
            other.assert_retained(&actual.mixer, other_slot, 0);
            actual.assert_current_other_all(other_slot);
            actual.assert_control_readers(target, other_slot);
            assert_selected_owners_retained(
                actual.mixer.stems_for_measurement()[other_slot]
                    .as_ref()
                    .unwrap(),
                &other_stems,
            );
            assert!(actual.mixer.voices[other.lane].paused);
        }
        assert!(audible.iter().any(|sample| sample.abs() > 0.001));
        assert!(
            actual.mixer.voices[source.lane]
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames
                > source.history.unwrap().fed_output_frames
        );
        // Pause/resume fences pending work while the adopted owner's own permit
        // still authorizes this unchanged source/selection. It must not require
        // an unsolicited replacement. The genuine ALL change already crossed its
        // exact preparation/adoption boundary in settle_current_native above.
        // Continue beyond a full preparation lead with the same productive owner.
        let reference_lane = chronology.source_lane();
        let reference_native = chronology.mixer.voices[reference_lane]
            .stretch
            .native_state_address();
        let adopted = actual.mixer.voices[source.lane]
            .stretch
            .adopted_request_id();
        let reference_adopted = chronology.mixer.voices[reference_lane]
            .stretch
            .adopted_request_id();
        assert!(adopted.is_some());
        assert!(reference_adopted.is_some());
        let mut remaining =
            crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES + 137;
        while remaining != 0 {
            let frames = 257.min(remaining);
            assert_eq!(actual.render(frames), chronology.render(frames));
            remaining -= frames;
            let voice = &actual.mixer.voices[source.lane];
            let reference_voice = &chronology.mixer.voices[reference_lane];
            assert_eq!(voice.generation, source.generation);
            assert_eq!(voice.stretch.native_state_address(), source.native);
            assert_eq!(voice.stretch.adopted_request_id(), adopted);
            assert_eq!(
                reference_voice.stretch.native_state_address(),
                reference_native
            );
            assert_eq!(
                reference_voice.stretch.adopted_request_id(),
                reference_adopted
            );
            assert!(
                voice
                    .source_playback
                    .matches_exact(&reference_voice.source_playback)
            );
            assert_eq!(
                voice.stretch.pending_fifo_frames(),
                reference_voice.stretch.pending_fifo_frames()
            );
            let history = voice.stretch.productive_history().unwrap();
            let reference_history = reference_voice.stretch.productive_history().unwrap();
            assert_eq!(history.binding, source.history.unwrap().binding);
            assert_eq!(history.next_position, reference_history.next_position);
            assert_eq!(history.domain, reference_history.domain);
            assert_eq!(
                history.fed_output_frames,
                reference_history.fed_output_frames
            );
            let dsp = actual.mixer.dsp_source_history_for_test(target).unwrap();
            let reference_dsp = chronology
                .mixer
                .dsp_source_history_for_test(SOURCE)
                .unwrap();
            assert_eq!(dsp.binding, source.dsp.binding);
            assert_eq!(dsp.next_position, reference_dsp.next_position);
            assert_eq!(dsp.fed_output_frames, reference_dsp.fed_output_frames);
            actual.assert_current_all(target);
            actual.assert_productive_wet(target, &actual.all);
            chronology.assert_productive_wet(SOURCE, &chronology.all);
            other.assert_retained(&actual.mixer, other_slot, 0);
            actual.assert_current_other_all(other_slot);
            actual.assert_control_readers(target, other_slot);
            assert_selected_owners_retained(
                actual.mixer.stems_for_measurement()[other_slot]
                    .as_ref()
                    .unwrap(),
                &other_stems,
            );
        }
        actual.assert_current_all(target);
        actual.assert_productive_wet(target, &actual.all);
        other.assert_retained(&actual.mixer, other_slot, 0);
        actual.assert_current_other_all(other_slot);
        actual.assert_control_readers(target, other_slot);
        assert_selected_owners_retained(
            actual.mixer.stems_for_measurement()[other_slot]
                .as_ref()
                .unwrap(),
            &other_stems,
        );
        let terminal = hold.action.projection();
        for _ in 0..2 {
            assert!(actual.engine.release_waveform_hold(&hold).unwrap());
        }
        assert_eq!(actual.drain(), 0);
        assert_eq!(hold.action.projection(), terminal);
        assert_eq!(ticket.native_acknowledgement(), Some(receipt));
    }
}

#[test]
fn actual_selected_all_wet_move_hold_pair_release_preserves_native_chronology() {
    assert_selected_all_wet_composition(EMPTY);
}

#[test]
fn actual_selected_all_wet_swap_hold_pair_release_preserves_paused_b_and_native_chronology() {
    assert_selected_all_wet_composition(OTHER);
}
