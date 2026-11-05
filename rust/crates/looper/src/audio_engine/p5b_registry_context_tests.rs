//! Saved file ownership across an unrelated complete 216-bank retirement.
use super::*;
use crate::audio_engine::project_assets::{FileIdentity, ProjectAssets, capture_identity};

#[path = "p5b_fixture_lifetime_tests.rs"]
mod fixture_lifetime_tests;
pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests) use fixture_lifetime_tests::observe_ordinary_harness;
pub(in crate::audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests) use fixture_lifetime_tests::{begin_ordinary_fixture, finish_cancelled_source_fixture, finish_ordinary_fixture, observe_ordinary_productive};

struct FileWitness {
    path: PathBuf,
    identity: FileIdentity,
    bytes: Vec<u8>,
}

impl FileWitness {
    fn capture(path: PathBuf) -> Self {
        Self {
            identity: capture_identity(&path).unwrap().unwrap(),
            bytes: fs::read(&path).unwrap(),
            path,
        }
    }

    fn verify(&self) {
        assert_eq!(
            capture_identity(&self.path).unwrap(),
            Some(self.identity.clone())
        );
        assert_eq!(fs::read(&self.path).unwrap(), self.bytes);
    }
}

struct SavedSource {
    _temp: tempfile::TempDir,
    root: PathBuf,
    original: FileWitness,
    cache: PathBuf,
    cache_identity: FileIdentity,
    cache_files: Vec<FileWitness>,
    assignments: [u64; 2],
    pcm: Vec<std::sync::Weak<[f32]>>,
    accepted_pcm: Vec<f32>,
    frames: usize,
    channels: usize,
}

impl SavedSource {
    fn after_engine_shutdown() -> Self {
        // The real fixture takes two ordinary source ACKs for one original.
        // Preserve its directory outside the old engine's lifetime; no file,
        // material, lease, ticket or PCM owner crosses the boundary below.
        let h = Harness::new();
        let original = FileWitness::capture(h.material.lease.original_path.clone());
        let cache = h.material.lease.cache_path.clone();
        let cache_identity = capture_identity(&cache).unwrap().unwrap();
        let cache_files = fs::read_dir(&cache)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                assert!(path.is_file());
                FileWitness::capture(path)
            })
            .collect();
        let assignments = [0, NUM_SAMPLES - 1].map(|id| {
            h.engine.cold_leases.lock().unwrap()[id]
                .as_ref()
                .unwrap()
                .assignment_id()
        });
        let accepted = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
        let accepted_pcm = accepted.samples.to_vec();
        let frames = accepted.frame_count();
        let channels = accepted.channels;
        let mut pcm = vec![Arc::downgrade(&h.material.sample.samples)];
        for id in [0, NUM_SAMPLES - 1] {
            pcm.push(Arc::downgrade(
                &h.callback.mixer.bank_for_measurement()[id]
                    .as_ref()
                    .unwrap()
                    .samples,
            ));
        }
        let Harness {
            _temp,
            root,
            mut engine,
            callback,
            producer,
            consumer,
            material,
            _original_owner,
            ..
        } = h;
        engine.cold_jobs.shutdown_checked().unwrap();
        assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
        drop((
            callback,
            consumer,
            producer,
            material,
            _original_owner,
            engine,
        ));
        wait_phase(
            "prior saved source PCM ends after engine shutdown",
            0,
            || pcm.iter().all(|sample| sample.strong_count() == 0),
        );
        Self {
            _temp,
            root,
            original,
            cache,
            cache_identity,
            cache_files,
            assignments,
            pcm,
            accepted_pcm,
            frames,
            channels,
        }
    }

    fn verify_saved_descriptor(&self) {
        assert!(self.pcm.iter().all(|sample| sample.strong_count() == 0));
        let assets = ProjectAssets::shared();
        let state = assets.state.lock().unwrap();
        assert!(
            state.readers.iter().any(|reader| {
                reader.cold.as_ref().is_some_and(|lease| {
                    self.assignments.contains(&lease.assignment_id())
                        && lease.original_path == self.original.path
                        && lease.cache_path == self.cache
                        && !lease.assignment_retired()
                }) && reader.pcm.iter().all(|pcm| pcm.strong_count() == 0)
            }),
            "the exact saved file descriptor must outlive its engine and PCM"
        );
        // Read-only values under the existing gate, without collector/pruning,
        // extra ledger-status calls or upgraded PCM locators.
        eprintln!(
            "saved registry context: readers={} available={} saved_assignments={:?}",
            state.readers.len(),
            ProjectAssets::available_reader_slots(&state),
            self.assignments
        );
        drop(state);
        self.original.verify();
        assert_eq!(
            capture_identity(&self.cache).unwrap(),
            Some(self.cache_identity.clone())
        );
        assert_eq!(
            fs::read_dir(&self.cache).unwrap().count(),
            self.cache_files.len()
        );
        for file in &self.cache_files {
            file.verify();
        }
    }

    fn restore_then_retire(&self) {
        let restored =
            prepare_material(&self.root, &self.original.path, RATE, 2, &|| false).unwrap();
        assert!(restored.lease.integrity.warm);
        assert_eq!(restored.lease.original_path, self.original.path);
        assert_eq!(restored.lease.cache_path, self.cache);
        assert_eq!(restored.sample.frame_count(), self.frames);
        assert_eq!(restored.sample.channels, self.channels);
        assert_eq!(restored.sample.samples.as_ref(), self.accepted_pcm);
        let restored_pcm = Arc::downgrade(&restored.sample.samples);
        let mut h = Harness::new();
        let auxiliary_root = h.root.clone();
        let auxiliary_original = h.material.lease.original_path.clone();
        let auxiliary_cache = h.material.lease.cache_path.clone();
        let auxiliary_pcm = [
            Arc::downgrade(&h.material.sample.samples),
            Arc::downgrade(
                &h.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .samples,
            ),
            Arc::downgrade(
                &h.callback.mixer.bank_for_measurement()[NUM_SAMPLES - 1]
                    .as_ref()
                    .unwrap()
                    .samples,
            ),
        ];
        let preparation = control::prepared_for_test(&h.engine, restored, &self.root).unwrap();
        let request = control::adopt_for_format(
            &h.engine,
            0,
            &preparation,
            h.producer.clone(),
            (2, RATE, self.root.clone()),
        )
        .unwrap();
        wait_source_enqueue(&mut h, &request);
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        wait_source_ack(&mut h, 0, request.request_id());
        assert!(request.is_current().unwrap());
        let admitted = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
        assert_eq!(admitted.frame_count(), self.frames);
        assert_eq!(admitted.channels, self.channels);
        assert_eq!(admitted.samples.as_ref(), self.accepted_pcm);
        let admitted_pcm = Arc::downgrade(&admitted.samples);
        self.original.verify();
        for file in &self.cache_files {
            file.verify();
        }
        preparation.release_preparation().unwrap();
        drop((preparation, request));
        crate::audio_engine::cold_load::unload_for_producer(&h.engine, 0, &h.producer).unwrap();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert!(h.callback.mixer.bank_for_measurement()[0].is_none());
        reconcile(&h.engine).unwrap();
        crate::audio_engine::cold_load::unload_for_producer(
            &h.engine,
            NUM_SAMPLES - 1,
            &h.producer,
        )
        .unwrap();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert!(h.callback.mixer.bank_for_measurement()[NUM_SAMPLES - 1].is_none());
        reconcile(&h.engine).unwrap();
        let Harness {
            _temp: auxiliary_temp,
            mut engine,
            callback,
            producer,
            consumer,
            material,
            _original_owner,
            ..
        } = h;
        engine.cold_jobs.shutdown_checked().unwrap();
        assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
        drop((
            callback,
            consumer,
            producer,
            material,
            _original_owner,
            engine,
        ));
        wait_phase("restored own source readers end", 0, || {
            restored_pcm.strong_count() == 0
                && admitted_pcm.strong_count() == 0
                && auxiliary_pcm.iter().all(|pcm| pcm.strong_count() == 0)
        });
        let assets = ProjectAssets::shared();
        assets
            .retire(&self.root, &self.original.path, false)
            .unwrap();
        assets
            .retire(&auxiliary_root, &auxiliary_original, false)
            .unwrap();
        wait_phase(
            "explicit own saved original and cache retirement",
            0,
            || {
                !self.original.path.exists()
                    && !self.cache.exists()
                    && !auxiliary_original.exists()
                    && !auxiliary_cache.exists()
            },
        );
        drop(auxiliary_temp);
    }
}

#[test]
#[ignore = "complete 216-bank saved-restore context integration"]
fn saved_original_restores_after_unrelated_216_bank_retirement() {
    let saved = SavedSource::after_engine_shutdown();
    saved.verify_saved_descriptor();
    // Reuse every original source/window/OwnACK/accounting/pressure assertion,
    // including all three rounds and final actual-reader retirement. The exact
    // original leaf remains independently runnable and unchanged.
    all_216_stopped_pads_ack_real_preload_and_repeat_window_registry_retirement();
    saved.verify_saved_descriptor();
    saved.restore_then_retire();
}
