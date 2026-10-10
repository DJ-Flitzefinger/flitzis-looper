//! Weak immutable stem backing in the existing asset registry. No native authority is shared.
use super::super::stem_pair_descriptor::{StemPairDescriptor, StemPcmManifest};
use super::*;
use crate::messages::{CompleteSourceIdentity, ResidentSourceView, STEM_BUFFER_COUNT};
use std::io::Read;

const MAX_PAIR_METADATA_BYTES: u64 = 256 * 1024;
const MAX_PAIR_PINS: usize = 256;

/// Supplied only after the complete native pair verifier has sealed both areas.
pub(in crate::audio_engine) struct PairPins {
    pub files: Vec<fs::File>,
    pub pcm_path: PathBuf,
    pub descriptor_path: PathBuf,
}

struct PairedStemFiles {
    root: PathBuf,
    paths: [PathBuf; 3],
    identities: [FileIdentity; 3],
    proofs: [Vec<VerifiedFile>; 3],
    source_lease: CommittedColdLease,
}

impl Reader {
    pub(super) fn protects_path(&self, path: &Path) -> bool {
        intersects(&self.path, path)
            || self
                .shared_stems
                .as_ref()
                .and_then(|shared| shared.pair.as_ref())
                .is_some_and(|pair| pair.protects(path))
    }

    pub(super) fn assigned_by(&self, path: &Path) -> bool {
        intersects(&self.path, path)
            || self
                .shared_stems
                .as_ref()
                .and_then(|shared| shared.pair.as_ref())
                .is_some_and(|pair| pair.assigned(path))
    }

    pub(super) fn shared_native_live(&self) -> bool {
        self.shared_stems
            .as_ref()
            .is_some_and(SharedStemReaders::native_live)
    }

    pub(super) fn shared_file_key(&self) -> Option<(PathBuf, [u8; 32], Option<[PathBuf; 3]>)> {
        let shared = self.shared_stems.as_ref()?;
        Some((
            self.path.clone(),
            shared.identity_value,
            shared.pair.as_ref().map(|pair| pair.paths.clone()),
        ))
    }

    pub(super) fn paired_retirements(
        &self,
        path: &Path,
        identity: Option<&FileIdentity>,
    ) -> Option<Vec<(PathBuf, Retirement)>> {
        let pair = self.shared_stems.as_ref()?.pair.as_ref()?;
        let selected = pair
            .paths
            .iter()
            .zip(&pair.identities)
            .position(|(target, captured)| target == path && identity == Some(captured))?;
        let indices = if selected == 0 {
            vec![0, 1, 2]
        } else {
            vec![selected]
        };
        Some(
            indices
                .into_iter()
                .map(|index| {
                    (
                        pair.paths[index].clone(),
                        Retirement {
                            root: pair.root.clone(),
                            recursive: index != 2,
                            identity: Some(pair.identities[index].clone()),
                            files: Some(pair.proofs[index].clone()),
                            pcm: false,
                            protection: None,
                        },
                    )
                })
                .collect(),
        )
    }
}

pub(super) fn related_pair_paths(
    state: &State,
    path: &Path,
    identity: Option<&FileIdentity>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for reader in &state.readers {
        if let Some(pair) = reader
            .shared_stems
            .as_ref()
            .and_then(|shared| shared.pair.as_ref())
            && pair.matches_target(path, identity)
        {
            for area in &pair.paths {
                if !paths.contains(area) {
                    paths.push(area.clone());
                }
            }
        }
    }
    paths
}

impl PairedStemFiles {
    fn protects(&self, path: &Path) -> bool {
        self.paths.iter().any(|area| intersects(area, path))
            || intersects(&self.source_lease.original_path, path)
            || intersects(&self.source_lease.cache_path, path)
    }

    fn assigned(&self, path: &Path) -> bool {
        self.paths.iter().any(|area| intersects(area, path))
    }

    fn matches_target(&self, path: &Path, identity: Option<&FileIdentity>) -> bool {
        self.paths
            .iter()
            .zip(&self.identities)
            .any(|(area, captured)| area == path && Some(captured) == identity)
    }
}

pub(super) struct SharedStemReaders {
    version: String,
    rate: u32,
    source: Option<Arc<CompleteSourceIdentity>>,
    reference: Weak<[f32]>,
    start: usize,
    end: usize,
    identity: Weak<[u8; 32]>,
    identity_value: [u8; 32],
    pcm: Option<[Weak<[f32]>; STEM_BUFFER_COUNT]>,
    pair: Option<PairedStemFiles>,
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
            available_mask: ((1_u16 << crate::messages::STEM_BUFFER_COUNT) - 1) as u8,
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
        let last = shared.stems[3].samples.clone();
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
    fn native_live(&self) -> bool {
        self.identity.strong_count() > 0
            || self
                .pcm
                .as_ref()
                .is_some_and(|pcm| pcm.iter().any(|reader| reader.strong_count() > 0))
    }

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
        let pcm = self.pcm.as_ref()?;
        let mut buffers = Vec::with_capacity(STEM_BUFFER_COUNT);
        for pcm in pcm {
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
            identity_value: *stems.complete_set_identity.as_ref(),
            pcm: Some(std::array::from_fn(|index| {
                Arc::downgrade(&stems.stems[index].samples)
            })),
            pair: None,
            _files: files,
        };
        let pcm = shared
            .pcm
            .as_ref()
            .expect("resident stems supplied")
            .to_vec();
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

    /// Retain complete paired file ownership without creating any publication
    /// permission or inventing resident components for metadata-only demand.
    pub(in crate::audio_engine) fn retain_stem_pair(
        &self,
        path: PathBuf,
        reference: &SampleBuffer,
        version: &str,
        identity: &Arc<[u8; 32]>,
        stems: Option<&PreparedStemSet>,
        mut pins: PairPins,
        source_lease: CommittedColdLease,
    ) -> io::Result<()> {
        if pins.files.len() > MAX_PAIR_PINS {
            return Err(io::Error::other(
                "complete stem pair file pin bound exceeded",
            ));
        }
        source_lease.verify_reference(reference)?;
        let view = reference
            .residency
            .as_ref()
            .ok_or_else(|| io::Error::other("paired source descriptor missing"))?;
        if !reference.valid_residency(view.source.sample_rate_hz, view.source.channels) {
            return Err(io::Error::other("invalid paired source window"));
        }
        if let Some(stems) = stems
            && (!Arc::ptr_eq(&stems.complete_set_identity, identity)
                || stems.sample_rate_hz != view.source.sample_rate_hz
                || stems.channels != reference.channels
                || stems.frame_count != reference.frame_count()
                || stems.stems.iter().any(|stem| !stem.same_window(reference)))
        {
            return Err(io::Error::other(
                "paired component identity/window mismatch",
            ));
        }
        let material = source_lease
            .material_id
            .as_ref()
            .ok_or_else(|| io::Error::other("paired material identity missing"))?;
        let root = source_lease
            .original_path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("paired samples root missing"))?;
        let mut guards = directory_guards(root)?;
        let typed = super::super::material_paths::resolve(root, &path)?;
        let pcm = super::super::material_paths::resolve(root, &pins.pcm_path)?;
        let common = super::super::material_paths::resolve(root, &pins.descriptor_path)?;
        let ready = |path: &Path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(super::super::material_paths::ready_generation)
        };
        if typed.kind
            != (super::super::material_paths::AssetKind::StemDirectory {
                material: Some(material.clone()),
                generation: true,
            })
            || pcm.kind
                != (super::super::material_paths::AssetKind::StemPcmDirectory {
                    material: material.clone(),
                    generation: true,
                })
            || common.kind
                != (super::super::material_paths::AssetKind::StemPairDescriptor {
                    material: material.clone(),
                })
            || !ready(&typed.path)
            || !ready(&pcm.path)
        {
            return Err(io::Error::other(
                "paired paths must name one committed typed material",
            ));
        }
        guards.extend(directory_guards(&typed.path)?);
        guards.extend(directory_guards(&pcm.path)?);
        guards.extend(directory_guards(
            common
                .path
                .parent()
                .ok_or_else(|| io::Error::other("pair descriptor parent missing"))?,
        )?);
        let encoded = read_pair_metadata(&common.path)?;
        let descriptor: StemPairDescriptor =
            serde_json::from_slice(&encoded).map_err(io::Error::other)?;
        descriptor.validate().map_err(io::Error::other)?;
        let source = &descriptor.content.source;
        let hex = |bytes: &[u8; 32]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let original = super::super::material_paths::resolve(root, &source_lease.original_path)?;
        if original.kind
            != (super::super::material_paths::AssetKind::Original {
                material: Some(material.clone()),
            })
        {
            return Err(io::Error::other(
                "paired canonical source is not the verified material original",
            ));
        }
        let original_reference = original
            .path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_str()
            .ok_or_else(|| io::Error::other("paired source reference is not Unicode"))?
            .replace('\\', "/");
        let canonical_version = format!(
            "samples/{original_reference}|sha256-v1:{}",
            hex(&view.source.original_sha256)
        );
        // A legacy subscriber may still name its own old original. Immutable
        // pair lineage binds the canonical verified lease; reuse eligibility
        // below separately retains the subscriber's current runtime version.
        if descriptor.content.source_version != canonical_version
            || descriptor.content.material_id != *material
            || descriptor.stem_set_identity != hex(identity.as_ref())
            || super::super::material_paths::resolve(root, Path::new(&descriptor.wav_generation))?
                .path
                != typed.path
            || super::super::material_paths::resolve(root, Path::new(&descriptor.pcm_generation))?
                .path
                != pcm.path
            || source.frame_count != view.source.frame_count as u64
            || source.channels as usize != view.source.channels
            || source.sample_rate_hz != view.source.sample_rate_hz
            || source.source_zero_frame != view.source.source_zero_frame as u64
            || source.original_sha256 != hex(&view.source.original_sha256)
            || source.playback_sha256 != hex(&view.source.playback_sha256)
            || source.mono_sha256 != hex(&view.source.mono_sha256)
            || source.transform_sha256 != hex(&view.source.transform_sha256)
            || Some(source.original_bytes)
                != source_lease.manifest.descriptor["decoder"]["original"]["bytes"].as_u64()
        {
            return Err(io::Error::other(
                "paired descriptor/source/path identity mismatch",
            ));
        }
        let wav_marker = read_pair_metadata(&typed.path.join(".complete.json"))?;
        let pcm_marker = read_pair_metadata(&pcm.path.join("manifest.json"))?;
        let manifest: StemPcmManifest =
            serde_json::from_slice(&pcm_marker).map_err(io::Error::other)?;
        manifest.validate().map_err(io::Error::other)?;
        if manifest.content != descriptor.content
            || manifest.stem_set_identity != descriptor.stem_set_identity
            || manifest.wav_generation != descriptor.wav_generation
            || format!("{:x}", Sha256::digest(&pcm_marker)) != descriptor.pcm_manifest_sha256
            || format!("{:x}", Sha256::digest(&wav_marker)) != descriptor.wav_manifest_sha256
        {
            return Err(io::Error::other("paired complete marker binding mismatch"));
        }
        let mut wav_proof = Vec::with_capacity(6);
        let mut pcm_proof = Vec::with_capacity(6);
        for artifact in &descriptor.content.artifacts {
            wav_proof.push(pair_leaf_proof(
                &typed.path,
                &format!("{}.wav", artifact.name),
                artifact.wav_bytes,
                &artifact.wav_sha256,
                &pins.files,
            )?);
            pcm_proof.push(pair_leaf_proof(
                &pcm.path,
                &format!("{}.f32le", artifact.name),
                artifact.pcm_bytes,
                &artifact.pcm_sha256,
                &pins.files,
            )?);
        }
        wav_proof.push(pair_leaf_proof(
            &typed.path,
            ".complete.json",
            wav_marker.len() as u64,
            &descriptor.wav_manifest_sha256,
            &pins.files,
        )?);
        pcm_proof.push(pair_leaf_proof(
            &pcm.path,
            "manifest.json",
            pcm_marker.len() as u64,
            &descriptor.pcm_manifest_sha256,
            &pins.files,
        )?);
        check_pair_children(&typed.path, &wav_proof)?;
        check_pair_children(&pcm.path, &pcm_proof)?;
        let common_name = common
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("common leaf name invalid"))?;
        let common_proof = pair_leaf_proof(
            common.path.parent().unwrap(),
            common_name,
            encoded.len() as u64,
            &format!("{:x}", Sha256::digest(&encoded)),
            &pins.files,
        )?;
        let identities = [
            capture_identity(&typed.path)?,
            capture_identity(&pcm.path)?,
            capture_identity(&common.path)?,
        ]
        .map(|identity| identity.ok_or_else(|| io::Error::other("paired object disappeared")));
        let [wav_identity, pcm_identity, common_identity] = identities;
        let pair = PairedStemFiles {
            root: root.to_owned(),
            paths: [typed.path.clone(), pcm.path, common.path],
            identities: [wav_identity?, pcm_identity?, common_identity?],
            proofs: [wav_proof, pcm_proof, vec![common_proof]],
            source_lease,
        };
        pins.files.append(&mut guards);
        let shared = SharedStemReaders {
            version: version.to_owned(),
            rate: view.source.sample_rate_hz,
            source: Some(view.source.clone()),
            reference: Arc::downgrade(&reference.samples),
            start: reference.resident_start(),
            end: reference.resident_end(),
            identity: Arc::downgrade(identity),
            identity_value: *identity.as_ref(),
            pcm: stems.map(|stems| {
                std::array::from_fn(|index| Arc::downgrade(&stems.stems[index].samples))
            }),
            pair: Some(pair),
            _files: pins.files,
        };
        let pcm = shared
            .pcm
            .as_ref()
            .map_or_else(Vec::new, |pcm| pcm.to_vec());
        let reader = Reader {
            path: typed.path,
            pcm,
            cold: None,
            pending_assignment: false,
            saved_claim: false,
            engine: Weak::new(),
            shared_stems: Some(shared),
        };
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("asset gate poisoned"))?;
        // Replace only a dead same-pair file representative. New handles are
        // already sealed before old handles end; live windows stay independent.
        let replacement = state.readers.iter().position(|existing| {
            existing.pcm.iter().all(|pcm| pcm.strong_count() == 0)
                && existing.shared_stems.as_ref().is_some_and(|old| {
                    !old.native_live()
                        && old.identity_value == *identity.as_ref()
                        && old
                            .pair
                            .as_ref()
                            .zip(
                                reader
                                    .shared_stems
                                    .as_ref()
                                    .and_then(|new| new.pair.as_ref()),
                            )
                            .is_some_and(|(old, new)| {
                                old.paths == new.paths && old.identities == new.identities
                            })
                })
        });
        if let Some(index) = replacement {
            state.readers[index] = reader;
        } else {
            if state.readers.len() >= MAX_READER_RECORDS {
                return Err(io::Error::other("project asset reader registry full"));
            }
            state.readers.push(reader);
        }
        Ok(())
    }
}

fn read_pair_metadata(path: &Path) -> io::Result<Vec<u8>> {
    let mut reader = super::super::cold_store::sealed_reader(path)?;
    let bytes = reader.metadata()?.len();
    if bytes == 0 || bytes > MAX_PAIR_METADATA_BYTES {
        return Err(io::Error::other("pair metadata extent exceeds bound"));
    }
    let mut encoded = vec![0; bytes as usize];
    reader.read_exact(&mut encoded)?;
    if reader.read(&mut [0])? != 0 {
        return Err(io::Error::other("pair metadata EOF mismatch"));
    }
    Ok(encoded)
}

fn pair_leaf_proof(
    directory: &Path,
    name: &str,
    bytes: u64,
    sha256: &str,
    pins: &[fs::File],
) -> io::Result<VerifiedFile> {
    let file = super::super::cold_store::sealed_reader(&directory.join(name))?;
    let identity = file_identity(&file)?;
    if !file.metadata()?.is_file()
        || file.metadata()?.len() != bytes
        || !pins
            .iter()
            .any(|pin| file_identity(pin).ok().as_ref() == Some(&identity))
    {
        return Err(io::Error::other(
            "pair leaf is not the actual verified sealed owner",
        ));
    }
    Ok(VerifiedFile {
        name: name.to_owned(),
        identity,
        bytes,
        sha256: sha256.to_owned(),
    })
}

pub(super) fn check_pair_children(path: &Path, proof: &[VerifiedFile]) -> io::Result<()> {
    let mut count = 0;
    for entry in fs::read_dir(path)?.take(proof.len() + 1) {
        let entry = entry?;
        if !entry.file_type()?.is_file()
            || !proof
                .iter()
                .any(|file| entry.file_name() == file.name.as_str())
        {
            return Err(io::Error::other(
                "unknown paired generation child; preserved",
            ));
        }
        count += 1;
    }
    if count != proof.len() {
        return Err(io::Error::other("incomplete paired generation; preserved"));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod paired_owner_tests {
    // Real original/WAV/PCM/common files and the ordinary native registry. This
    // suite establishes file ownership, not a source or timing callback ACK.
    use super::*;
    use crate::audio_engine::material_migration::{PreparedMigrationMaterial, prepare_material};
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::audio_engine::stem_cache::{STEM_FILE_NAMES, source_version_hash};
    use crate::audio_engine::stem_pair::prepare_complete_pair;
    use serde_json::json;

    const RATE: u32 = 8_000;
    const GENERATION: &str = "0123456789abcdef0123456789abcdef";

    fn wav() -> Vec<u8> {
        let mut pcm = [0_i16; 64];
        pcm[8] = i16::MIN;
        pcm[24] = i16::MAX;
        pcm[40] = 16_384;
        let data_bytes = (pcm.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&RATE.to_le_bytes());
        bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        for value in pcm {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        material: PreparedMigrationMaterial,
        canonical_version: String,
        legacy_version: String,
        wav_reference: String,
        wav_path: PathBuf,
    }

    struct Registered {
        identity: Arc<[u8; 32]>,
        stems: Option<PreparedStemSet>,
        pcm_path: PathBuf,
        descriptor_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("samples");
            fs::create_dir(&root).unwrap();
            let old = root.join("old.wav");
            let encoded = wav();
            fs::write(&old, &encoded).unwrap();
            let material = prepare_material(&root, &old, RATE, 1, &|| false).unwrap();
            let metadata = material.metadata();
            let material_id = metadata["material_id"].as_str().unwrap();
            let digest = metadata["original"]["sha256"].as_str().unwrap();
            let canonical_version = format!(
                "{}|sha256-v1:{digest}",
                metadata["new_reference"].as_str().unwrap()
            );
            let legacy_version = format!("samples/old.wav|sha256-v1:{digest}");
            let wav_reference =
                format!("samples/materials/M{material_id}/stems/.ready-{GENERATION}");
            let wav_path = root.parent().unwrap().join(&wav_reference);
            fs::create_dir_all(&wav_path).unwrap();
            let mut hashes = serde_json::Map::new();
            for name in STEM_FILE_NAMES {
                fs::write(wav_path.join(format!("{name}.wav")), &encoded).unwrap();
                hashes.insert(
                    name.to_owned(),
                    json!(format!("{:x}", Sha256::digest(&encoded))),
                );
            }
            fs::write(wav_path.join(".complete.json"), serde_json::to_vec(&json!({
                "schema":"stem-set-sha256-v1", "source_version":canonical_version, "stems":hashes,
            })).unwrap()).unwrap();
            let (root, wav_path) = owned_path(&root, &wav_path).unwrap();
            Self {
                _temp: temp,
                root,
                material,
                canonical_version,
                legacy_version,
                wav_reference,
                wav_path,
            }
        }

        fn register(
            &self,
            assets: &ProjectAssets,
            resident: bool,
            runtime_version: &str,
        ) -> Registered {
            let pair = prepare_complete_pair(
                &self.root,
                &self.material,
                &self.canonical_version,
                &self.wav_reference,
                &|| false,
            )
            .unwrap();
            let bytes: [u8; 32] = std::array::from_fn(|index| {
                u8::from_str_radix(
                    &pair.descriptor.stem_set_identity[index * 2..index * 2 + 2],
                    16,
                )
                .unwrap()
            });
            let identity = Arc::new(bytes);
            let reference = &self.material.sample;
            let stems = resident.then(|| PreparedStemSet {
                complete_set_identity: identity.clone(),
                accepted_timing: None,
                reference_samples: reference.samples.clone(),
                publication: PreparedSourcePermit::unbound(),
                source_version_hash: source_version_hash(runtime_version),
                sample_rate_hz: RATE,
                channels: 1,
                frame_count: reference.frame_count(),
                available_mask: ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8,
                stems: pair.prepare_component_views(reference).unwrap(),
            });
            let pins = pair.into_pins(self.root.parent().unwrap());
            let pcm_path = super::super::super::material_paths::resolve(&self.root, &pins.pcm_path)
                .unwrap()
                .path;
            let descriptor_path =
                super::super::super::material_paths::resolve(&self.root, &pins.descriptor_path)
                    .unwrap()
                    .path;
            assets
                .retain_stem_pair(
                    self.wav_path.clone(),
                    reference,
                    runtime_version,
                    &identity,
                    stems.as_ref(),
                    pins,
                    self.material.lease.clone(),
                )
                .unwrap();
            Registered {
                identity,
                stems,
                pcm_path,
                descriptor_path,
            }
        }
    }

    fn assert_pair_present(fixture: &Fixture, registered: &Registered) {
        for name in STEM_FILES {
            assert!(fixture.wav_path.join(name).exists());
        }
        for name in STEM_PCM_FILES {
            assert!(registered.pcm_path.join(name).exists());
        }
        assert!(registered.descriptor_path.is_file());
    }

    #[test]
    fn saved_wav_owner_keeps_metadata_only_pair_without_fake_resident_buffers() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, false, &fixture.canonical_version);
        let mut saved = assets.acquire(&fixture.root, &fixture.wav_path).unwrap();
        assert!(
            assets
                .shared_stems(
                    &fixture.wav_path,
                    &fixture.material.sample,
                    &fixture.canonical_version,
                    RATE
                )
                .unwrap()
                .is_none()
        );
        let Registered {
            identity,
            stems,
            pcm_path,
            descriptor_path,
        } = registered;
        assert!(stems.is_none());
        drop(identity);
        assets.collect();
        let state = assets.state.lock().unwrap();
        assert_eq!(state.readers.len(), 1);
        assert!(state.readers[0].pcm.is_empty());
        assert!(
            state.readers[0]
                .shared_stems
                .as_ref()
                .unwrap()
                .pcm
                .is_none()
        );
        drop(state);
        assert!(fs::write(pcm_path.join("instrumental.f32le"), b"changed").is_err());
        assert!(fs::remove_file(&descriptor_path).is_err());
        assets
            .retire(&fixture.root, &fixture.wav_path, true)
            .unwrap();
        assets.collect();
        assert!(fixture.wav_path.exists() && pcm_path.exists() && descriptor_path.exists());
        saved.release();
        assets.collect();
        assert!(!fixture.wav_path.exists() && !pcm_path.exists() && !descriptor_path.exists());
        assert_eq!(assets.status().unwrap().1, 0);
    }

    #[test]
    fn descriptor_job_handle_alone_pins_all_three_targets_until_true_last_drop() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, false, &fixture.canonical_version);
        assets
            .retire(&fixture.root, &fixture.wav_path, true)
            .unwrap();
        assets.collect();
        assert_eq!(assets.state.lock().unwrap().retiring.len(), 3);
        assert_pair_present(&fixture, &registered);
        assert!(fs::remove_file(registered.pcm_path.join("instrumental.f32le")).is_err());
        let pcm = registered.pcm_path.clone();
        let descriptor = registered.descriptor_path.clone();
        drop(registered);
        assets.collect();
        assert!(!fixture.wav_path.exists() && !pcm.exists() && !descriptor.exists());
    }

    #[test]
    fn final_fourth_component_voice_pins_full_five_artifact_pair() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, true, &fixture.canonical_version);
        let last = registered.stems.as_ref().unwrap().stems[3].samples.clone();
        let pcm = registered.pcm_path.clone();
        let descriptor = registered.descriptor_path.clone();
        assets
            .retire(&fixture.root, &fixture.wav_path, true)
            .unwrap();
        drop(registered);
        assets.collect();
        assert!(fixture.wav_path.join("instrumental.wav").exists());
        assert!(pcm.join("instrumental.f32le").exists() && descriptor.exists());
        assert!(fs::write(pcm.join("vocals.f32le"), b"changed").is_err());
        drop(last);
        assets.collect();
        assert!(!fixture.wav_path.exists() && !pcm.exists() && !descriptor.exists());
    }

    #[test]
    fn legacy_subscriber_version_keeps_own_reuse_binding_with_canonical_pair_lineage() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, true, &fixture.legacy_version);
        let reused = assets
            .shared_stems(
                &fixture.wav_path,
                &fixture.material.sample,
                &fixture.legacy_version,
                RATE,
            )
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(
            &reused.complete_set_identity,
            &registered.identity
        ));
        assert!(reused.accepted_timing.is_none());
        assert!(
            assets
                .shared_stems(
                    &fixture.wav_path,
                    &fixture.material.sample,
                    &fixture.canonical_version,
                    RATE
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(
            reused.source_version_hash,
            source_version_hash(&fixture.legacy_version)
        );
        assert_pair_present(&fixture, &registered);
    }

    #[test]
    fn acquiring_or_reclaiming_each_exact_pair_target_cancels_linked_retirement() {
        for selected in 0..3 {
            let fixture = Fixture::new();
            let assets = ProjectAssets::isolated();
            let registered = fixture.register(&assets, false, &fixture.canonical_version);
            let path = [
                &fixture.wav_path,
                &registered.pcm_path,
                &registered.descriptor_path,
            ][selected];
            let mut reservation = assets.acquire_pin(&fixture.root, path).unwrap();
            assets
                .retire(&fixture.root, &fixture.wav_path, true)
                .unwrap();
            assert_eq!(assets.state.lock().unwrap().retiring.len(), 3);
            assets
                .reclaim_stems(reservation.owner.as_ref().unwrap())
                .unwrap();
            assert!(assets.state.lock().unwrap().retiring.is_empty());
            assets
                .retire(&fixture.root, &fixture.wav_path, true)
                .unwrap();
            let mut owner = assets.acquire(&fixture.root, path).unwrap();
            assert!(assets.state.lock().unwrap().retiring.is_empty());
            reservation.release();
            owner.release();
            assets.collect();
            assert_pair_present(&fixture, &registered);
        }
    }

    #[test]
    fn three_target_queue_saturation_fails_before_any_partial_admission() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, false, &fixture.canonical_version);
        {
            let mut state = assets.state.lock().unwrap();
            for index in 0..MAX_RETIREMENTS - 2 {
                state.retiring.insert(
                    fixture.root.join(format!("old-{index}.wav")),
                    Retirement {
                        root: fixture.root.clone(),
                        recursive: false,
                        identity: None,
                        files: None,
                        pcm: false,
                        protection: None,
                    },
                );
            }
        }
        let error = assets
            .retire(&fixture.root, &fixture.wav_path, true)
            .unwrap_err();
        assert!(error.to_string().contains("queue full"));
        {
            let mut state = assets.state.lock().unwrap();
            assert_eq!(state.retiring.len(), MAX_RETIREMENTS - 2);
            for target in [
                &fixture.wav_path,
                &registered.pcm_path,
                &registered.descriptor_path,
            ] {
                assert!(!state.retiring.contains_key(target));
            }
            state.retiring.clear();
        }
        assert_pair_present(&fixture, &registered);
        assets
            .retire(&fixture.root, &fixture.wav_path, true)
            .unwrap();
        assert_eq!(assets.state.lock().unwrap().retiring.len(), 3);
    }

    #[test]
    fn explicit_new_pcm_and_descriptor_rollback_leaves_reused_wav_untouched() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, false, &fixture.canonical_version);
        let before = fs::read(fixture.wav_path.join("instrumental.wav")).unwrap();
        let pcm = registered.pcm_path.clone();
        let descriptor = registered.descriptor_path.clone();
        assets.retire(&fixture.root, &pcm, true).unwrap();
        assets.retire(&fixture.root, &descriptor, false).unwrap();
        assert_eq!(assets.state.lock().unwrap().retiring.len(), 2);
        assert!(
            !assets
                .state
                .lock()
                .unwrap()
                .retiring
                .contains_key(&fixture.wav_path)
        );
        drop(registered);
        assets.collect();
        assert!(!pcm.exists() && !descriptor.exists());
        assert_eq!(
            fs::read(fixture.wav_path.join("instrumental.wav")).unwrap(),
            before
        );
        assert!(fixture.wav_path.join(".complete.json").exists());
    }

    #[test]
    fn unknown_pcm_child_blocks_all_three_targets_without_deleting_any_known_file() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let registered = fixture.register(&assets, false, &fixture.canonical_version);
        let unknown = registered.pcm_path.join("foreign.bin");
        fs::write(&unknown, b"unrelated").unwrap();
        assert!(
            assets
                .retire(&fixture.root, &fixture.wav_path, true)
                .is_err()
        );
        assert!(assets.state.lock().unwrap().retiring.is_empty());
        assert_eq!(fs::read(&unknown).unwrap(), b"unrelated");
        assert_pair_present(&fixture, &registered);
    }

    #[test]
    fn repeated_dead_metadata_handles_keep_one_saved_file_representative() {
        let fixture = Fixture::new();
        let assets = ProjectAssets::isolated();
        let mut owner = assets.acquire(&fixture.root, &fixture.wav_path).unwrap();
        for _ in 0..24 {
            let registered = fixture.register(&assets, false, &fixture.canonical_version);
            drop(registered);
            assets.collect();
            assert_eq!(assets.state.lock().unwrap().readers.len(), 1);
        }
        assets
            .retire(&fixture.root, &fixture.wav_path, true)
            .unwrap();
        owner.release();
        assets.collect();
        assert!(assets.state.lock().unwrap().readers.is_empty());
        assert!(!fixture.wav_path.exists());
    }
}
