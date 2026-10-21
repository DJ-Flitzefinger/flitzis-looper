//! Productive immutable readers across the actual internal Pair ACK and control mirror.

use super::*;
use crate::audio_engine::audio_stream::process_control_message;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::pad_pair::{self as pair, PadPairTicket};
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, TriggerQuantization};
use rtrb::RingBuffer;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

pub(in crate::audio_engine) fn own_pair_ack(
    fixture: &mut LaunchFixture,
    mixer: &mut RtMixer,
) -> PadPairTicket {
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    fixture.engine.test_control_channels =
        Some((fixture.producer.clone(), Arc::new(Mutex::new(feedback))));
    let captured = pair::capture(&fixture.engine, 0, 1).unwrap();
    let ticket = pair::prepare(&fixture.engine, &captured, 48_000, 1).unwrap();
    pair::publish(&fixture.engine, &ticket).unwrap();
    let message = fixture.consumer.pop().unwrap();
    process_control_message(
        message,
        &mut FixedCapacityScheduler::<8>::new(),
        0,
        &mut TriggerQuantization::Immediate,
        &mut TransportTimeline::new(48_000),
        mixer,
        &mut Vec::<AudioMessage>::new(),
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(ticket.native_acknowledgement().is_some());
    ticket
}

#[test]
fn actual_blocked_complete_reader_follows_own_pair_without_fresh_source_permission() {
    let mut fixture = LaunchFixture::new(4);
    let reference = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let reader = CompleteSourceReader::capture(&fixture.engine, 0, reference.clone()).unwrap();
    let guard = reader.guard.clone().unwrap();
    let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
    let (started, entered) = mpsc::channel();
    let (release, resumed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut pcm = Vec::new();
        reader
            .visit_region(0..128, &|| false, |_, chunk| {
                started.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(5)).unwrap();
                pcm.extend_from_slice(chunk);
                Ok(())
            })
            .map(|()| pcm)
    });
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut mixer = fixture.mixer();
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let ticket = own_pair_ack(&mut fixture, &mut mixer);
    assert!(guard.current());
    assert!(matches!(
        guard.observe_control(),
        PlacementObservation::Pending
    ));
    assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
    assert!(
        matches!(guard.observe_control(), PlacementObservation::Current(snapshot) if snapshot.slot() == 1)
    );
    assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
    assert!(CompleteSourceReader::capture(&fixture.engine, 0, reference).is_err());
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap().unwrap(), vec![0.25; 128]);
}

#[test]
fn complete_source_guard_rejects_native_revocation_with_equal_old_metadata_and_shared_pcm() {
    let fixture = LaunchFixture::new(4);
    let reference = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let reader = CompleteSourceReader::capture(&fixture.engine, 0, reference.clone()).unwrap();
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(1, &reference, 48_000, 7);
    fixture.engine.sample_cache.lock().unwrap()[1] = Some(reference);
    fixture.engine.loaded_source_generations.lock().unwrap()[1] = (7, 48_000);
    let guard = reader.guard.unwrap();
    assert!(guard.current());
    fixture.engine.input_runtime_ownership.revoke_source(0);
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[0],
        (7, 48_000)
    );
    assert!(!guard.current());
}

#[cfg(windows)]
mod cold {
    use super::*;
    use crate::audio_engine::cold_residency::ResidentLoadHint;
    use crate::audio_engine::content_placement::SlotSnapshot;
    use crate::messages::ControlMessage;
    use rtrb::{Consumer, Producer};
    use std::fs;
    use std::time::Instant;

    /// Same productive admission/ACK lane as cold_offline_tests::ColdOffline.
    /// Both immutable assignments are issued by the worker, never assembled here.
    struct ColdCustody {
        engine: AudioEngine,
        directory: tempfile::TempDir,
        producer: Arc<Mutex<Producer<ControlMessage>>>,
        consumer: Consumer<ControlMessage>,
        // Match the production stream's heap-owned mixer. Returning multiple
        // inline mixers through fixture branches overflows the default test stack.
        mixer: Box<RtMixer>,
    }

    impl ColdCustody {
        fn new() -> Self {
            Self::new_selected(Some(ResidentLoadHint {
                start_s: 32.0 / 48_000.0,
                end_s: 64.0 / 48_000.0,
                key_lock: false,
            }))
        }

        fn new_selected(hint: Option<ResidentLoadHint>) -> Self {
            Self::new_selected_with_pcm(hint, [8192; 128])
        }

        fn new_selected_with_pcm(hint: Option<ResidentLoadHint>, pcm: [i16; 128]) -> Self {
            Self::new_selected_with_signal(hint, &pcm)
        }

        fn new_selected_with_signal(hint: Option<ResidentLoadHint>, pcm: &[i16]) -> Self {
            Self::new_selected_with_interleaved_signal(hint, pcm, 1)
        }

        fn new_selected_with_interleaved_signal(
            hint: Option<ResidentLoadHint>,
            pcm: &[i16],
            channels: u16,
        ) -> Self {
            assert!(matches!(channels, 1 | 2));
            assert!(!pcm.is_empty() && pcm.len().is_multiple_of(usize::from(channels)));
            let original_frames = pcm.len() / usize::from(channels);
            pyo3::Python::initialize();
            let directory = tempfile::tempdir().unwrap();
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            let pcm_bytes = u32::try_from(pcm.len().checked_mul(2).unwrap()).unwrap();
            bytes.extend_from_slice(&(36 + pcm_bytes).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            bytes.extend_from_slice(&channels.to_le_bytes());
            bytes.extend_from_slice(&48_000_u32.to_le_bytes());
            bytes.extend_from_slice(&(96_000_u32 * u32::from(channels)).to_le_bytes());
            bytes.extend_from_slice(&(2_u16 * channels).to_le_bytes());
            bytes.extend_from_slice(&16_u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&pcm_bytes.to_le_bytes());
            for sample in pcm {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            fs::write(directory.path().join("external.wav"), bytes).unwrap();
            let mut engine = AudioEngine::new().unwrap();
            assert!(engine.stream_handle.is_none());
            let (producer, consumer) = RingBuffer::new(8);
            let producer = Arc::new(Mutex::new(producer));
            let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
            engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
            let mut mixer = Box::new(RtMixer::new(2, 48_000.0));
            mixer.set_input_runtime_ownership(engine.input_runtime_ownership.clone());
            mixer.set_current_timing_acknowledgements(
                engine.current_timing_acknowledgements.clone(),
            );
            mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
            let mut fixture = Self {
                engine,
                directory,
                producer,
                consumer,
                mixer,
            };
            fixture.load_and_ack_selected_with_frames(0, hint, original_frames);
            fixture.load_and_ack_selected_with_frames(1, hint, original_frames);
            fixture
        }

        fn load_and_ack_selected(&mut self, id: usize, hint: Option<ResidentLoadHint>) {
            self.load_and_ack_selected_with_frames(id, hint, 128);
        }

        fn load_and_ack_selected_with_frames(
            &mut self,
            id: usize,
            hint: Option<ResidentLoadHint>,
            original_frames: usize,
        ) {
            let request = crate::audio_engine::cold_load::admit_for_format_selected(
                &self.engine,
                id,
                self.directory
                    .path()
                    .join("external.wav")
                    .to_string_lossy()
                    .into(),
                (false, false, true, hint),
                self.producer.clone(),
                (2, 48_000, self.directory.path().join("samples")),
            )
            .unwrap();
            wait_until(|| {
                self.engine.input_runtime_ownership.cold_status(id, request) == Some(0)
                    && self.consumer.slots() == 1
            });
            self.dispatch_one();
            assert_eq!(
                self.engine.input_runtime_ownership.cold_status(id, request),
                Some(2)
            );
            wait_until(|| self.engine.cold_loading[id].load(Ordering::Acquire) == 0);
            let sample = self.engine.sample_cache.lock().unwrap()[id]
                .clone()
                .unwrap();
            let (start, end) = hint.map_or((0, original_frames), |hint| {
                (
                    (hint.start_s * 48_000.0).floor() as usize,
                    (hint.end_s * 48_000.0).ceil() as usize,
                )
            });
            assert_eq!(sample.frame_count(), original_frames);
            assert_eq!(sample.resident_start(), start);
            assert_eq!(sample.resident_end(), end);
            assert_eq!(sample.samples.len(), (end - start) * 2);
            assert_eq!(
                self.engine.loaded_source_generations.lock().unwrap()[id],
                (request, 48_000)
            );
            assert!(self.engine.cold_leases.lock().unwrap()[id].is_some());
            assert!(
                self.engine
                    .input_runtime_ownership
                    .source_current(id, &sample, 48_000)
            );
        }

        fn dispatch_one(&mut self) {
            process_control_message(
                self.consumer.pop().unwrap(),
                &mut FixedCapacityScheduler::<8>::new(),
                0,
                &mut TriggerQuantization::Immediate,
                &mut TransportTimeline::new(48_000),
                &mut self.mixer,
                &mut Vec::<AudioMessage>::new(),
                &mut ImmediateAudioBufferRetirement,
            );
        }
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "cold custody worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn actual_cold_complete_reader_and_offline_source_follow_own_pair_ack_and_mirror() {
        let mut fixture = ColdCustody::new();
        let samples = fixture.engine.sample_cache.lock().unwrap().clone();
        let original = samples[0].clone().unwrap();
        let other = samples[1].as_ref().unwrap();
        assert!(Arc::ptr_eq(&original.samples, &other.samples));
        assert!(original.same_window(other));
        let generations = fixture
            .engine
            .loaded_source_generations
            .lock()
            .unwrap()
            .clone();
        let digests = fixture.engine.loaded_source_digests.lock().unwrap().clone();
        assert_eq!(generations[0], generations[1]);
        assert_eq!(digests[0], digests[1]);
        let requests = fixture.engine.pad_request_ids.lock().unwrap().clone();
        let placements: [SlotSnapshot; 2] = [0, 1].map(|id| {
            fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(id)
                .unwrap()
        });
        assert_ne!(placements[0].content_key(), placements[1].content_key());
        let leases = fixture.engine.cold_leases.lock().unwrap().clone();
        let lease = leases[0].as_ref().unwrap();
        let other_lease = leases[1].as_ref().unwrap();
        assert_ne!(lease.assignment_id(), other_lease.assignment_id());
        assert_eq!(lease.original_path, other_lease.original_path);
        assert_eq!(lease.samples_root(), other_lease.samples_root());
        assert_eq!(lease.cache_assignment_count(), 2);
        let relative = lease
            .original_path
            .strip_prefix(lease.samples_root())
            .unwrap();
        let version = format!(
            "samples/{}|sha256-v1:{}",
            relative.to_string_lossy().replace('\\', "/"),
            digests[0].as_ref().unwrap()
        );
        let source = fixture
            .engine
            .capture_offline_stem_source(0, version.clone())
            .unwrap();
        let source_path = source.source_path();
        assert_eq!(source.source_generation(), generations[0].0);
        assert_eq!(source.source_request(), requests[0]);
        assert_eq!(source.target_shape(), (48_000, 2, 128));
        let reader = CompleteSourceReader::capture(&fixture.engine, 0, original.clone()).unwrap();
        assert_eq!(
            reader.lease.as_ref().unwrap().assignment_id(),
            lease.assignment_id()
        );
        let guard = reader.guard.clone().unwrap();
        let original_binding = guard.binding;
        assert_eq!(guard.key(), placements[0].content_key());
        let (started, entered) = mpsc::channel();
        let (release, resumed) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut pcm = Vec::new();
            reader
                .visit_region(0..128, &|| false, |first, chunk| {
                    assert_eq!(first, 0);
                    started.send(()).unwrap();
                    resumed.recv_timeout(Duration::from_secs(5)).unwrap();
                    pcm.extend_from_slice(chunk);
                    Ok(())
                })
                .map(|()| pcm)
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let captured = pair::capture(&fixture.engine, 0, 1).unwrap();
        let ticket = pair::prepare(&fixture.engine, &captured, 48_000, 2).unwrap();
        assert!(!pair::reconcile(&fixture.engine, &ticket).unwrap());
        pair::publish(&fixture.engine, &ticket).unwrap();
        assert!(ticket.native_acknowledgement().is_none());
        fixture.dispatch_one();
        let ack = ticket.native_acknowledgement().unwrap();
        assert_eq!(ack.target_after.cell, placements[0].cell().index());
        assert_eq!(ack.target_after.lifetime, placements[0].lifetime());
        assert!(guard.current());
        assert!(matches!(
            guard.observe_control(),
            PlacementObservation::Pending
        ));
        for id in [0, 1] {
            let error = fixture
                .engine
                .offline_stem_source_is_current(id, &source)
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("offline stem source placement is pending")
            );
            let error = fixture
                .engine
                .capture_prepared_stem_source(id, &source)
                .err()
                .unwrap();
            assert!(
                error
                    .to_string()
                    .contains("offline stem source placement is pending")
            );
        }
        assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
        assert_eq!(source.source_version(), version);
        assert_eq!(source.source_path(), source_path);
        pyo3::Python::attach(|py| source.verify(py).unwrap());
        // Complete immutable reads may finish during the Native/control mirror gap.
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), vec![0.25; 256]);
        assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
        assert!(pair::reconcile(&fixture.engine, &ticket).unwrap());
        assert!(
            matches!(guard.observe_control(), PlacementObservation::Current(snapshot)
            if snapshot.slot() == 1 && snapshot.content_key() == placements[0].content_key())
        );
        assert!(guard.current());
        assert_eq!(guard.binding.placement, original_binding.placement);
        assert_eq!(guard.generation, generations[0].0);
        assert_eq!(guard.rate, 48_000);
        assert!(
            fixture
                .engine
                .offline_stem_source_is_current(1, &source)
                .unwrap()
        );
        assert!(
            !fixture
                .engine
                .offline_stem_source_is_current(0, &source)
                .unwrap()
        );
        assert!(
            fixture
                .engine
                .capture_prepared_stem_source(0, &source)
                .is_err()
        );
        let cache = fixture.engine.sample_cache.lock().unwrap();
        assert!(cache[1].as_ref().unwrap().same_window(&original));
        assert!(Arc::ptr_eq(
            &cache[0].as_ref().unwrap().samples,
            &original.samples
        ));
        drop(cache);
        assert_eq!(*fixture.engine.pad_request_ids.lock().unwrap(), requests);
        assert_eq!(
            *fixture.engine.loaded_source_generations.lock().unwrap(),
            generations
        );
        assert_eq!(
            *fixture.engine.loaded_source_digests.lock().unwrap(),
            digests
        );
        let current_leases = fixture.engine.cold_leases.lock().unwrap();
        let moved = current_leases[1].as_ref().unwrap();
        assert_eq!(moved.assignment_id(), lease.assignment_id());
        assert_eq!(moved.original_path, lease.original_path);
        assert_eq!(moved.samples_root(), lease.samples_root());
        assert_eq!(
            current_leases[0].as_ref().unwrap().assignment_id(),
            other_lease.assignment_id()
        );
        assert_eq!(moved.cache_assignment_count(), 2);
        drop(current_leases);
        assert_eq!(source.source_path(), source_path);
        assert_eq!(source.source_generation(), generations[0].0);
        assert_eq!(source.source_version(), version);
        pyo3::Python::attach(|py| source.verify(py).unwrap());
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(&lease.original_path)
                .is_err()
        );
    }

    mod source_beatmap_custody_tests {
        include!("source_beatmap_custody_tests.rs");
    }
}
