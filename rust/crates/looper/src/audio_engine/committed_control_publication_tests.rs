//! Captured committed control arming and deferred external failure settlement.
use super::super::content_placement::PlacementError;
use super::*;
#[path = "resident_pair_worker_tests.rs"]
mod resident_pair_worker_tests;
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::cell::{Cell, RefCell};
use std::sync::mpsc;

thread_local! {
    static BEFORE_COMMIT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn before_commit() {
    if let Some(hook) = BEFORE_COMMIT.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
}

fn with_before_commit<T>(hook: impl FnOnce() + 'static, action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_COMMIT.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BEFORE_COMMIT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    let _reset = Reset;
    action()
}

struct PendingWriter {
    release: Option<mpsc::Sender<()>>,
    writer: Option<std::thread::JoinHandle<Result<SlotSnapshot, PlacementError>>>,
}
impl PendingWriter {
    fn hold(ownership: Arc<InputRuntimeOwnership>) -> Self {
        let other = ownership.slot_snapshot(215).unwrap();
        let write = ownership
            .prepare_slot_write_fixed(other, other.lifetime())
            .unwrap();
        let (entered, observed) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            ownership.commit_slot_write_fixed(
                &write,
                || true,
                |_| {
                    entered.send(()).unwrap();
                    resume.recv().unwrap();
                },
            )
        });
        let held = Self {
            release: Some(release),
            writer: Some(writer),
        };
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        held
    }
    fn complete(mut self) {
        self.release.take().unwrap().send(()).unwrap();
        self.writer.take().unwrap().join().unwrap().unwrap();
    }
}
impl Drop for PendingWriter {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

struct Fixture {
    engine: AudioEngine,
    binding: InputRuntimePadBinding,
    epoch: Arc<AtomicU64>,
    placement: SlotSnapshot,
}
impl Fixture {
    fn new() -> Self {
        let sample = SampleBuffer {
            samples: Arc::from([0.25_f32; 64]),
            channels: 1,
            residency: None,
        }
        .with_complete_source(48_000);
        Self::with_sample(sample)
    }
    fn with_sample(sample: SampleBuffer) -> Self {
        pyo3::Python::initialize();
        let engine = AudioEngine::new().unwrap();
        let ownership = &engine.input_runtime_ownership;
        let captured = ownership.slot_snapshot(0).unwrap();
        let lifetime = super::super::content_lifetime::reserve().unwrap();
        let write = ownership.prepare_slot_write(captured, lifetime).unwrap();
        ownership
            .publish_source_write(
                &write,
                &sample,
                48_000,
                7,
                ownership.next_authority(0).unwrap(),
                Some(TimingIntent::Manual),
                false,
                || true,
            )
            .unwrap();
        engine.sample_cache.lock().unwrap()[0] = Some(sample);
        engine.pad_request_ids.lock().unwrap()[0] = 7;
        engine.loaded_source_generations.lock().unwrap()[0] = (7, 48_000);
        engine.loaded_source_digests.lock().unwrap()[0] = Some(format!("{:064x}", 7));
        engine.timing_intents.lock().unwrap()[0] = TimingIntent::Manual;
        engine
            .prepared_source_epoch(0)
            .unwrap()
            .store(7, Ordering::Release);
        let binding = input_runtime_binding::capture(&engine, 0).unwrap().unwrap();
        let placement = binding.binding.placement.unwrap();
        ownership
            .resident_control_for_cell(placement.cell())
            .store(6, Ordering::Release);
        ownership
            .key_lock_request_for_cell(placement.cell())
            .store(4, Ordering::Release);
        Self {
            engine,
            binding,
            placement,
            epoch: Arc::new(AtomicU64::new(9)),
        }
    }
    fn control(&self) -> CommittedControlWrite {
        CommittedControlWrite::capture(&self.binding, 9, Some(10)).unwrap()
    }
    fn counters(&self) -> (u64, u64, u64) {
        let ownership = &self.engine.input_runtime_ownership;
        (
            self.epoch.load(Ordering::Acquire),
            ownership
                .resident_control_for_cell(self.placement.cell())
                .load(Ordering::Acquire),
            ownership
                .key_lock_request_for_cell(self.placement.cell())
                .load(Ordering::Acquire),
        )
    }
    fn status(&self, published: bool) -> Arc<ResidencyStatus> {
        let publication = PreparedSourcePermit::new(Arc::new(AtomicU64::new(6)), 6);
        publication.mark_pending().unwrap();
        let proposal = self.engine.resident_proposals.begin_stems(0).unwrap();
        proposal.arm().unwrap();
        Arc::new(ResidencyStatus {
            publication,
            effective: AtomicU8::new(0),
            effective_failure: AtomicBool::new(false),
            error: Mutex::new(None),
            proposal: Some(proposal),
            policy_epoch: None,
            ownership: self.engine.input_runtime_ownership.clone(),
            id: 0,
            binding: self.binding.binding,
            resident_control: true,
            control_published: AtomicBool::new(published),
            preparation_clock: super::super::resident_relocation::ResidentPreparationClock::default(
            ),
            control_wait_observed: AtomicBool::new(false),
            failure_pending: AtomicBool::new(false),
            failure_settled: AtomicBool::new(false),
            failed_settlement: Mutex::new(false),
        })
    }
}

/// Public receipts retain a genuine sealed pair owner, without starting a resident worker.
struct PublicTicketFixture {
    owner: Arc<SelectionOwner>,
    fixture: Fixture,
    _directory: tempfile::TempDir,
}
impl PublicTicketFixture {
    fn new() -> Self {
        use sha2::{Digest, Sha256};
        use std::fs;
        fn wav(scale: i16) -> Vec<u8> {
            let mut bytes = Vec::new();
            let frames = 4096_u32;
            let size = frames * 4;
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + size).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            bytes.extend_from_slice(&48_000_u32.to_le_bytes());
            bytes.extend_from_slice(&192_000_u32.to_le_bytes());
            bytes.extend_from_slice(&4_u16.to_le_bytes());
            bytes.extend_from_slice(&16_u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&size.to_le_bytes());
            for frame in 0..frames {
                let value = (1000.0 * (f64::from(frame) * 0.031).sin()).round() as i16 * scale;
                for _ in 0..2 {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            bytes
        }
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let directory = tempfile::Builder::new()
            .prefix("committed-control-projection-")
            .tempdir_in(repo.parent().unwrap().join("scratch"))
            .unwrap();
        let root = directory.path().join("samples");
        fs::create_dir(&root).unwrap();
        let input = root.join("projection.wav");
        fs::write(&input, wav(4)).unwrap();
        let material =
            super::super::material_migration::prepare_material(&root, &input, 48_000, 2, &|| false)
                .unwrap();
        let metadata = material.metadata();
        let source_version = format!(
            "{}|sha256-v1:{}",
            metadata["new_reference"].as_str().unwrap(),
            metadata["original"]["sha256"].as_str().unwrap()
        );
        let material_root = super::super::material_paths::canonical_original_root_reference(
            std::path::Path::new(metadata["new_reference"].as_str().unwrap()),
            metadata["material_id"].as_str().unwrap(),
        )
        .unwrap();
        let wav_reference = format!(
            "{}/stems/.ready-11111111111141118111111111111111",
            material_root.to_string_lossy().replace('\\', "/")
        );
        let generation = root.parent().unwrap().join(&wav_reference);
        fs::create_dir_all(&generation).unwrap();
        let mut hashes = serde_json::Map::new();
        for (index, name) in ["vocals", "melody", "bass", "drums", "instrumental"]
            .iter()
            .enumerate()
        {
            let encoded = wav(if index == 4 { 3 } else { 1 });
            fs::write(generation.join(format!("{name}.wav")), &encoded).unwrap();
            hashes.insert(
                (*name).to_owned(),
                serde_json::json!(format!("{:x}", Sha256::digest(&encoded))),
            );
        }
        fs::write(
            generation.join(".complete.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema": "stem-set-sha256-v1", "source_version": source_version, "stems": hashes,
            }))
            .unwrap(),
        )
        .unwrap();
        let reader = Arc::new(
            super::super::stem_pair::prepare_complete_pair(
                &root,
                &material,
                &source_version,
                &wav_reference,
                &|| false,
            )
            .unwrap(),
        );
        let identity = std::array::from_fn(|index| {
            u8::from_str_radix(
                &reader.descriptor.stem_set_identity[index * 2..index * 2 + 2],
                16,
            )
            .unwrap()
        });
        let fixture = Fixture::with_sample(material.sample.clone());
        assert!(fixture.binding.current());
        let owner = Arc::new(SelectionOwner {
            id: 0,
            binding: fixture.binding.binding,
            source_generation: 7,
            source_version,
            identity: Arc::new(identity),
            pair: StemPairOwner {
                root,
                descriptor_reference: reader.descriptor_reference.clone(),
                source_lease: material.lease.clone(),
                reader,
            },
            generation_path: generation,
            epoch: Arc::new(AtomicU64::new(1)),
            expected: 1,
            revoked: Arc::new(AtomicBool::new(false)),
            source_epoch: fixture.engine.prepared_source_epoch(0).unwrap().clone(),
            ownership: fixture.engine.input_runtime_ownership.clone(),
        });
        assert!(owner.current());
        Self {
            owner,
            fixture,
            _directory: directory,
        }
    }
    fn staged(&self, policy: bool) -> ResidentStemTicket {
        let epoch = if policy {
            Arc::new(AtomicU64::new(9))
        } else {
            self.fixture.epoch.clone()
        };
        let publication = PreparedSourcePermit::new(epoch, 10);
        publication.mark_pending().unwrap();
        let proposal = if policy {
            self.fixture.engine.resident_proposals.begin_policy(0)
        } else {
            self.fixture.engine.resident_proposals.begin_stems(0)
        }
        .unwrap();
        proposal.arm().unwrap();
        ResidentStemTicket {
            status: Arc::new(ResidencyStatus {
                publication,
                effective: AtomicU8::new(0),
                effective_failure: AtomicBool::new(false),
                error: Mutex::new(None),
                proposal: Some(proposal),
                policy_epoch: policy.then(|| (Arc::new(AtomicU64::new(17)), 18)),
                ownership: self.fixture.engine.input_runtime_ownership.clone(),
                id: 0,
                binding: self.fixture.binding.binding,
                resident_control: !policy,
                control_published: AtomicBool::new(false),
                preparation_clock:
                    super::super::resident_relocation::ResidentPreparationClock::default(),
                control_wait_observed: AtomicBool::new(false),
                failure_pending: AtomicBool::new(false),
                failure_settled: AtomicBool::new(false),
                failed_settlement: Mutex::new(false),
            }),
            owner: self.owner.clone(),
            full_mix: true,
            binding: self.fixture.binding.binding,
            launch_revision: self
                .fixture
                .engine
                .input_runtime_ownership
                .launch_revision(0),
            acknowledgements: self.fixture.binding.acknowledgements.clone(),
            candidate: None,
            policy_only: policy,
        }
    }
}

#[test]
fn committed_armed_unpublished_public_ordinary_and_policy_receipts_remain_current() {
    for policy in [false, true] {
        let fixture = PublicTicketFixture::new();
        let ticket = fixture.staged(policy);
        assert!(
            !ticket.status.publication.current(),
            "future epoch is not published yet"
        );
        assert!(ticket.status.proposal.as_ref().unwrap().armed());
        assert!(ticket.is_current());
        assert_eq!(ticket.residency_status(), "pending");
        assert_eq!(
            ticket.effective_mode_status(),
            if policy { "not_requested" } else { "waiting" }
        );
        assert!(ticket.error().is_none());
        let control =
            CommittedControlWrite::capture(&fixture.fixture.binding, 9, (!policy).then_some(10))
                .unwrap();
        control
            .commit(
                &fixture.fixture.binding,
                &fixture.fixture.epoch,
                || true,
                || {
                    if policy {
                        ticket.status.publication.epoch.store(10, Ordering::Release);
                    }
                    ticket
                        .status
                        .control_published
                        .store(true, Ordering::Release);
                    assert!(ticket.is_current());
                },
            )
            .unwrap();
        assert!(ticket.is_current());
        assert!(ticket.error().is_none());
        ticket.status.publication.epoch.store(11, Ordering::Release);
        assert!(
            !ticket.is_current(),
            "published receipts require their actual epoch"
        );
        assert_eq!(
            ticket.error().as_deref(),
            Some("Committed stem residency or mode ownership changed")
        );
    }
}

#[test]
fn committed_armed_unpublished_public_receipts_reject_real_cancellation_and_supersession() {
    for policy in [false, true] {
        let fixture = PublicTicketFixture::new();
        let cancelled = fixture.staged(policy);
        assert!(cancelled.withdraw());
        assert!(!cancelled.is_current());
        assert_eq!(cancelled.residency_status(), "rejected");
        assert_eq!(
            cancelled.error().as_deref(),
            Some("Committed stem residency or mode ownership changed")
        );
        let superseded = fixture.staged(policy);
        let _replacement = fixture.staged(policy);
        assert!(!superseded.is_current());
        assert_eq!(
            superseded.error().as_deref(),
            Some("Committed stem residency or mode ownership changed")
        );
        assert_eq!(fixture.fixture.counters(), (9, 6, 4));
    }
}

#[test]
fn committed_final_apply_serializes_failure_then_retains_pending_native_settlement() {
    let fixture = PublicTicketFixture::new();
    let ticket = fixture.staged(false);
    let control = fixture.fixture.control();
    let binding = &fixture.fixture.binding;
    let epoch = &fixture.fixture.epoch;
    let status = ticket.status.clone();
    let (mut producer, mut queued) = rtrb::RingBuffer::<u64>::new(1);
    let (claimed, observed) = mpsc::channel();
    let (apply, resume) = mpsc::channel();
    let (next_marker, marker_ready) = mpsc::channel();
    let (attempting, attempted) = mpsc::channel();
    let (finished, failed) = mpsc::channel();
    std::thread::scope(|threads| {
        let publisher = threads.spawn(move || {
            status.publish_control(|| {
                control
                    .commit(
                        binding,
                        epoch,
                        || {
                            let current = !status.failure_pending.load(Ordering::Acquire)
                                && status.publication.status() == "pending"
                                && status.proposal.as_ref().unwrap().current();
                            assert!(current);
                            // Exactly the old race: every final guard passed, but apply has not run.
                            claimed.send(()).unwrap();
                            resume.recv_timeout(Duration::from_secs(5)).unwrap();
                            current
                        },
                        || {
                            status.control_published.store(true, Ordering::Release);
                            producer.push(status.publication.expected).unwrap();
                        },
                    )
                    .map_err(|error| error.to_string())?;
                // The publication marker is released, while the same failure gate still holds.
                // A new marker forces the subsequent external settlement to retain Pending.
                assert!(
                    next_marker
                        .send(PendingWriter::hold(status.ownership.clone()))
                        .is_ok()
                );
                Ok(())
            })
        });
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let failing_status = ticket.status.clone();
        let failure = threads.spawn(move || {
            attempting.send(()).unwrap();
            finished
                .send(failing_status.fail("injected failure after final guards".into()))
                .unwrap();
        });
        attempted.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(ticket.status.failed_settlement.try_lock().is_err());
        assert!(failed.try_recv().is_err());
        assert!(!ticket.status.failure_pending.load(Ordering::Acquire));
        assert!(!ticket.status.control_published.load(Ordering::Acquire));
        assert_eq!(ticket.residency_status(), "pending");
        assert!(ticket.error().is_none());
        assert_eq!(fixture.fixture.counters(), (9, 6, 4));
        apply.send(()).unwrap();
        let held = marker_ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(failed.recv_timeout(Duration::from_secs(5)).unwrap());
        publisher.join().unwrap().unwrap();
        failure.join().unwrap();
        assert!(ticket.status.control_published.load(Ordering::Acquire));
        assert_eq!(queued.pop().unwrap(), 10);
        assert_eq!(fixture.fixture.counters(), (10, 10, 4));
        assert_eq!(ticket.status.publication.status(), "rejected");
        assert_eq!(ticket.residency_status(), "pending");
        assert_eq!(ticket.effective_mode_status(), "waiting");
        assert!(ticket.status.settlement_pending());
        assert!(!ticket.status.failure_settled.load(Ordering::Acquire));
        held.complete();
        ticket.status.retry_failure();
        assert_eq!(ticket.residency_status(), "rejected");
        assert_eq!(ticket.effective_mode_status(), "rejected");
        assert!(!ticket.status.settlement_pending());
        assert_eq!(fixture.fixture.counters(), (10, 0, 4));
        assert_eq!(
            fixture
                .fixture
                .engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .content_key(),
            fixture.fixture.placement.content_key()
        );
    });
}

#[test]
fn committed_control_after_capture_pending_preserves_image_then_retries() {
    let fixture = Fixture::new();
    let control = fixture.control();
    let held = Arc::new(Mutex::new(None));
    let keep = held.clone();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let applied = Cell::new(false);
    let result = with_before_commit(
        move || *keep.lock().unwrap() = Some(PendingWriter::hold(ownership)),
        || {
            control.commit(
                &fixture.binding,
                &fixture.epoch,
                || true,
                || applied.set(true),
            )
        },
    );
    let before_retry = fixture.counters();
    held.lock().unwrap().take().unwrap().complete();
    assert!(result.is_err());
    assert!(!applied.get());
    assert_eq!(before_retry, (9, 6, 4));
    control
        .commit(
            &fixture.binding,
            &fixture.epoch,
            || true,
            || {
                assert!(fixture.engine.input_runtime_ownership.placement_pending());
                assert_eq!(fixture.counters(), (10, 10, 4));
            },
        )
        .unwrap();
    let after = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap();
    assert_eq!(after.content_key(), fixture.placement.content_key());
    assert_eq!(after.binding_epoch(), fixture.placement.binding_epoch());
}

#[test]
fn committed_control_original_counter_source_and_ack_changes_reject_before_claim() {
    for change in 0..7 {
        let fixture = Fixture::new();
        let control = fixture.control();
        let ownership = &fixture.engine.input_runtime_ownership;
        match change {
            0 => fixture.epoch.store(20, Ordering::Release),
            1 => ownership
                .resident_control_for_cell(fixture.placement.cell())
                .store(21, Ordering::Release),
            2 => ownership
                .key_lock_request_for_cell(fixture.placement.cell())
                .store(22, Ordering::Release),
            3 => ownership
                .authority_for_cell(fixture.placement.cell())
                .store(23, Ordering::Release),
            4 => {
                assert!(fixture.binding.acknowledgements.acknowledge_for_key(
                    ownership,
                    fixture.placement.content_key().unwrap(),
                    24,
                ));
            }
            5 => {
                let write = ownership
                    .prepare_slot_write_fixed(
                        fixture.placement,
                        super::super::content_lifetime::reserve().unwrap(),
                    )
                    .unwrap();
                ownership
                    .commit_slot_write_fixed(&write, || true, |_| {})
                    .unwrap();
            }
            _ => {
                let replacement = SampleBuffer {
                    samples: Arc::from([0.5_f32; 64]),
                    channels: 1,
                    residency: None,
                }
                .with_complete_source(48_000);
                ownership.publish_test_source(0, &replacement, 48_000, 8);
            }
        }
        let before = fixture.counters();
        let claimed = Cell::new(false);
        assert!(
            control
                .commit(
                    &fixture.binding,
                    &fixture.epoch,
                    || {
                        claimed.set(true);
                        true
                    },
                    || panic!("stale captured image cannot apply")
                )
                .is_err()
        );
        assert!(!claimed.get());
        assert_eq!(fixture.counters(), before);
    }
}

#[test]
fn committed_failure_pending_retains_terminal_until_poll_retry() {
    for replacement in [false, true] {
        let fixture = Fixture::new();
        let status = fixture.status(true);
        let held = PendingWriter::hold(fixture.engine.input_runtime_ownership.clone());
        assert!(status.fail("injected worker failure".into()));
        assert_eq!(status.publication_status(), "pending");
        assert_eq!(
            status.publication.status(),
            "rejected",
            "private cancellation CAS fences callback claim"
        );
        assert_eq!(status.effective.load(Ordering::Acquire), 0);
        assert!(status.settlement_pending());
        assert!(!status.preparation_current());
        held.complete();
        if replacement {
            fixture
                .engine
                .input_runtime_ownership
                .resident_control_for_cell(fixture.placement.cell())
                .store(23, Ordering::Release);
        }
        status.retry_failure();
        assert_eq!(status.publication.status(), "rejected");
        assert_eq!(status.effective.load(Ordering::Acquire), 3);
        assert!(!status.settlement_pending());
        assert_eq!(fixture.counters().1, if replacement { 23 } else { 0 });
    }
}

#[test]
fn committed_unpublished_worker_and_retired_lifetime_preserve_same_counter_owner() {
    for published in [false, true] {
        let fixture = Fixture::new();
        let status = fixture.status(published);
        if published {
            let ownership = &fixture.engine.input_runtime_ownership;
            let write = ownership
                .prepare_slot_write_fixed(
                    fixture.placement,
                    super::super::content_lifetime::reserve().unwrap(),
                )
                .unwrap();
            ownership
                .commit_slot_write_fixed(
                    &write,
                    || true,
                    |cell| {
                        ownership
                            .resident_control_for_cell(cell)
                            .store(6, Ordering::Release);
                    },
                )
                .unwrap();
        }
        drop(ResidencyJobGuard {
            status: status.clone(),
            enqueued: false,
        });
        assert_eq!(status.publication.status(), "rejected");
        assert_eq!(fixture.counters().1, 6);
    }
}

#[test]
fn committed_callback_claim_wins_failure_without_cleanup_or_rollback_status() {
    let fixture = Fixture::new();
    let status = fixture.status(true);
    assert!(status.publication.claim_resident());
    assert!(!status.fail("too late to cancel actual adoption".into()));
    assert!(!status.settlement_pending());
    assert!(!status.failure_pending.load(Ordering::Acquire));
    assert_eq!(status.publication.status(), "adopting");
    assert_eq!(fixture.counters().1, 6);
    status.publication.mark_accepted();
    assert_eq!(status.publication_status(), "accepted");
    assert_eq!(status.effective.load(Ordering::Acquire), 0);
}

#[test]
fn committed_external_finish_waits_for_moved_control_mirror_and_callback_finish_needs_no_nested_claim()
 {
    let fixture = Fixture::new();
    let status = fixture.status(true);
    let ownership = &fixture.engine.input_runtime_ownership;
    let pair = ownership
        .prepare_placement_pair(fixture.placement, ownership.slot_snapshot(1).unwrap())
        .unwrap();
    ownership
        .commit_placement_pair(&pair, true, || true, || {})
        .unwrap();
    let moved = ownership.slot_snapshot(1).unwrap();
    let replacement = ownership.slot_snapshot(0).unwrap();
    ownership
        .resident_control_for_cell(replacement.cell())
        .store(6, Ordering::Release);
    let write = ownership
        .prepare_slot_write_fixed(moved, moved.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                status.finish_control_rt();
                assert_eq!(fixture.counters().1, 0);
                assert_eq!(
                    ownership
                        .resident_control_for_cell(replacement.cell())
                        .load(Ordering::Acquire),
                    6
                );
            },
        )
        .unwrap();
    ownership
        .resident_control_for_cell(moved.cell())
        .store(6, Ordering::Release);
    assert!(status.fail("failure after move".into()));
    assert_eq!(status.publication_status(), "pending");
    assert_eq!(fixture.counters().1, 6);
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                ownership.publish_pair_control_slots(&[replacement, moved]);
            },
        )
        .unwrap();
    status.retry_failure();
    assert_eq!(fixture.counters().1, 0);
    assert_eq!(
        ownership
            .resident_control_for_cell(replacement.cell())
            .load(Ordering::Acquire),
        6
    );
}

#[test]
fn committed_exchanged_final_owner_is_destroyed_after_marker_release() {
    struct Owner {
        ownership: Arc<InputRuntimeOwnership>,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            assert!(!self.ownership.placement_pending());
            self.dropped.store(true, Ordering::Release);
        }
    }
    let fixture = Fixture::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let mut owner = Some(Owner {
        ownership: fixture.engine.input_runtime_ownership.clone(),
        dropped: dropped.clone(),
    });
    let mut displaced = None;
    fixture
        .control()
        .commit(
            &fixture.binding,
            &fixture.epoch,
            || true,
            || {
                displaced = owner.take();
                assert!(!dropped.load(Ordering::Acquire));
            },
        )
        .unwrap();
    assert!(!dropped.load(Ordering::Acquire));
    drop(displaced);
    assert!(dropped.load(Ordering::Acquire));
}

struct CallbackRetirement {
    ownership: Arc<InputRuntimeOwnership>,
    work: Vec<Box<CommittedStemRtWork>>,
}
impl super::super::buffer_retirement::AudioBufferRetirement for CallbackRetirement {
    fn retire_committed_stem_work(&mut self, work: Box<CommittedStemRtWork>) {
        assert!(
            self.ownership.placement_pending(),
            "actual committed callback owns the marker"
        );
        self.work.push(work);
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<super::super::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: Arc<AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<AtomicU8>) {}
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {}
    fn retire_constant_timing(&mut self, _: super::super::constant_timing::PreparedConstantTiming) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<super::super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<super::super::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn available_retirement_slots(&mut self) -> usize {
        usize::MAX
    }
}

#[test]
fn committed_actual_callback_retains_identical_work_on_pending_then_checks_source_under_marker() {
    for stale in [false, true] {
        let fixture = Fixture::new();
        let ownership = fixture.engine.input_runtime_ownership.clone();
        let mut mixer = Box::new(super::super::mixer::RtMixer::new(1, 48_000.0));
        mixer.set_input_runtime_ownership(ownership.clone());
        mixer.set_current_timing_acknowledgements(fixture.binding.acknowledgements.clone());
        let sample = fixture.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap();
        let mut retirement = CallbackRetirement {
            ownership: ownership.clone(),
            work: Vec::new(),
        };
        assert!(mixer.load_sample_rt(0, sample, &mut retirement));
        let publication = PreparedSourcePermit::new(Arc::new(AtomicU64::new(1)), 1);
        publication.mark_pending().unwrap();
        let selected_epoch = Arc::new(AtomicU64::new(0));
        let work = Box::new(CommittedStemRtWork::Register {
            id: 0,
            binding: fixture.binding.binding,
            publication: publication.clone(),
            selection: SelectionPermit {
                identity: [7; 32],
                source_generation: 7,
                epoch: selected_epoch.clone(),
                expected: 1,
                revoked: Arc::new(AtomicBool::new(false)),
            },
        });
        let address = work.as_ref() as *const CommittedStemRtWork as usize;
        let held = PendingWriter::hold(ownership.clone());
        mixer.apply_committed_stem_work(work, &mut retirement);
        let Some(ControlMessage::CommittedStemWork(retained)) = mixer.take_deferred_control()
        else {
            panic!("same complete work must be deferred");
        };
        assert_eq!(
            retained.as_ref() as *const CommittedStemRtWork as usize,
            address
        );
        assert!(retirement.work.is_empty());
        assert_eq!(publication.status(), "pending");
        held.complete();
        if stale {
            ownership
                .authority_for_cell(fixture.placement.cell())
                .store(29, Ordering::Release);
        }
        mixer.apply_committed_stem_work(retained, &mut retirement);
        assert_eq!(
            publication.status(),
            if stale { "rejected" } else { "accepted" }
        );
        assert_eq!(selected_epoch.load(Ordering::Acquire), u64::from(!stale));
        assert_eq!(retirement.work.len(), 1);
        assert!(!ownership.placement_pending());
        retirement.work.clear();
    }
}

#[test]
fn committed_actual_callback_capture_publishes_current_history_while_marker_owned() {
    let fixture = Fixture::new();
    let ownership = fixture.engine.input_runtime_ownership.clone();
    let mut mixer = Box::new(super::super::mixer::RtMixer::new(1, 48_000.0));
    mixer.set_input_runtime_ownership(ownership.clone());
    mixer.set_current_timing_acknowledgements(fixture.binding.acknowledgements.clone());
    mixer.set_prepared_source_epochs(fixture.engine.prepared_source_epochs.clone());
    let sample = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let mut retirement = CallbackRetirement {
        ownership: ownership.clone(),
        work: Vec::new(),
    };
    assert!(mixer.load_sample_rt(0, sample.clone(), &mut retirement));
    let status = fixture.status(false);
    let (sender, mut receiver) = rtrb::RingBuffer::new(1);
    let capture = Arc::new(StemVoiceCapture {
        id: 0,
        binding: fixture.binding.binding,
        status: status.clone(),
        selection: SelectionPermit {
            identity: [7; 32],
            source_generation: 7,
            epoch: Arc::new(AtomicU64::new(1)),
            expected: 1,
            revoked: Arc::new(AtomicBool::new(false)),
        },
        launch_revision: ownership.launch_revision(0),
        _operation_timing_revision: None,
        target: StemMixMode::FullMix,
        candidate: None,
        sender: UnsafeCell::new(sender),
        used: AtomicBool::new(false),
    });
    mixer.apply_committed_stem_work(
        Box::new(CommittedStemRtWork::Capture(capture)),
        &mut retirement,
    );
    assert_eq!(status.publication.status(), "pending");
    let pin = receiver
        .pop()
        .expect("a complete source yields current captured history");
    assert!(pin.permit.current(&sample, 48_000));
    assert!(!ownership.placement_pending());
    assert_eq!(retirement.work.len(), 1);
    retirement.work.clear();
}
