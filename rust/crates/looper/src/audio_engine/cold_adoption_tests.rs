//! Failure/race cases use the same productive worker and native callback drain.
use super::*;

struct Fixture {
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    callback: Callback,
    previous: SampleBuffer,
    source: PathBuf,
    original: Vec<u8>,
    directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("external.wav");
        let original = wav(&source, 48_000);
        let engine = AudioEngine::new().unwrap();
        let previous = old(&engine);
        let callback = Callback::new(&engine, &previous);
        let (producer, consumer) = rtrb::RingBuffer::new(8);
        Self {
            engine,
            producer: Arc::new(Mutex::new(producer)),
            consumer,
            callback,
            previous,
            source,
            original,
            directory,
        }
    }
    fn admit(&self, id: usize, automatic: bool) -> u64 {
        self.admit_replacing(id, automatic, false)
    }
    fn admit_replacing(&self, id: usize, automatic: bool, replace: bool) -> u64 {
        admit_for_format(
            &self.engine,
            id,
            self.source.to_string_lossy().into(),
            false,
            automatic,
            replace,
            self.producer.clone(),
            2,
            48_000,
            self.directory.path().join("samples"),
        )
        .unwrap()
    }
    fn pending(&mut self, id: usize, request: u64) {
        wait_until(|| self.engine.input_runtime_ownership.cold_status(id, request) == Some(0));
        wait_until(|| self.consumer.peek().is_ok());
    }
    fn accept_old_timing(&mut self) {
        use crate::audio_engine::constant_timing::{
            AcceptedTimingProjection, PreparedConstantTiming,
        };
        use crate::audio_engine::prepared_source::PreparedSourcePermit;
        let epoch = &self.engine.prepared_source_epochs[0];
        epoch.store(3, Ordering::Release);
        let permit = PreparedSourcePermit::for_epoch(epoch.clone(), 3);
        permit.mark_pending().unwrap();
        self.producer
            .lock()
            .unwrap()
            .push(ControlMessage::PublishConstantTiming {
                id: 0,
                timing: PreparedConstantTiming {
                    reference: self.previous.clone(),
                    publication: permit.clone(),
                    projection: AcceptedTimingProjection {
                        revision: [0x43; 32],
                        period_seconds: 0.5,
                        origin_seconds: -0.125,
                        sample_rate_hz: 48_000,
                        publication_epoch: 3,
                    },
                },
            })
            .unwrap();
        assert_eq!(self.callback.drain(&mut self.consumer), 1);
        assert_eq!(permit.status(), "accepted");
        assert_eq!(
            self.engine.current_timing_acknowledgements.current_epoch(0),
            3
        );
    }
    fn assert_no_terminal(&self) {
        while let Ok(event) = self.engine.loader_rx.lock().unwrap().try_recv() {
            assert!(
                !matches!(
                    event,
                    LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
                ),
                "{event:?}"
            );
        }
    }
    fn assert_rolled_back(&self) {
        assert!(Arc::ptr_eq(
            &self.previous.samples,
            &self.engine.sample_cache.lock().unwrap()[0]
                .as_ref()
                .unwrap()
                .samples
        ));
        assert_eq!(
            self.engine.loaded_source_generations.lock().unwrap()[0],
            (7, 48_000)
        );
        assert_eq!(
            self.engine.loaded_source_digests.lock().unwrap()[0].as_deref(),
            Some("old")
        );
        assert!(
            self.engine
                .input_runtime_ownership
                .source_current(0, &self.previous, 48_000)
        );
        assert!(self.engine.cold_leases.lock().unwrap()[0].is_none());
        assert_eq!(fs::read(&self.source).unwrap(), self.original);
        let cache = self.directory.path().join("samples/.pcm-cache/v1");
        if cache.exists() {
            assert_eq!(fs::read_dir(cache).unwrap().count(), 0);
        }
        assert!(!self.directory.path().join("samples/external.wav").exists());
        self.callback.assert_old_voice(&self.previous);
    }
}

#[test]
#[cfg(windows)]
fn retirement_backpressure_keeps_cold_pending_and_pinned_adoption_needs_no_feedback() {
    let mut fixture = Fixture::new();
    fixture.accept_old_timing();
    let request = fixture.admit(0, false);
    fixture.pending(0, request);
    fixture.callback.retirement.slots = 2;
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 0);
    fixture.callback.assert_old_voice(&fixture.previous);
    fixture.assert_no_terminal();
    assert_eq!(
        fixture
            .engine
            .current_timing_acknowledgements
            .current_epoch(0),
        3
    );
    fixture.callback.retirement.slots = 3;
    fixture.callback.feedback.slots = 0;
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Success { .. }
    ));
    assert!(fixture.callback.feedback.events.is_empty());
    fixture.callback.assert_old_voice(&fixture.previous);
    assert_eq!(
        fixture
            .engine
            .current_timing_acknowledgements
            .current_epoch(0),
        0
    );
}

#[test]
#[cfg(windows)]
fn explicit_assignment_replacement_stops_old_voice_only_after_real_callback_ack() {
    let mut fixture = Fixture::new();
    let request = fixture.admit_replacing(0, false, true);
    fixture.pending(0, request);
    fixture.callback.assert_old_voice(&fixture.previous);
    fixture.assert_no_terminal();
    fixture.callback.retirement.slots = MAX_VOICES + 2;
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 0);
    fixture.callback.assert_old_voice(&fixture.previous);
    fixture.assert_no_terminal();
    fixture.callback.retirement.slots = MAX_VOICES + 3;
    fixture.callback.feedback.slots = 0;
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 0);
    fixture.callback.assert_old_voice(&fixture.previous);
    fixture.assert_no_terminal();
    fixture.callback.feedback.slots = 1;
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Success { .. }
    ));
    assert!(
        !fixture
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert_eq!(fixture.callback.feedback.events.len(), 1);
    assert!(matches!(
        fixture.callback.feedback.events[0],
        AudioMessage::SampleStopped { id: 0 }
    ));
    assert!(fixture.callback.mixer.play_sample(0, 1.0));
    let new_source = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    let new_voice = fixture
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert!(Arc::ptr_eq(
        &new_voice.sample.as_ref().unwrap().samples,
        &new_source.samples
    ));
}

#[test]
#[cfg(windows)]
fn callback_moves_cold_ack_token_to_retirement_before_final_owner_release() {
    let mut fixture = Fixture::new();
    let request = fixture.admit(0, false);
    fixture.pending(0, request);
    let weak = match fixture.consumer.peek().unwrap() {
        ControlMessage::LoadColdSample { adoption, .. } => Arc::downgrade(adoption),
        other => panic!("expected cold command, got {other:?}"),
    };
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(|| fixture.engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert_eq!(
        weak.strong_count(),
        1,
        "retirement must retain the final token owner"
    );
    assert!(
        fixture
            .callback
            .retirement
            .retired
            .iter()
            .any(|item| matches!(item, RetiredAudioBuffer::ColdAdoption(_)))
    );
    fixture
        .callback
        .retirement
        .retired
        .retain(|item| !matches!(item, RetiredAudioBuffer::ColdAdoption(_)));
    assert!(
        weak.upgrade().is_none(),
        "off-callback retirement releases the token"
    );
}

#[test]
#[cfg(windows)]
fn superseding_after_actual_ack_keeps_adopted_files_durable_before_new_assignment() {
    let mut fixture = Fixture::new();
    let old_request = fixture.admit(0, false);
    fixture.pending(0, old_request);
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    unload_for_producer(&fixture.engine, 0, &fixture.producer).unwrap();
    let new_path = fixture.directory.path().join("after-ack.wav");
    let new_original = wav(&new_path, 44_100);
    let new_request = admit_for_format(
        &fixture.engine,
        0,
        new_path.to_string_lossy().into(),
        false,
        false,
        true,
        fixture.producer.clone(),
        2,
        48_000,
        fixture.directory.path().join("samples"),
    )
    .unwrap();
    fixture.pending(0, new_request);
    // Both terminal results are valid: metadata delivery may win or lose to unload.
    assert!(matches!(
        terminal(&fixture.engine, old_request),
        LoaderEvent::Success { .. } | LoaderEvent::Error { .. }
    ));
    assert_eq!(
        fs::read(fixture.directory.path().join("samples/external.wav")).unwrap(),
        fixture.original
    );
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 2);
    assert!(matches!(
        terminal(&fixture.engine, new_request),
        LoaderEvent::Success { .. }
    ));
    let leases = fixture.engine.cold_leases.lock().unwrap();
    let lease = leases[0].as_ref().unwrap();
    assert_eq!(fs::read(&lease.original_path).unwrap(), new_original);
    assert_eq!(
        fs::read_dir(fixture.directory.path().join("samples/.pcm-cache/v1"))
            .unwrap()
            .count(),
        2
    );
    assert!(fixture.callback.mixer.play_sample(0, 1.0));
}

#[test]
#[cfg(windows)]
fn callback_rejection_rolls_back_pending_cache_without_clearing_old_current_projection() {
    let mut fixture = Fixture::new();
    fixture.accept_old_timing();
    let request = fixture.admit(0, false);
    fixture.pending(0, request);
    // This is the production cancellation primitive for an unclaimed command.
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .cancel_cold(0, request)
    );
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Error { .. }
    ));
    fixture.assert_rolled_back();
    assert_eq!(
        fixture
            .engine
            .current_timing_acknowledgements
            .current_epoch(0),
        3
    );
    fixture.callback.assert_old_voice(&fixture.previous);
    assert_eq!(
        fixture
            .engine
            .current_timing_acknowledgements
            .current_epoch(0),
        3
    );
}

#[test]
#[cfg(windows)]
fn actual_unload_and_new_load_cannot_let_old_failure_rollback_new_source_or_disk_lease() {
    let mut fixture = Fixture::new();
    let old_request = fixture.admit(0, false);
    fixture.pending(0, old_request);
    unload_for_producer(&fixture.engine, 0, &fixture.producer).unwrap();
    let new_path = fixture.directory.path().join("new-source.wav");
    let new_original = wav(&new_path, 96_000);
    let new_request = admit_for_format(
        &fixture.engine,
        0,
        new_path.to_string_lossy().into(),
        false,
        false,
        true,
        fixture.producer.clone(),
        2,
        48_000,
        fixture.directory.path().join("samples"),
    )
    .unwrap();
    assert!(new_request > old_request);
    fixture.pending(0, new_request);
    let new_source = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert!(matches!(
        terminal(&fixture.engine, old_request),
        LoaderEvent::Error { .. }
    ));
    assert!(Arc::ptr_eq(
        &new_source.samples,
        &fixture.engine.sample_cache.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .samples,
    ));
    fixture.callback.assert_old_voice(&fixture.previous);
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 3);
    assert!(matches!(
        terminal(&fixture.engine, new_request),
        LoaderEvent::Success { .. }
    ));
    assert_eq!(
        fixture.engine.loaded_source_generations.lock().unwrap()[0],
        (new_request, 48_000)
    );
    assert_eq!(
        fixture.engine.loaded_source_digests.lock().unwrap()[0].as_deref(),
        Some(format!("{:x}", Sha256::digest(&new_original)).as_str()),
    );
    assert!(
        fixture
            .engine
            .input_runtime_ownership
            .source_current(0, &new_source, 48_000)
    );
    let leases = fixture.engine.cold_leases.lock().unwrap();
    let lease = leases[0].as_ref().unwrap();
    assert_eq!(fs::read(&lease.original_path).unwrap(), new_original);
    assert!(lease.cache_path.join("manifest.json").is_file());
    assert_eq!(
        fs::read_dir(fixture.directory.path().join("samples/.pcm-cache/v1"))
            .unwrap()
            .count(),
        1
    );
    assert!(
        !fixture
            .directory
            .path()
            .join("samples/external.wav")
            .exists()
    );
    drop(leases);
    wait_until(|| fixture.engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert!(
        !fixture
            .engine
            .loading_sample_ids
            .lock()
            .unwrap()
            .contains(&0)
    );
    assert!(fixture.callback.mixer.play_sample(0, 1.0));
}

#[test]
#[cfg(windows)]
fn actual_manual_and_tap_edits_reject_pending_cold_and_keep_new_intent_and_old_audio() {
    for intent in [TimingIntent::Manual, TimingIntent::Tap] {
        let mut fixture = Fixture::new();
        let request = fixture.admit(0, true);
        fixture.pending(0, request);
        crate::audio_engine::constant_timing::set_intent(
            &fixture.engine,
            &fixture.producer,
            0,
            intent,
        )
        .unwrap();
        assert!(matches!(
            terminal(&fixture.engine, request),
            LoaderEvent::Error { .. }
        ));
        fixture.assert_rolled_back();
        assert_eq!(fixture.engine.timing_intents.lock().unwrap()[0], intent);
        assert_eq!(fixture.callback.drain(&mut fixture.consumer), 2);
        fixture.callback.assert_old_voice(&fixture.previous);
        assert!(fixture.callback.feedback.events.is_empty());
    }
}

#[test]
#[cfg(windows)]
fn empty_legacy_load_survives_initial_epoch_edit_and_ack_does_not_create_acceptance() {
    let mut fixture = Fixture::new();
    let request = fixture.admit(1, false);
    fixture.pending(1, request);
    crate::audio_engine::constant_timing::set_intent(
        &fixture.engine,
        &fixture.producer,
        1,
        TimingIntent::Manual,
    )
    .unwrap();
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 2);
    let mut success = false;
    wait_until(|| {
        if let Ok(event) = fixture.engine.loader_rx.lock().unwrap().try_recv() {
            match event {
                LoaderEvent::Success {
                    id: 1, request_id, ..
                } if request_id == request => success = true,
                LoaderEvent::Error { id: 1, error, .. } => {
                    panic!("initial legacy edit rejected: {error}")
                }
                _ => {}
            }
        }
        success
    });
    assert_eq!(
        fixture.engine.timing_intents.lock().unwrap()[1],
        TimingIntent::Manual
    );
    assert_eq!(
        fixture
            .engine
            .current_timing_acknowledgements
            .current_epoch(1),
        0
    );
    assert!(fixture.engine.cold_leases.lock().unwrap()[1].is_some());
    assert!(fixture.callback.mixer.play_sample(1, 1.0));
}

#[test]
#[cfg(windows)]
fn shutdown_cancels_pending_and_queued_jobs_and_drains_owned_disk_artifacts() {
    let mut fixture = Fixture::new();
    let first = fixture.admit(0, false);
    fixture.pending(0, first);
    let second = fixture.admit(1, false);
    fixture.pending(1, second);
    let queued = fixture.admit(2, false);
    fixture.engine.shut_down().unwrap();
    fixture.assert_rolled_back();
    for id in 0..3 {
        assert_eq!(fixture.engine.cold_loading[id].load(Ordering::Acquire), 0);
        assert!(
            !fixture
                .engine
                .loading_sample_ids
                .lock()
                .unwrap()
                .contains(&id)
        );
    }
    assert!(fixture.engine.sample_cache.lock().unwrap()[1].is_none());
    assert!(fixture.engine.sample_cache.lock().unwrap()[2].is_none());
    let mut failures = Vec::new();
    while let Ok(event) = fixture.engine.loader_rx.lock().unwrap().try_recv() {
        match event {
            LoaderEvent::Error { id, request_id, .. } => failures.push((id, request_id)),
            LoaderEvent::Success { .. } => panic!("shutdown published success"),
            _ => {}
        }
    }
    assert!(failures.contains(&(0, first)));
    assert!(failures.contains(&(1, second)));
    assert!(!failures.contains(&(2, queued))); // Queued work never starts; its guard retires.
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 2);
    fixture.callback.assert_old_voice(&fixture.previous);
}

#[test]
fn admission_capacity_and_generation_overflow_fail_before_request_or_source_mutation() {
    let fixture = Fixture::new();
    let epoch = fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let reservations: Vec<_> = (0..crate::audio_engine::cold_jobs::QUEUED_JOBS)
        .map(|_| fixture.engine.cold_jobs.reserve().unwrap())
        .collect();
    let attempt = || {
        admit_for_format(
            &fixture.engine,
            0,
            fixture.source.to_string_lossy().into(),
            false,
            false,
            false,
            fixture.producer.clone(),
            2,
            48_000,
            fixture.directory.path().join("samples"),
        )
    };
    assert!(attempt().is_err());
    assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[0], 7);
    assert_eq!(
        fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire),
        epoch
    );
    assert!(
        !fixture
            .engine
            .loading_sample_ids
            .lock()
            .unwrap()
            .contains(&0)
    );
    drop(reservations);
    fixture.engine.pad_request_ids.lock().unwrap()[0] = u64::MAX / 4;
    assert!(attempt().is_err());
    assert_eq!(
        fixture.engine.pad_request_ids.lock().unwrap()[0],
        u64::MAX / 4
    );
    assert_eq!(
        fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire),
        epoch
    );
    fixture.assert_rolled_back();
}

#[test]
fn old_loading_guard_cannot_remove_new_job_while_waiting_for_loading_mutex() {
    let fixture = Fixture::new();
    fixture.engine.cold_loading[0].store(8, Ordering::Release);
    let mut loading = fixture.engine.loading_sample_ids.lock().unwrap();
    loading.insert(0);
    let guard = LoadingGuard {
        id: 0,
        request_id: 8,
        slots: fixture.engine.cold_loading.clone(),
        loading: fixture.engine.loading_sample_ids.clone(),
    };
    let job = std::thread::spawn(move || drop(guard));
    wait_until(|| fixture.engine.cold_loading[0].load(Ordering::Acquire) == 0);
    fixture.engine.cold_loading[0].store(9, Ordering::Release);
    drop(loading);
    job.join().unwrap();
    assert!(
        fixture
            .engine
            .loading_sample_ids
            .lock()
            .unwrap()
            .contains(&0)
    );
    assert_eq!(fixture.engine.cold_loading[0].load(Ordering::Acquire), 9);
    let _loading = fixture.engine.loading_sample_ids.lock().unwrap();
    drop(LoadingGuard {
        id: 0,
        request_id: 8,
        slots: fixture.engine.cold_loading.clone(),
        loading: fixture.engine.loading_sample_ids.clone(),
    });
}
