//! Weak immutable stem backing in the existing asset registry. No native authority is shared.
use super::*;
use crate::messages::{CompleteSourceIdentity, ResidentSourceView, STEM_BUFFER_COUNT};

pub(super) struct SharedStemReaders {
    version: String,
    rate: u32,
    source: Option<Arc<CompleteSourceIdentity>>,
    reference: Weak<[f32]>,
    start: usize,
    end: usize,
    identity: Weak<[u8; 32]>,
    pcm: [Weak<[f32]>; STEM_BUFFER_COUNT],
    // Deny writes/deletes until every actual queued/bank/voice/job reader ends.
    // The registry drops these handles off-thread, never in a callback payload.
    _files: Vec<fs::File>,
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::messages::ResidentContext;
    use std::sync::atomic::AtomicU64;

    fn sample(revision: u64) -> SampleBuffer {
        let complete = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0; 8]),
            residency: None,
        }
        .with_complete_source(8_000);
        complete
            .window(2, 6, revision, ResidentContext::FiniteLoop)
            .unwrap()
    }

    fn stems(reference: &SampleBuffer) -> PreparedStemSet {
        PreparedStemSet {
            complete_set_identity: Arc::new([7; 32]),
            accepted_timing: None,
            reference_samples: reference.samples.clone(),
            publication: PreparedSourcePermit::new(Arc::new(AtomicU64::new(1)), 1),
            source_version_hash: 1,
            sample_rate_hz: 8_000,
            channels: 1,
            frame_count: 8,
            available_mask: 31,
            stems: std::array::from_fn(|index| SampleBuffer {
                channels: 1,
                samples: Arc::from([index as f32; 4]),
                residency: reference.residency.clone(),
            }),
        }
    }

    #[test]
    fn shared_pcm_is_rebound_to_independent_source_window_and_ack() {
        let assets = ProjectAssets::isolated();
        let first = sample(2);
        let second = sample(9); // different descriptor and PCM owners, equal complete content
        assert!(!first.same_source(&second));
        let prepared = stems(&first);
        assets
            .retain_shared_stems(
                "generation".into(),
                &prepared,
                &first,
                "version",
                Vec::new(),
            )
            .unwrap();
        let mut shared = assets
            .shared_stems(Path::new("generation"), &second, "version", 8_000)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(
            &shared.complete_set_identity,
            &prepared.complete_set_identity
        ));
        for index in 0..STEM_BUFFER_COUNT {
            assert!(Arc::ptr_eq(
                &shared.stems[index].samples,
                &prepared.stems[index].samples
            ));
            assert!(shared.stems[index].same_window(&second));
        }
        assert!(Arc::ptr_eq(&shared.reference_samples, &second.samples));
        assert!(shared.accepted_timing.is_none());
        let own = PreparedSourcePermit::new(Arc::new(AtomicU64::new(4)), 4);
        shared.publication = own.clone();
        prepared.publication.mark_pending().unwrap();
        prepared.publication.mark_accepted();
        assert_eq!(own.status(), "captured");
        own.mark_pending().unwrap();
        own.mark_accepted();
        assert_eq!(prepared.publication.status(), "accepted");
    }

    #[test]
    fn source_version_transform_and_window_mismatch_cannot_reuse_pcm() {
        let assets = ProjectAssets::isolated();
        let reference = sample(2);
        let prepared = stems(&reference);
        assets
            .retain_shared_stems(
                "generation".into(),
                &prepared,
                &reference,
                "version",
                Vec::new(),
            )
            .unwrap();
        assert!(
            assets
                .shared_stems(Path::new("generation"), &reference, "other", 8_000)
                .unwrap()
                .is_none()
        );
        assert!(
            assets
                .shared_stems(Path::new("generation"), &reference, "version", 16_000)
                .unwrap()
                .is_none()
        );
        let mut changed = sample(3);
        let view = Arc::get_mut(changed.residency.as_mut().unwrap()).unwrap();
        Arc::get_mut(&mut view.source).unwrap().transform_sha256 = [3; 32];
        assert!(
            assets
                .shared_stems(Path::new("generation"), &changed, "version", 8_000)
                .unwrap()
                .is_none()
        );
        let shifted = SampleBuffer {
            channels: 1,
            samples: Arc::from([0.0; 8]),
            residency: None,
        }
        .with_complete_source(8_000)
        .window(1, 5, 3, ResidentContext::FiniteLoop)
        .unwrap();
        assert!(
            assets
                .shared_stems(Path::new("generation"), &shifted, "version", 8_000)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn sealed_shared_files_follow_actual_last_component_reader() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let path = root.join("stems/#1/.ready-00000000000000000000000000000001");
        fs::create_dir_all(&path).unwrap();
        for name in STEM_FILES {
            fs::write(path.join(name), b"immutable").unwrap();
        }
        let (_, path) = owned_path(&root, &path).unwrap();
        let assets = ProjectAssets::isolated();
        let reference = sample(1);
        let prepared = stems(&reference);
        assets
            .retain_shared_stems(
                path.clone(),
                &prepared,
                &reference,
                "version",
                seal_stem_artifacts(&path).unwrap(),
            )
            .unwrap();
        let shared = assets
            .shared_stems(&path, &sample(9), "version", 8_000)
            .unwrap()
            .unwrap();
        let mut owner = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, true).unwrap();
        owner.release();
        drop(prepared);
        assets.collect();
        assert!(fs::write(path.join("vocals.wav"), b"changed").is_err());
        // One voice/job component still pins all immutable generation artifacts.
        let last = shared.stems[4].samples.clone();
        drop(shared);
        assets.collect();
        assert!(path.exists());
        assert!(fs::remove_file(path.join("vocals.wav")).is_err());
        drop(last);
        assets.collect();
        assert!(!path.exists());
        assert_eq!(assets.status().unwrap().1, 0);
    }

    #[test]
    fn reserved_reclaim_cancels_retirement_without_admitting_another_owner() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let path = root.join("stems/#1/.ready-00000000000000000000000000000001");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("vocals.wav"), b"old").unwrap();
        let assets = ProjectAssets::isolated();
        let mut owner = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, true).unwrap();
        {
            let mut state = assets.state.lock().unwrap();
            let weak = Arc::downgrade(owner.owner.as_ref().unwrap());
            state.owners.resize(MAX_OWNER_RECORDS, weak);
        }
        assert!(assets.acquire(&root, &path).is_err());
        assets.reclaim_stems(owner.owner.as_ref().unwrap()).unwrap();
        assert_eq!(assets.state.lock().unwrap().owners.len(), MAX_OWNER_RECORDS);
        owner.release();
        assets.collect();
        assert!(
            path.join("vocals.wav").exists(),
            "restored persisted generation survives shutdown"
        );
    }

    #[test]
    fn reclaim_rejects_missing_future_and_replaced_generation_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let path = root.join("stems/#1/.ready-00000000000000000000000000000001");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let assets = ProjectAssets::isolated();
        let future = assets.acquire(&root, &path).unwrap();
        assert!(
            assets
                .reclaim_stems(future.owner.as_ref().unwrap())
                .is_err()
        );
        fs::create_dir(&path).unwrap();
        assert!(
            assets
                .reclaim_stems(future.owner.as_ref().unwrap())
                .is_err()
        );
        let mut actual = assets.acquire(&root, &path).unwrap();
        assets.retire(&root, &path, true).unwrap();
        assets
            .reclaim_stems(actual.owner.as_ref().unwrap())
            .unwrap();
        assets
            .reclaim_stems(actual.owner.as_ref().unwrap())
            .unwrap(); // idempotent
        assets.retire(&root, &path, true).unwrap();
        let backup = path.with_file_name("backup");
        fs::rename(&path, &backup).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(
            assets
                .reclaim_stems(actual.owner.as_ref().unwrap())
                .is_err()
        );
        assert!(
            assets
                .state
                .lock()
                .unwrap()
                .retiring
                .contains_key(&actual.owner.as_ref().unwrap().path)
        );
        fs::remove_dir(&path).unwrap();
        fs::rename(&backup, &path).unwrap();
        actual.release();
        Python::initialize();
        Python::attach(|_| {
            assert!(actual.reclaim_stems(path.to_string_lossy().into()).is_err());
            assert!(
                future
                    .reclaim_stems(root.join("wrong.wav").to_string_lossy().into())
                    .is_err()
            );
        });
    }
}

impl SharedStemReaders {
    fn matches(&self, reference: &SampleBuffer, version: &str, rate: u32) -> bool {
        self.version == version
            && self.rate == rate
            && self.start == reference.resident_start()
            && self.end == reference.resident_end()
            && match (&self.source, &reference.residency) {
                (Some(source), Some(view)) => source.as_ref() == view.source.as_ref(),
                (None, None) => self.reference.ptr_eq(&Arc::downgrade(&reference.samples)),
                _ => false,
            }
    }

    fn bind(&self, reference: &SampleBuffer, version: &str, rate: u32) -> Option<PreparedStemSet> {
        if !self.matches(reference, version, rate) {
            return None;
        }
        let identity = self.identity.upgrade()?;
        let mut buffers = Vec::with_capacity(STEM_BUFFER_COUNT);
        for pcm in &self.pcm {
            buffers.push(SampleBuffer {
                channels: reference.channels,
                samples: pcm.upgrade()?,
                residency: reference.residency.as_ref().map(|view| {
                    Arc::new(ResidentSourceView {
                        source: view.source.clone(),
                        start_frame: view.start_frame,
                        window_revision: view.window_revision,
                        context: view.context,
                    })
                }),
            });
        }
        Some(PreparedStemSet {
            complete_set_identity: identity,
            accepted_timing: None,
            reference_samples: reference.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unbound(),
            source_version_hash: crate::audio_engine::stem_cache::source_version_hash(version),
            sample_rate_hz: rate,
            channels: reference.channels,
            frame_count: reference.frame_count(),
            available_mask: ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8,
            stems: buffers.try_into().ok()?,
        })
    }
}

/// Seal before decoding/alignment, so retained PCM binds one immutable artifact generation.
pub(in crate::audio_engine) fn seal_stem_artifacts(path: &Path) -> io::Result<Vec<fs::File>> {
    let mut files = directory_guards(path)?;
    for name in STEM_FILES {
        let leaf = path.join(name);
        if name == ".complete.json" && !leaf.exists() {
            // Typed legacy WAV sets predate the completion marker.
            continue;
        }
        files.push(crate::audio_engine::cold_store::sealed_reader(&leaf)?);
    }
    Ok(files)
}

impl ProjectAssets {
    /// Resolve actual retained PCM; a path alone never grants source/timing/ACK eligibility.
    pub(in crate::audio_engine) fn shared_stems(
        &self,
        path: &Path,
        reference: &SampleBuffer,
        version: &str,
        rate: u32,
    ) -> io::Result<Option<PreparedStemSet>> {
        let state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        Ok(state
            .readers
            .iter()
            .filter(|reader| reader.path == path)
            .find_map(|reader| reader.shared_stems.as_ref()?.bind(reference, version, rate)))
    }

    pub(in crate::audio_engine) fn retain_shared_stems(
        &self,
        path: PathBuf,
        stems: &PreparedStemSet,
        reference: &SampleBuffer,
        version: &str,
        files: Vec<fs::File>,
    ) -> io::Result<()> {
        let shared = SharedStemReaders {
            version: version.to_owned(),
            rate: stems.sample_rate_hz,
            source: reference.residency.as_ref().map(|view| view.source.clone()),
            reference: Arc::downgrade(&reference.samples),
            start: reference.resident_start(),
            end: reference.resident_end(),
            identity: Arc::downgrade(&stems.complete_set_identity),
            pcm: std::array::from_fn(|index| Arc::downgrade(&stems.stems[index].samples)),
            _files: files,
        };
        let pcm = shared.pcm.to_vec();
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        if state.readers.len() >= MAX_READER_RECORDS {
            return Err(io::Error::other("project asset reader registry full"));
        }
        state.readers.push(Reader {
            path,
            pcm,
            cold: None,
            pending_assignment: false,
            saved_claim: false,
            engine: Weak::new(),
            shared_stems: Some(shared),
        });
        Ok(())
    }
}
