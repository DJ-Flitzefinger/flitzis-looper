use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::input_runtime_binding::InputRuntimePadBinding;
use crate::messages::SampleBuffer;
use std::cell::Cell;
use std::sync::{Arc, Barrier};

const SLOT: usize = 3;
const REQUEST: u64 = 7;
const EPOCH: u64 = 9;
const RATE: u32 = 48_000;
const CURRENT_ERROR: &str = "retained current analysis diagnostic";

#[test]
fn dequeued_native_delivery_retains_original_owner_through_actual_move_and_swap() {
    use crate::audio_engine::audio_stream::process_control_message;
    use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
    use crate::audio_engine::complete_context::{CompleteSourceReader, NativeAnalysisOwner};
    use crate::audio_engine::input_mapping::launch_admission_tests::LaunchFixture;
    use crate::audio_engine::pad_pair;
    use crate::audio_engine::scheduler::FixedCapacityScheduler;
    use crate::audio_engine::transport::TransportTimeline;
    use crate::messages::{AudioMessage, TriggerQuantization};
    use rtrb::RingBuffer;
    use std::sync::Mutex;

    for target in [1, 215] {
        let mut fixture = LaunchFixture::new(4);
        let reference = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let reader = CompleteSourceReader::capture(&fixture.engine, 0, reference).unwrap();
        let owner = NativeAnalysisOwner::new(
            reader.guard.clone().unwrap(),
            fixture.engine.prepared_source_epoch(0).unwrap().clone(),
        );
        let request = fixture.engine.pad_request_ids.lock().unwrap()[0];
        fixture
            .engine
            .loader_tx
            .send(owner.event(LoaderEvent::TaskSuccess {
                id: 0,
                request_id: request,
                task: BackgroundTaskKind::Analysis,
                analysis: None,
            }))
            .unwrap();
        Python::attach(|py| {
            let event = fixture.engine.poll_loader_events(py).unwrap().unwrap();
            let dict = event.bind(py).cast::<PyDict>().unwrap();
            let retained: Py<LoaderEventDelivery> = dict
                .get_item("_native_event_owner")
                .unwrap()
                .unwrap()
                .extract()
                .unwrap();
            assert_eq!(
                resolve_delivery(&fixture.engine, &retained.borrow(py)).unwrap(),
                Some((0, false))
            );
            assert!(retained.borrow(py).same_owner(&LoaderEventDelivery {
                owner: owner.clone(),
                timing_epoch: None,
                source_beatmap: None,
                source_beatmap_artifact: None,
                source_beatmap_raw: None,
            }));
            let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
            fixture.engine.test_control_channels =
                Some((fixture.producer.clone(), Arc::new(Mutex::new(feedback))));
            let mut mixer = fixture.mixer();
            mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
            let captured = pad_pair::capture(&fixture.engine, 0, target).unwrap();
            let ticket = pad_pair::prepare(&fixture.engine, &captured, RATE, 1).unwrap();
            pad_pair::publish(&fixture.engine, &ticket).unwrap();
            process_control_message(
                fixture.consumer.pop().unwrap(),
                &mut FixedCapacityScheduler::<8>::new(),
                0,
                &mut TriggerQuantization::Immediate,
                &mut TransportTimeline::new(RATE),
                &mut mixer,
                &mut Vec::<AudioMessage>::new(),
                &mut ImmediateAudioBufferRetirement,
            );
            assert!(ticket.native_acknowledgement().is_some());
            assert!(
                resolve_delivery(&fixture.engine, &retained.borrow(py)).is_err(),
                "own ACK waits for actual control mirror"
            );
            assert!(pad_pair::reconcile(&fixture.engine, &ticket).unwrap());
            assert_eq!(
                resolve_delivery(&fixture.engine, &retained.borrow(py)).unwrap(),
                Some((target, false))
            );
            assert_eq!(
                dict.get_item("id")
                    .unwrap()
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                0,
                "old numeric projection is diagnostic, retained Native owner grants delivery"
            );
            let foreign = AudioEngine::new().unwrap();
            assert_eq!(
                resolve_delivery(&foreign, &retained.borrow(py)).unwrap(),
                None
            );
            crate::audio_engine::next_pad_request_id(
                &fixture.engine.pad_request_ids,
                target,
                fixture.engine.prepared_source_epoch(target).unwrap(),
            )
            .unwrap();
            assert_eq!(
                resolve_delivery(&fixture.engine, &retained.borrow(py)).unwrap(),
                None,
                "a superseded actual original task never authorizes the moved occupant"
            );
        });
    }
}

#[test]
fn retained_source_delivery_rechecks_timing_without_borrowing_new_analysis() {
    let fixture = Fixture::new();
    let owner = crate::audio_engine::complete_context::NativeAnalysisOwner::from_publication(
        &fixture.publication,
        &fixture.sample,
        RATE,
    );
    let delivery = LoaderEventDelivery {
        owner,
        timing_epoch: Some(EPOCH),
        source_beatmap: None,
        source_beatmap_artifact: None,
        source_beatmap_raw: None,
    };
    assert_eq!(
        resolve_delivery(&fixture.engine, &delivery).unwrap(),
        Some((SLOT, false))
    );
    crate::audio_engine::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        SLOT,
        fixture.engine.prepared_source_epoch(SLOT).unwrap(),
    )
    .unwrap();
    assert_eq!(
        resolve_delivery(&fixture.engine, &delivery).unwrap(),
        Some((SLOT, true))
    );
}

struct Fixture {
    engine: AudioEngine,
    sample: SampleBuffer,
    key: ContentKey,
    binding: InputRuntimePadBinding,
    publication: crate::audio_engine::input_runtime_binding::SourcePublication,
}

impl Fixture {
    fn new() -> Self {
        Python::initialize();
        let engine = AudioEngine::new().unwrap();
        let sample = SampleBuffer {
            samples: Arc::from([0.25_f32, -0.5, 0.75, -0.125].as_slice()),
            channels: 1,
            residency: None,
        }
        .with_complete_source(RATE);
        let ownership = &engine.input_runtime_ownership;
        let empty = ownership.slot_snapshot(SLOT).unwrap();
        let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
        let write = ownership.prepare_slot_write(empty, lifetime).unwrap();
        let authority = ownership.next_authority(SLOT).unwrap();
        let publication = ownership
            .publish_source_write(
                &write,
                &sample,
                RATE,
                REQUEST,
                authority,
                None,
                false,
                || true,
            )
            .unwrap();
        let key = publication.key().unwrap();
        assert_ne!(key.lifetime(), 0);
        assert!(publication.current(&sample, RATE));
        engine.pad_request_ids.lock().unwrap()[SLOT] = REQUEST;
        engine.sample_cache.lock().unwrap()[SLOT] = Some(sample.clone());
        engine.loaded_source_generations.lock().unwrap()[SLOT] = (REQUEST, RATE);
        engine.loaded_source_digests.lock().unwrap()[SLOT] = Some("a".repeat(64));
        engine
            .prepared_source_epoch(SLOT)
            .unwrap()
            .store(EPOCH, Ordering::Release);
        let binding = engine
            .current_input_runtime_pad_binding(SLOT)
            .unwrap()
            .unwrap();
        assert!(binding.is_current());
        Self {
            engine,
            sample,
            key,
            binding,
            publication,
        }
    }

    fn send_success(&self, epoch: u64, error: &str) {
        self.engine
            .loader_tx
            .send(LoaderEvent::Success {
                original_lease: None,
                timing_epoch: Some(epoch),
                id: SLOT,
                request_id: REQUEST,
                duration_s: 4.0 / f64::from(RATE),
                detected_loop_start_s: Some(1.0 / f64::from(RATE)),
                cached_path: "samples/queued-current.wav".into(),
                analysis: None,
                analysis_error: Some(error.into()),
                analysis_tempo_only: false,
                incoming_defaults: None,
            })
            .unwrap();
    }

    fn send_progress(&self, request_id: u64, stage: &str) {
        self.engine
            .loader_tx
            .send(LoaderEvent::Progress {
                id: SLOT,
                request_id,
                percent: 0.5,
                stage: stage.into(),
            })
            .unwrap();
    }

    fn assert_source_unchanged(&self) {
        assert_eq!(self.engine.pad_request_ids.lock().unwrap()[SLOT], REQUEST);
        assert_eq!(
            self.engine
                .prepared_source_epoch(SLOT)
                .unwrap()
                .load(Ordering::Acquire),
            EPOCH
        );
        assert_eq!(
            self.engine.loaded_source_generations.lock().unwrap()[SLOT],
            (REQUEST, RATE)
        );
        assert!(
            self.engine
                .input_runtime_ownership
                .content_key_current(self.key)
        );
        assert_eq!(
            self.engine
                .input_runtime_ownership
                .slot_snapshot(SLOT)
                .unwrap()
                .content_key(),
            Some(self.key)
        );
        assert!(self.binding.is_current());
        assert!(Arc::ptr_eq(
            &self.engine.sample_cache.lock().unwrap()[SLOT]
                .as_ref()
                .unwrap()
                .samples,
            &self.sample.samples,
        ));
    }
}

#[test]
fn original_native_source_event_survives_timing_advance_without_stale_analysis() {
    let fixture = Fixture::new();
    let owner = crate::audio_engine::complete_context::NativeAnalysisOwner::from_publication(
        &fixture.publication,
        &fixture.sample,
        RATE,
    );
    fixture
        .engine
        .loader_tx
        .send(owner.event(LoaderEvent::Success {
            id: SLOT,
            request_id: REQUEST,
            timing_epoch: Some(EPOCH),
            duration_s: 4.0 / f64::from(RATE),
            detected_loop_start_s: Some(1.0 / f64::from(RATE)),
            cached_path: "samples/queued-current.wav".into(),
            original_lease: None,
            analysis: Some(crate::messages::LoadedAnalysis {
                bpm: 119.75,
                key: None,
                beat_grid: flitzis_looper_analysis::BeatGrid {
                    beats: vec![0.0],
                    downbeats: vec![],
                    bars: vec![],
                },
                bpm_method_json: Some("original optional metadata, must be stripped".into()),
                beat_error: None,
            }),
            analysis_error: Some("original timing diagnostic".into()),
            analysis_tempo_only: true,
            incoming_defaults: None,
        }))
        .unwrap();
    crate::audio_engine::next_pad_request_id(
        &fixture.engine.pad_request_ids,
        SLOT,
        fixture.engine.prepared_source_epoch(SLOT).unwrap(),
    )
    .unwrap();
    assert!(owner.current());
    Python::attach(|py| {
        let event = fixture.engine.poll_loader_events(py).unwrap().unwrap();
        let dict = event.bind(py).cast::<PyDict>().unwrap();
        assert_eq!(
            dict.get_item("analysis_kind")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "tempo_only"
        );
        assert!(dict.get_item("analysis").unwrap().is_none());
        assert_success(py, event, None, true);
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
    });
}

fn assert_success(py: Python<'_>, event: Py<PyAny>, expected_error: Option<&str>, stale: bool) {
    let dict = event.bind(py).cast::<PyDict>().unwrap();
    assert_eq!(
        dict.get_item("type")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap(),
        "success"
    );
    assert_eq!(
        dict.get_item("id")
            .unwrap()
            .unwrap()
            .extract::<usize>()
            .unwrap(),
        SLOT
    );
    assert_eq!(
        dict.get_item("request_id")
            .unwrap()
            .unwrap()
            .extract::<u64>()
            .unwrap(),
        REQUEST
    );
    assert_eq!(
        dict.get_item("cached_path")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap(),
        "samples/queued-current.wav"
    );
    assert_eq!(
        dict.get_item("duration_s")
            .unwrap()
            .unwrap()
            .extract::<f64>()
            .unwrap(),
        4.0 / f64::from(RATE)
    );
    assert_eq!(
        dict.get_item("detected_loop_start_s")
            .unwrap()
            .unwrap()
            .extract::<f64>()
            .unwrap(),
        1.0 / f64::from(RATE)
    );
    let error = dict
        .get_item("analysis_error")
        .unwrap()
        .map(|value| value.extract::<String>().unwrap());
    assert_eq!(error.as_deref(), expected_error);
    assert_eq!(dict.get_item("timing_stale").unwrap().is_some(), stale);
}

fn assert_progress(fixture: &Fixture, py: Python<'_>, stage: &str) {
    let event = fixture.engine.poll_loader_events(py).unwrap().unwrap();
    let dict = event.bind(py).cast::<PyDict>().unwrap();
    assert_eq!(
        dict.get_item("type")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap(),
        "progress"
    );
    assert_eq!(
        dict.get_item("request_id")
            .unwrap()
            .unwrap()
            .extract::<u64>()
            .unwrap(),
        REQUEST
    );
    assert_eq!(
        dict.get_item("stage")
            .unwrap()
            .unwrap()
            .extract::<String>()
            .unwrap(),
        stage
    );
    assert!(dict.get_item("timing_stale").unwrap().is_none());
}

#[test]
fn real_dequeued_loader_event_survives_source_write_pending_then_replays_once_in_order() {
    let fixture = Fixture::new();
    fixture.send_success(EPOCH, CURRENT_ERROR);
    fixture.send_progress(REQUEST, "second queued event");
    fixture.send_progress(REQUEST - 1, "obsolete request must remain filtered");
    fixture.send_success(EPOCH - 1, "obsolete timing diagnostic must remain filtered");
    fixture.send_progress(REQUEST, "last queued event");
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let before = ownership.slot_snapshot(SLOT).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(before, before.lifetime())
        .unwrap();
    let after_dequeue = Barrier::new(2);
    let writer_is_odd = Barrier::new(2);
    let release_writer = Barrier::new(2);

    Python::attach(|py| {
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                after_dequeue.wait();
                ownership.commit_slot_write_fixed(
                    &write,
                    || true,
                    |_| {
                        writer_is_odd.wait();
                        release_writer.wait();
                    },
                )
            });
            let popped = Cell::new(false);
            let held = poll(&fixture.engine, py, || {
                popped.set(true);
                after_dequeue.wait();
                writer_is_odd.wait();
            });
            let retained_from_actual_pop = matches!(
                fixture.engine.pending_loader_event.lock().unwrap().as_ref(),
                Some(LoaderEvent::Success {
                    id: SLOT, request_id: REQUEST, timing_epoch: Some(EPOCH), analysis_error: Some(error), ..
                }) if error == CURRENT_ERROR
            );
            let held_again = fixture.engine.poll_loader_events(py);
            release_writer.wait();
            let completed = writer.join().unwrap().unwrap();
            assert!(popped.get());
            assert!(held.unwrap().is_none());
            assert!(held_again.unwrap().is_none());
            assert!(retained_from_actual_pop);
            assert_eq!(completed.content_key(), before.content_key());
            assert_eq!(completed.binding_epoch(), before.binding_epoch());
            assert_eq!(completed.layout_revision(), before.layout_revision() + 2);
        });

        fixture.assert_source_unchanged();
        let replay = poll(&fixture.engine, py, || {
            panic!("retained loader event must replay before another channel dequeue");
        })
        .unwrap()
        .unwrap();
        assert_success(py, replay, Some(CURRENT_ERROR), false);
        assert!(
            fixture
                .engine
                .pending_loader_event
                .lock()
                .unwrap()
                .is_none()
        );
        assert_progress(&fixture, py, "second queued event");
        let stale = fixture.engine.poll_loader_events(py).unwrap().unwrap();
        assert_success(py, stale, None, true);
        assert_progress(&fixture, py, "last queued event");
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
        fixture.assert_source_unchanged();
    });
}

#[test]
fn source_write_pending_before_poll_keeps_real_channel_events_without_dequeue() {
    let fixture = Fixture::new();
    fixture.send_success(EPOCH, CURRENT_ERROR);
    fixture.send_progress(REQUEST, "next queued event");
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let before = ownership.slot_snapshot(SLOT).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(before, before.lifetime())
        .unwrap();
    let writer_is_odd = Barrier::new(2);
    let release_writer = Barrier::new(2);
    Python::attach(|py| {
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                ownership.commit_slot_write_fixed(
                    &write,
                    || true,
                    |_| {
                        writer_is_odd.wait();
                        release_writer.wait();
                    },
                )
            });
            writer_is_odd.wait();
            let held = poll(&fixture.engine, py, || {
                panic!("an already pending source writer must prevent channel dequeue");
            });
            let held_again = fixture.engine.poll_loader_events(py);
            let no_dequeued_event = fixture
                .engine
                .pending_loader_event
                .lock()
                .unwrap()
                .is_none();
            release_writer.wait();
            let completed = writer.join().unwrap().unwrap();
            assert!(held.unwrap().is_none());
            assert!(held_again.unwrap().is_none());
            assert!(no_dequeued_event);
            assert_eq!(completed.content_key(), before.content_key());
            assert_eq!(completed.binding_epoch(), before.binding_epoch());
        });
        fixture.assert_source_unchanged();
        let first = fixture.engine.poll_loader_events(py).unwrap().unwrap();
        assert_success(py, first, Some(CURRENT_ERROR), false);
        assert_progress(&fixture, py, "next queued event");
        assert!(fixture.engine.poll_loader_events(py).unwrap().is_none());
        fixture.assert_source_unchanged();
    });
}
