//! Bounded R4 capacity retry and HC-10 Clear/refill/old-Hold composition.
//! Actual Native messages, sources, lifetimes, effects and retirement worker only.
use super::*;
use crate::audio_engine::buffer_retirement::{AudioBufferRetirement, RtAudioBufferRetirement};
use crate::audio_engine::constants::{MAX_VOICES, NUM_PADS};
use crate::audio_engine::pad_copy::bank::{BankMutationPublication, BankMutationRtWork};
use crate::messages::{PreparedStemSet, SampleBuffer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetiredBank {
    owner: u64,
    required: usize,
}

/// The established Pair RetirementGate boundary, borrowing this callback's real
/// retirement lane. Accepted owners always reach the production off-RT worker.
struct RetirementGate<'a> {
    available: usize,
    inner: &'a mut RtAudioBufferRetirement,
    bank: &'a mut Option<RetiredBank>,
}

impl AudioBufferRetirement for RetirementGate<'_> {
    fn retire_bank_mutation(&mut self, work: Box<BankMutationRtWork>) {
        assert!(
            self.bank.is_none(),
            "one actual parent payload retires once"
        );
        *self.bank = Some(RetiredBank {
            owner: work.guard.owner,
            required: work.retirement_slots_needed(),
        });
        self.inner.retire_bank_mutation(work);
    }

    fn retire_bank_publication(&mut self, publication: Arc<BankMutationPublication>) {
        self.inner.retire_bank_publication(publication);
    }

    fn retire_pad_pair(&mut self, work: Box<crate::audio_engine::pad_pair::PadPairRtWork>) {
        self.inner.retire_pad_pair(work);
    }

    fn retire_hold_action(
        &mut self,
        work: Box<crate::audio_engine::hold_action::HoldActionRtWork>,
    ) {
        self.inner.retire_hold_action(work);
    }

    fn retire_source_publication(
        &mut self,
        publication: crate::audio_engine::input_runtime_binding::SourcePublication,
    ) {
        self.inner.retire_source_publication(publication);
    }

    fn retire_pad_copy(&mut self, work: Box<crate::audio_engine::pad_copy::PadCopyRtWork>) {
        self.inner.retire_pad_copy(work);
    }

    fn retire_committed_stem_work(
        &mut self,
        work: Box<crate::audio_engine::committed_stem_residency::CommittedStemRtWork>,
    ) {
        self.inner.retire_committed_stem_work(work);
    }

    fn retire_resident_capture(
        &mut self,
        capture: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        self.inner.retire_resident_capture(capture);
    }

    fn retire_resident_cancellation(&mut self, cancelled: Arc<std::sync::atomic::AtomicBool>) {
        self.inner.retire_resident_cancellation(cancelled);
    }

    fn retire_resident_transaction(&mut self, work: Box<crate::messages::ResidentTransaction>) {
        self.inner.retire_resident_transaction(work);
    }

    fn retire_cold_adoption(&mut self, adoption: Arc<std::sync::atomic::AtomicU8>) {
        self.inner.retire_cold_adoption(adoption);
    }

    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.inner.retire_sample(sample);
    }

    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.inner.retire_prepared_stems(stems);
    }

    fn retire_constant_timing(
        &mut self,
        timing: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
        self.inner.retire_constant_timing(timing);
    }

    fn retire_global_playback_batch(
        &mut self,
        batch: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.inner.retire_global_playback_batch(batch);
    }

    fn retire_accepted_timing_refresh(
        &mut self,
        refresh: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.inner.retire_accepted_timing_refresh(refresh);
    }

    fn available_retirement_slots(&mut self) -> usize {
        self.available.min(self.inner.available_retirement_slots())
    }
}

fn drain_limited(
    callback: &mut Callback,
    available: usize,
    retired: &mut Option<RetiredBank>,
) -> usize {
    assert_eq!(callback.frame, callback.transport.output_frame());
    drain_parameter_messages(
        &mut callback.parameters,
        &mut callback.mixer,
        &mut callback.transport,
    );
    let mut gate = RetirementGate {
        available,
        inner: &mut callback.retirement,
        bank: retired,
    };
    drain_control_messages(
        &mut callback.controls,
        &mut callback.scheduler,
        callback.frame,
        &mut callback.quantization,
        &mut callback.transport,
        &mut callback.mixer,
        &mut callback.feedback,
        &mut gate,
    )
}

fn fixture(stems: bool) -> BankFixture {
    use std::io::Write;
    use std::os::windows::fs::MetadataExt;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/bank36-closure-cases-20261020");
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => assert_eq!(
                metadata.file_attributes() & 0x400,
                0,
                "owned closure fixture must not traverse a reparse point"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("closure fixture ancestor is unavailable: {error}"),
        }
    }
    fs::create_dir_all(&parent).unwrap();
    for ancestor in parent.ancestors() {
        assert_eq!(
            fs::symlink_metadata(ancestor).unwrap().file_attributes() & 0x400,
            0
        );
    }
    let owned = tempfile::Builder::new()
        .prefix("bank-closure-")
        .tempdir_in(parent)
        .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned.path().join("OWNER.md"))
        .unwrap()
        .write_all(b"Owned synthetic R4 Native capacity/Clear-refill fixture; no Human data, device or installation. Jobs/readers drop before this root.")
        .unwrap();
    BankFixture {
        fixture: Fixture::with_pair_registration_at(
            stems,
            true,
            true,
            "manual",
            Some(owned.path().to_path_buf()),
        ),
        _owned_root: owned,
    }
}

fn full_mix_profile_fixture() -> BankFixture {
    // The established independent Bank PCM profile uses FullMix. A registered
    // ALL fixture renders host-dependent 17-frame residency polls before return;
    // those unequal histories cannot serve as independent exact PCM references.
    // Keep component-owner coverage in the capacity case instead. All three
    // compared voices here inherit FullMix and actual ON Native preparation.
    let mut fixture = fixture(false);
    fixture.with_engine(|engine| engine.stop_sample(0).unwrap());
    assert_eq!(fixture.callback.drain(), 1);
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(0))
    );
    // Reuse controller_native_tests::controller's stopped FullMix/start pattern.
    // Its initial headless mode command uses the existing authoritative dispatcher.
    fixture
        .producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemMixMode {
            id: 0,
            mode: StemMixMode::FullMix,
            source_version_hash: 0,
        })
        .unwrap();
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(
        fixture.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert!(fixture.callback.mixer.key_lock_for_measurement(0));
    fixture.start(0);
    let warm = fixture.callback.render(16_384);
    assert!(warm.iter().all(|sample| sample.is_finite()));
    assert!(warm.iter().any(|sample| sample.abs() > 1e-4));
    assert_ne!(voice(&fixture, 0).3, 0);
    fixture
}

fn snapshots(fixture: &Fixture) -> Vec<crate::audio_engine::content_placement::SlotSnapshot> {
    fixture.with_engine(|engine| {
        (0..NUM_SAMPLES)
            .map(|slot| engine.input_runtime_ownership.slot_snapshot(slot).unwrap())
            .collect()
    })
}

fn assert_snapshots(
    fixture: &Fixture,
    expected: &[crate::audio_engine::content_placement::SlotSnapshot],
) {
    for (current, original) in snapshots(fixture).iter().zip(expected) {
        assert!(current.same_projection(*original));
    }
}

fn sample_backing(fixture: &Fixture) -> Vec<Option<(usize, usize, usize)>> {
    fixture
        .callback
        .mixer
        .bank_for_measurement()
        .iter()
        .map(|sample| {
            sample.as_ref().map(|sample| {
                (
                    sample.samples.as_ptr() as usize,
                    sample.channels,
                    sample.frame_count(),
                )
            })
        })
        .collect()
}

fn stem_backing(fixture: &Fixture) -> Vec<Option<usize>> {
    fixture
        .callback
        .mixer
        .stems_for_measurement()
        .iter()
        .map(|stems| {
            stems
                .as_ref()
                .map(|stems| Arc::as_ptr(&stems.complete_set_identity) as usize)
        })
        .collect()
}

#[test]
fn actual_bank_retirement_threshold_retains_same_parent_until_complete_capacity_returns() {
    isolated(
        "bank_public_tests::closure_cases::actual_bank_retirement_threshold_retains_same_parent_until_complete_capacity_returns",
        || {
            let mut fixture = fixture(true);
            for slot in 36..72 {
                put(&mut fixture, slot);
            }
            put(&mut fixture, 72);
            for slot in [37, 71, 72] {
                fixture.start(slot);
            }
            fixture.callback.render(37);
            let source = hold(&fixture, 0, 901);
            let removed = hold(&fixture, 37, 902);
            let outside = hold(&fixture, 72, 903);
            fixture.callback.drain();
            for action in [&source, &removed, &outside] {
                assert_eq!(action.action.state(), HoldActionState::PausedOwned);
            }
            let effects = [
                source.action.effect(),
                removed.action.effect(),
                outside.action.effect(),
            ];
            let before_voices = [
                voice(&fixture, 0),
                voice(&fixture, 37),
                voice(&fixture, 71),
                voice(&fixture, 72),
            ];
            let before_slots = snapshots(&fixture);
            let before_backing = sample_backing(&fixture);
            let before_stems = stem_backing(&fixture);
            let captured = capture(&fixture, Some(0), 1);
            let ticket = prepare(&fixture, &captured);
            fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
            let (requests, reservations) = fixture.with_engine(|engine| {
                (
                    engine.pad_request_ids.lock().unwrap().clone(),
                    (0..NUM_SAMPLES)
                        .map(|slot| engine.input_runtime_ownership.copy_reservation(slot))
                        .collect::<Vec<_>>(),
                )
            });
            let pending: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
            let owner = pending["owner"].as_u64().unwrap();
            let publication = match fixture.callback.controls.peek().unwrap() {
                ControlMessage::BankMutation(publication) => publication.clone(),
                other => panic!("expected the actual prepared Bank payload: {other:?}"),
            };
            // The fixed bound accounts independently for every possible voice's
            // owners, all36 displaced images and the one parent. The accepted
            // actual work below also reports its own exact required bound.
            let required = 3 * MAX_VOICES + NUM_PADS + 1;
            assert!(fixture.callback.retirement.available_retirement_slots() >= required);
            let mut retired = None;
            for _ in 0..3 {
                assert_eq!(
                    drain_limited(&mut fixture.callback, required - 1, &mut retired),
                    0
                );
                // The actual dispatcher checks the complete retirement bound
                // before dequeue. Retain that exact unclaimed queue-head owner.
                assert!(fixture.callback.mixer.deferred_control().is_none());
                match fixture.callback.controls.peek().unwrap() {
                    ControlMessage::BankMutation(same) => {
                        assert!(Arc::ptr_eq(same, &publication))
                    }
                    other => {
                        panic!("the identical unclaimed publication must remain queued: {other:?}")
                    }
                }
                assert_eq!(ticket.publication_status(), "pending");
                assert!(ticket.acknowledgement().is_none());
                assert!(retired.is_none());
                assert_snapshots(&fixture, &before_slots);
                assert_eq!(sample_backing(&fixture), before_backing);
                assert_eq!(stem_backing(&fixture), before_stems);
                for action in [&source, &removed, &outside] {
                    assert_eq!(action.action.state(), HoldActionState::PausedOwned);
                }
                assert_eq!(
                    [
                        voice(&fixture, 0),
                        voice(&fixture, 37),
                        voice(&fixture, 71),
                        voice(&fixture, 72)
                    ],
                    before_voices
                );
                assert_eq!(
                    [
                        source.action.effect(),
                        removed.action.effect(),
                        outside.action.effect()
                    ],
                    effects
                );
                fixture.with_engine(|engine| {
                    assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
                    for (slot, reservation) in reservations.iter().enumerate() {
                        assert_eq!(
                            engine.input_runtime_ownership.copy_reservation(slot),
                            *reservation
                        );
                    }
                });
            }
            assert_eq!(
                drain_limited(&mut fixture.callback, required, &mut retired),
                1
            );
            assert_eq!(retired, Some(RetiredBank { owner, required }));
            assert_eq!(ticket.publication_status(), "accepted");
            assert!(fixture.callback.mixer.deferred_control().is_none());
            let accepted: Value = serde_json::from_str(&ticket.metadata_json()).unwrap();
            assert_eq!(accepted["owner"], pending["owner"]);
            assert_eq!(accepted["targets"].as_array().unwrap().len(), 36);
            assert_eq!(source.action.state(), HoldActionState::PausedOwned);
            assert_eq!(outside.action.state(), HoldActionState::PausedOwned);
            assert_eq!(removed.action.state(), HoldActionState::Retired);
            assert!(fixture.callback.mixer.bank_for_measurement()[36].is_some());
            assert!(
                fixture.callback.mixer.bank_for_measurement()[37..72]
                    .iter()
                    .all(Option::is_none)
            );
            assert!(
                !fixture
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .any(|lane| (36..72).contains(&lane.sample_id) && (lane.active || lane.paused))
            );
            assert_eq!(voice(&fixture, 0), before_voices[0]);
            assert_eq!(voice(&fixture, 72), before_voices[3]);
            let after = snapshots(&fixture);
            assert_eq!(
                drain_limited(&mut fixture.callback, required, &mut retired),
                0
            );
            assert_snapshots(&fixture, &after);
            assert_eq!(
                serde_json::from_str::<Value>(&ticket.metadata_json()).unwrap(),
                accepted
            );
            finish(&fixture, &ticket);
            fixture.with_engine(|engine| {
                assert!(engine.release_waveform_hold(&source).unwrap());
                assert!(engine.release_waveform_hold(&outside).unwrap());
            });
            fixture.callback.drain();
            assert_eq!(source.action.state(), HoldActionState::Resumed);
            assert_eq!(outside.action.state(), HoldActionState::Resumed);
            let pcm = fixture.callback.render(96);
            assert!(pcm.iter().all(|sample| sample.is_finite()));
            assert!(pcm.iter().any(|sample| sample.abs() > 1e-6));
        },
    );
}

fn clear_refill_profile(deliver_old_release: bool) -> Vec<f32> {
    let mut fixture = full_mix_profile_fixture();
    for slot in [36, 71, 72] {
        put(&mut fixture, slot);
        fixture.start(slot);
    }
    fixture.callback.render(37);
    let source = hold(&fixture, 0, 911);
    let removed = hold(&fixture, 36, 912);
    let outside = hold(&fixture, 72, 913);
    fixture.callback.drain();
    for action in [&source, &removed, &outside] {
        assert_eq!(action.action.state(), HoldActionState::PausedOwned);
    }
    let survivor_voices = [voice(&fixture, 0), voice(&fixture, 72)];
    let original = snapshots(&fixture);
    let captured = capture(&fixture, None, 1);
    let ticket = prepare(&fixture, &captured);
    fixture.with_engine(|engine| engine.publish_bank_mutation(&ticket).unwrap());
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.acknowledgement().is_some());
    assert!(
        fixture.callback.mixer.bank_for_measurement()[36..72]
            .iter()
            .all(Option::is_none)
    );
    assert_eq!(removed.action.state(), HoldActionState::Retired);
    assert_eq!([voice(&fixture, 0), voice(&fixture, 72)], survivor_voices);
    finish(&fixture, &ticket);
    put(&mut fixture, 36);
    let refilled_birth = snapshots(&fixture);
    assert_ne!(refilled_birth[36].lifetime(), original[36].lifetime());
    assert_ne!(refilled_birth[36].content_key(), original[36].content_key());
    fixture.start(36);
    fixture.callback.render(64);
    let refilled = snapshots(&fixture);
    assert_eq!(refilled[36].content_key(), refilled_birth[36].content_key());
    let fresh = voice(&fixture, 36);
    assert!(!fresh.2);
    assert_ne!(
        fresh.3, 0,
        "fresh ON content owns its actual Native processor"
    );
    let fresh_effects = [source.action.effect(), outside.action.effect()];
    for _ in 0..3 {
        if deliver_old_release {
            fixture.with_engine(|engine| assert!(engine.release_waveform_hold(&removed).unwrap()));
        }
        // A terminal original Hold releases before any queue publication. Poll
        // both independent profiles identically, without rendering extra frames.
        assert!(fixture.callback.controls.is_empty());
        assert_eq!(fixture.callback.drain(), 0);
        assert_eq!(removed.action.state(), HoldActionState::Retired);
        assert_eq!(voice(&fixture, 36), fresh);
        assert_snapshots(&fixture, &refilled);
        assert_eq!([voice(&fixture, 0), voice(&fixture, 72)], survivor_voices);
        assert_eq!(
            [source.action.effect(), outside.action.effect()],
            fresh_effects
        );
        assert_eq!(source.action.state(), HoldActionState::PausedOwned);
        assert_eq!(outside.action.state(), HoldActionState::PausedOwned);
    }
    fixture.with_engine(|engine| {
        assert!(engine.release_waveform_hold(&source).unwrap());
        assert!(engine.release_waveform_hold(&outside).unwrap());
    });
    fixture.callback.drain();
    assert_eq!(source.action.state(), HoldActionState::Resumed);
    assert_eq!(outside.action.state(), HoldActionState::Resumed);
    let pcm = fixture.callback.render(96);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 1e-6));
    pcm
}

#[test]
fn actual_bank_clear_refill_started_birth_ignores_the_original_hold_release() {
    isolated(
        "bank_public_tests::closure_cases::actual_bank_clear_refill_started_birth_ignores_the_original_hold_release",
        || {
            let reference = clear_refill_profile(false);
            let actual = clear_refill_profile(true);
            assert_eq!(
                actual, reference,
                "old releases changed fresh or independently resumed PCM"
            );
        },
    );
}
