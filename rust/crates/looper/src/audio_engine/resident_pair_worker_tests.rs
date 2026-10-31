//! Real queued workers retain their original key across own Pair ACK and control custody.
#[path = "committed_stem_placement_tests.rs"]
mod placement_admission_tests;

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::input_mapping::launch_admission_tests::dispatch;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::audio_engine::{pad_pair, resident_relocation};
use crate::messages::{AudioMessage, TriggerQuantization};
use rtrb::{Consumer, RingBuffer};
use std::sync::Condvar;

struct BlockedWorkers(Arc<(Mutex<bool>, Condvar)>);
impl BlockedWorkers {
    fn hold(engine: &AudioEngine) -> Self {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (entered, receive) = std::sync::mpsc::channel();
        for _ in 0..super::super::super::cold_jobs::WORKERS {
            let gate = gate.clone();
            let entered = entered.clone();
            engine
                .cold_jobs
                .submit(engine.cold_jobs.reserve().unwrap(), move || {
                    entered.send(()).unwrap();
                    let mut released = gate.0.lock().unwrap();
                    while !*released {
                        released = gate.1.wait(released).unwrap();
                    }
                })
                .unwrap();
        }
        for _ in 0..super::super::super::cold_jobs::WORKERS {
            receive.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        Self(gate)
    }
}
impl Drop for BlockedWorkers {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "bounded worker did not reach its checkpoint"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct PairWorkerFixture {
    source: PublicTicketFixture,
    mixer: Box<RtMixer>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: Consumer<ControlMessage>,
    selection: CommittedStemSetTicket,
}
impl PairWorkerFixture {
    fn new() -> Self {
        let mut source = PublicTicketFixture::new();
        let engine = &mut source.fixture.engine;
        let ownership = engine.input_runtime_ownership.clone();
        let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        let (producer, mut consumer) = RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback))));
        let mut mixer = Box::new(RtMixer::new(2, 48_000.0));
        mixer.set_input_runtime_ownership(ownership);
        mixer.set_current_timing_acknowledgements(engine.current_timing_acknowledgements.clone());
        mixer.set_prepared_source_epochs(engine.prepared_source_epochs.clone());
        mixer.load_sample(0, sample);
        // Use actual cold admission, file-owner registration and its own Native ACK.
        // The sealed disk pair supplies the original; no synthetic reader owner is installed.
        let request = crate::audio_engine::cold_load::admit_for_format_selected(
            engine,
            0,
            source
                .owner
                .pair
                .source_lease
                .original_path
                .to_string_lossy()
                .into(),
            (
                false,
                false,
                true,
                Some(crate::audio_engine::cold_residency::ResidentLoadHint {
                    start_s: 0.0,
                    end_s: 4096.0 / 48_000.0,
                    key_lock: false,
                }),
            ),
            producer.clone(),
            (2, 48_000, source.owner.pair.root.clone()),
        )
        .unwrap();
        wait_until(|| !consumer.is_empty());
        dispatch(
            &mut mixer,
            consumer.pop().unwrap(),
            &mut FixedCapacityScheduler::<8>::new(),
            &mut TransportTimeline::new(48_000),
            &mut TriggerQuantization::Immediate,
            &mut Vec::new(),
        );
        assert_eq!(
            engine.input_runtime_ownership.cold_status(0, request),
            Some(2)
        );
        wait_until(|| engine.cold_loading[0].load(Ordering::Acquire) == 0);
        let binding = input_runtime_binding::capture(engine, 0).unwrap().unwrap();
        let lease = engine.cold_leases.lock().unwrap()[0].clone().unwrap();
        let sample = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
        assert!(sample.residency.is_some());
        assert_eq!(sample.frame_count(), 4096);
        assert_eq!(sample.resident_start(), 0);
        assert_eq!(sample.resident_end(), 4096);
        lease.verify_reference(&sample).unwrap();
        assert_eq!(
            engine
                .project_assets
                .cold_lease_for_reader(&sample)
                .unwrap()
                .assignment_id(),
            lease.assignment_id()
        );
        source.fixture.placement = binding.binding.placement.unwrap();
        source.owner = Arc::new(SelectionOwner {
            id: 0,
            binding: binding.binding,
            source_generation: binding.source_generation,
            source_version: source.owner.source_version.clone(),
            identity: source.owner.identity.clone(),
            pair: StemPairOwner {
                source_lease: lease,
                ..source.owner.pair.clone()
            },
            generation_path: source.owner.generation_path.clone(),
            epoch: source.owner.epoch.clone(),
            expected: source.owner.expected,
            revoked: source.owner.revoked.clone(),
            source_epoch: engine.prepared_source_epoch(0).unwrap().clone(),
            ownership: engine.input_runtime_ownership.clone(),
        });
        source.fixture.binding = binding;
        // Actual Native registration seals this same immutable selected disk owner.
        let publication = PreparedSourcePermit::new(Arc::new(AtomicU64::new(1)), 1);
        publication.mark_pending().unwrap();
        mixer.apply_committed_stem_work(
            Box::new(CommittedStemRtWork::Register {
                id: 0,
                binding: source.owner.binding,
                selection: source.owner.rt_permit(),
                publication: publication.clone(),
            }),
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(publication.status(), "accepted");
        engine.resident_stem_cache.lock().unwrap()[0]
            .committed
            .accepted = Some(source.owner.clone());
        let selection = CommittedStemSetTicket {
            owner: source.owner.clone(),
            publication,
            candidate: None,
        };
        Self {
            source,
            mixer,
            producer,
            consumer,
            selection,
        }
    }
    fn dispatch(&mut self, message: ControlMessage) {
        dispatch(
            &mut self.mixer,
            message,
            &mut FixedCapacityScheduler::<8>::new(),
            &mut TransportTimeline::new(48_000),
            &mut TriggerQuantization::Immediate,
            &mut Vec::new(),
        );
    }
    fn attach_runtime_for_swap(&mut self) {
        let request = crate::audio_engine::cold_load::admit_for_format_selected(
            &self.source.fixture.engine,
            1,
            self.source
                .owner
                .pair
                .source_lease
                .original_path
                .to_string_lossy()
                .into(),
            (
                false,
                false,
                true,
                Some(crate::audio_engine::cold_residency::ResidentLoadHint {
                    start_s: 0.0,
                    end_s: 4096.0 / 48_000.0,
                    key_lock: false,
                }),
            ),
            self.producer.clone(),
            (2, 48_000, self.source.owner.pair.root.clone()),
        )
        .unwrap();
        wait_until(|| !self.consumer.is_empty());
        let message = self.consumer.pop().unwrap();
        self.dispatch(message);
        assert_eq!(
            self.source
                .fixture
                .engine
                .input_runtime_ownership
                .cold_status(1, request),
            Some(2)
        );
        wait_until(|| self.source.fixture.engine.cold_loading[1].load(Ordering::Acquire) == 0);
        let engine = &mut self.source.fixture.engine;
        let bindings =
            [0, 1].map(|id| input_runtime_binding::capture(engine, id).unwrap().unwrap());
        let runtime = crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            self.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            engine.input_runtime_ownership.clone(),
        );
        let mut loaded = vec![false; super::super::super::constants::NUM_SAMPLES];
        let mut rows = vec![None; super::super::super::constants::NUM_SAMPLES];
        for id in [0, 1] {
            loaded[id] = true;
            rows[id] = Some(&bindings[id]);
        }
        runtime
            .set_runtime_state(
                true,
                loaded,
                vec![0.0; super::super::super::constants::NUM_SAMPLES],
                vec![Some(4096.0 / 48_000.0); super::super::super::constants::NUM_SAMPLES],
                rows,
            )
            .unwrap();
        engine.input_runtime = Some(runtime);
    }
    fn pair_ack(&mut self) -> pad_pair::PadPairTicket {
        let engine = &self.source.fixture.engine;
        let captured = pad_pair::capture(engine, 0, 1).unwrap();
        let ticket = pad_pair::prepare(engine, &captured, 48_000, 2).unwrap();
        pad_pair::publish(engine, &ticket).unwrap();
        let message = self.consumer.pop().unwrap();
        self.dispatch(message);
        assert!(ticket.native_acknowledgement().is_some());
        ticket
    }
    fn replace_equal_pcm_lifetime(&self) {
        let engine = &self.source.fixture.engine;
        let ownership = &engine.input_runtime_ownership;
        let current = ownership.slot_snapshot(1).unwrap();
        let sample = engine.sample_cache.lock().unwrap()[1].clone().unwrap();
        let write = ownership
            .prepare_slot_write_fixed(
                current,
                super::super::super::content_lifetime::reserve().unwrap(),
            )
            .unwrap();
        ownership
            .publish_source_write(
                &write,
                &sample,
                48_000,
                self.source.fixture.binding.source_generation,
                self.source.owner.binding.authority_revision,
                Some(self.source.fixture.binding.copy_timing_context().0),
                false,
                || true,
            )
            .unwrap();
        assert!(sample.same_source(engine.sample_cache.lock().unwrap()[1].as_ref().unwrap()));
    }
}

#[test]
fn actual_blocked_window_worker_own_pair_mirror_keeps_original_key_and_rejects_equal_pcm_lifetime()
{
    for replaced in [false, true] {
        let mut fixture = PairWorkerFixture::new();
        let blockers = BlockedWorkers::hold(&fixture.source.fixture.engine);
        let ticket = resident_relocation::prepare_window_with_producer(
            &fixture.source.fixture.engine,
            0,
            resident_relocation::WindowRequest {
                loop_region: Some((512.0 / 48_000.0, Some(1024.0 / 48_000.0))),
                ..resident_relocation::WindowRequest::default()
            },
            fixture.producer.clone(),
        )
        .unwrap();
        let pair = fixture.pair_ack();
        drop(blockers);
        wait_until(|| ticket.control_wait_observed_for_test());
        assert!(ticket.is_current());
        assert_eq!(ticket.publication_status(), "preparing");
        assert!(fixture.consumer.is_empty());
        assert!(pad_pair::reconcile(&fixture.source.fixture.engine, &pair).unwrap());
        // A new physical origin request cannot revoke the moved original Source epoch.
        fixture
            .source
            .fixture
            .engine
            .pad_request_ids
            .lock()
            .unwrap()[0] += 1;
        wait_until(|| ticket.control_published_for_test());
        wait_until(|| !fixture.consumer.is_empty());
        let message = fixture.consumer.pop().unwrap();
        let ControlMessage::RelocateResident(transaction) = &message else {
            panic!("real worker relocation required");
        };
        assert_eq!(
            transaction.binding.placement.unwrap().content_key(),
            fixture.source.fixture.placement.content_key()
        );
        assert_eq!(transaction.sample.resident_start(), 512);
        assert_eq!(transaction.sample.resident_end(), 1024);
        if replaced {
            fixture.replace_equal_pcm_lifetime();
        }
        fixture.dispatch(message);
        assert_eq!(
            ticket.publication_status(),
            if replaced { "rejected" } else { "accepted" }
        );
        assert_eq!(ticket.is_current(), !replaced);
        resident_relocation::reconcile(&fixture.source.fixture.engine).unwrap();
        assert!(fixture.source.fixture.engine.sample_cache.lock().unwrap()[0].is_none());
        if !replaced {
            let cache = fixture.source.fixture.engine.sample_cache.lock().unwrap();
            assert_eq!(cache[1].as_ref().unwrap().resident_start(), 512);
            assert_eq!(cache[1].as_ref().unwrap().resident_end(), 1024);
        }
    }
}

#[test]
fn actual_blocked_committed_worker_own_pair_mirror_keeps_pin_and_rejects_equal_pcm_lifetime() {
    for replaced in [false, true] {
        let mut fixture = PairWorkerFixture::new();
        let blockers = BlockedWorkers::hold(&fixture.source.fixture.engine);
        let ticket = fixture
            .source
            .fixture
            .engine
            .request_committed_stem_residency_with_interest_and_producer(
                &fixture.selection,
                "all_stems",
                STEM_COMPONENT_MASK,
                "explicit",
                &fixture.producer,
            )
            .unwrap();
        let capture = fixture.consumer.pop().unwrap();
        fixture.dispatch(capture);
        let pair = fixture.pair_ack();
        drop(blockers);
        wait_until(|| ticket.status.control_wait_observed.load(Ordering::Acquire));
        assert!(ticket.is_current());
        assert_eq!(ticket.residency_status(), "captured");
        assert!(fixture.consumer.is_empty());
        assert!(pad_pair::reconcile(&fixture.source.fixture.engine, &pair).unwrap());
        fixture
            .source
            .fixture
            .engine
            .pad_request_ids
            .lock()
            .unwrap()[0] += 1;
        wait_until(|| ticket.status.control_published.load(Ordering::Acquire));
        wait_until(|| !fixture.consumer.is_empty());
        let message = fixture.consumer.pop().unwrap();
        let ControlMessage::CommittedStemWork(work) = &message else {
            panic!("real committed worker required");
        };
        let CommittedStemRtWork::Adopt(transaction) = work.as_ref() else {
            panic!("real prepared adoption required");
        };
        assert_eq!(
            transaction.binding.placement.unwrap().content_key(),
            fixture.source.fixture.placement.content_key()
        );
        assert!(
            transaction.pin.sample.same_source(
                fixture.source.fixture.engine.sample_cache.lock().unwrap()[1]
                    .as_ref()
                    .unwrap()
            )
        );
        assert!(Arc::ptr_eq(&transaction.status, &ticket.status));
        if replaced {
            fixture.replace_equal_pcm_lifetime();
        }
        fixture.dispatch(message);
        assert_eq!(
            ticket.residency_status(),
            if replaced { "rejected" } else { "accepted" }
        );
        assert_eq!(ticket.is_current(), !replaced);
        resident_relocation::reconcile(&fixture.source.fixture.engine).unwrap();
        assert!(fixture.source.fixture.engine.sample_cache.lock().unwrap()[0].is_none());
    }
}

#[test]
fn actual_component_read_own_pair_pending_move_and_equal_pcm_lifetime_are_revalidated() {
    for replaced_during_read in [false, true] {
        let mut fixture = PairWorkerFixture::new();
        let hold = crate::audio_engine::stem_pair::ComponentReadHold::new(
            &fixture.source.owner.pair.reader,
        );
        let ticket = fixture
            .source
            .fixture
            .engine
            .request_committed_stem_residency_with_interest_and_producer(
                &fixture.selection,
                "all_stems",
                STEM_COMPONENT_MASK,
                "explicit",
                &fixture.producer,
            )
            .unwrap();
        let capture = fixture.consumer.pop().unwrap();
        fixture.dispatch(capture);
        hold.wait();
        let pair = fixture.pair_ack();
        assert!(
            fixture
                .source
                .fixture
                .engine
                .resident_stem_cache
                .try_lock()
                .is_ok()
        );
        if replaced_during_read {
            assert!(pad_pair::reconcile(&fixture.source.fixture.engine, &pair).unwrap());
            fixture.replace_equal_pcm_lifetime();
            hold.release();
            wait_until(|| fixture.source.fixture.engine.cold_jobs.counts_for_test().0 == 0);
            assert_eq!(ticket.residency_status(), "rejected");
            assert!(fixture.consumer.is_empty());
        } else {
            hold.release();
            wait_until(|| ticket.status.control_wait_observed.load(Ordering::Acquire));
            assert_eq!(ticket.residency_status(), "captured");
            assert!(ticket.is_current());
            assert!(fixture.consumer.is_empty());
            assert!(pad_pair::reconcile(&fixture.source.fixture.engine, &pair).unwrap());
            wait_until(|| ticket.status.control_published.load(Ordering::Acquire));
            wait_until(|| !fixture.consumer.is_empty());
            let message = fixture.consumer.pop().unwrap();
            let ControlMessage::CommittedStemWork(work) = &message else {
                panic!("own moved adoption required")
            };
            let CommittedStemRtWork::Adopt(transaction) = work.as_ref() else {
                panic!("prepared adoption required")
            };
            assert_eq!(
                transaction.binding.placement.unwrap().content_key(),
                fixture.source.fixture.placement.content_key()
            );
            assert!(Arc::ptr_eq(&transaction.status, &ticket.status));
            assert!(
                transaction.pin.sample.same_source(
                    fixture.source.fixture.engine.sample_cache.lock().unwrap()[1]
                        .as_ref()
                        .unwrap(),
                )
            );
            fixture.dispatch(message);
            assert_eq!(ticket.residency_status(), "accepted");
            assert!(ticket.is_current());
        }
        resident_relocation::reconcile(&fixture.source.fixture.engine).unwrap();
        assert!(fixture.source.fixture.engine.sample_cache.lock().unwrap()[0].is_none());
        let settled = fixture
            .source
            .fixture
            .engine
            .project_assets
            .resident_budget_status(&fixture.source.fixture.engine.resident_account)
            .unwrap();
        assert_eq!(
            (
                settled.pending_resident_bytes,
                settled.pending_reservation_count
            ),
            (0, 0)
        );
    }
}

#[test]
fn actual_window_ack_after_pair_ack_catches_up_original_custody_before_control_mirror() {
    let mut fixture = PairWorkerFixture::new();
    fixture.attach_runtime_for_swap();
    let engine = &fixture.source.fixture.engine;
    let ticket = resident_relocation::prepare_window_with_producer(
        engine,
        0,
        resident_relocation::WindowRequest {
            loop_region: Some((512.0 / 48_000.0, Some(1024.0 / 48_000.0))),
            ..resident_relocation::WindowRequest::default()
        },
        fixture.producer.clone(),
    )
    .unwrap();
    wait_until(|| ticket.control_published_for_test());
    wait_until(|| !fixture.consumer.is_empty());
    let retained_window = fixture.consumer.pop().unwrap();
    // Hold the already-admitted owning callback payload while the actual Pair executes.
    let pair = fixture.pair_ack();
    fixture.dispatch(retained_window);
    assert_eq!(ticket.publication_status(), "accepted");
    let engine = &fixture.source.fixture.engine;
    let original_key = fixture.source.fixture.placement.content_key().unwrap();
    assert!(matches!(
        engine
            .input_runtime_ownership
            .observe_control_key(original_key),
        PlacementObservation::Pending
    ));
    assert_eq!(
        engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .resident_start(),
        0
    );
    resident_relocation::reconcile(engine).unwrap();
    assert_eq!(
        engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .resident_start(),
        512
    );
    assert!(matches!(
        engine
            .input_runtime_ownership
            .observe_control_key(original_key),
        PlacementObservation::Pending
    ));
    assert!(pad_pair::reconcile(engine, &pair).unwrap());
    resident_relocation::reconcile(engine).unwrap();
    assert!(ticket.is_current());
    let cache = engine.sample_cache.lock().unwrap();
    assert_eq!(cache[0].as_ref().unwrap().resident_start(), 0);
    assert_eq!(cache[1].as_ref().unwrap().resident_start(), 512);
    assert_eq!(cache[1].as_ref().unwrap().resident_end(), 1024);
    let midi = engine
        .input_runtime
        .as_ref()
        .unwrap()
        .pair_binding_for_test(1)
        .unwrap();
    assert_eq!(midi.placement.unwrap().content_key(), Some(original_key));
    assert_eq!(midi.resident, cache[1].as_ref().unwrap().resident_binding());
    assert!(engine.input_runtime_ownership.current(1, midi));
}

#[test]
fn actual_pair_gap_preparation_clock_excludes_pending_and_preserves_original_deadline() {
    let mut fixture = PairWorkerFixture::new();
    let pair = fixture.pair_ack();
    let engine = &fixture.source.fixture.engine;
    let key = fixture.source.fixture.placement.content_key().unwrap();
    let clock = resident_relocation::ResidentPreparationClock::default();
    let admitted = Instant::now();
    let gap_started = admitted + Duration::from_secs(10);
    let resumed = gap_started + Duration::from_secs(3600);
    let pending = matches!(
        engine.input_runtime_ownership.observe_control_key(key),
        PlacementObservation::Pending
    );
    assert!(pending);
    assert!(
        !clock
            .expired_at(admitted, pending, DEADLINE, gap_started)
            .unwrap()
    );
    assert!(
        !clock
            .expired_at(admitted, pending, DEADLINE, resumed)
            .unwrap()
    );
    assert!(pad_pair::reconcile(engine, &pair).unwrap());
    let pending = matches!(
        engine.input_runtime_ownership.observe_control_key(key),
        PlacementObservation::Pending
    );
    assert!(!pending);
    assert!(
        !clock
            .expired_at(admitted, pending, DEADLINE, resumed)
            .unwrap()
    );
    assert!(
        !clock
            .expired_at(
                admitted,
                pending,
                DEADLINE,
                resumed + Duration::from_secs(19)
            )
            .unwrap()
    );
    assert!(
        clock
            .expired_at(
                admitted,
                pending,
                DEADLINE,
                resumed + Duration::from_secs(20)
            )
            .unwrap()
    );
}
