//! Device-free complete store oracles. These use real material preparation,
//! PCM16 WAVs, sealed pair reopening and actual shared alignment. They do not
//! establish callback ACK, application selection or the full crash matrix.

use super::material_migration::{PreparedMigrationMaterial, prepare_material};
use super::material_paths;
use super::stem_cache::{STEM_FILE_NAMES, prepare_complete_stems_at_project_root};
use super::stem_pair::{VerifiedStemPair, open_verified_pair, prepare_complete_pair};
use super::stem_pair_descriptor::{StemPairDescriptor, StemPcmManifest};
use crate::messages::{CompleteSourceIdentity, ResidentContext, ResidentSourceView, SampleBuffer};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

fn artifact_receipt(fixture: &Fixture, reference: &str) -> String {
    use super::material_migration_recovery::MigrationArtifactLease;
    pyo3::Python::initialize();
    pyo3::Python::attach(|py| {
        let mut lease = MigrationArtifactLease::capture(
            py,
            fixture.root.to_string_lossy().into(),
            reference.into(),
        )
        .unwrap();
        let receipt = lease.receipt_json().unwrap();
        let mut reopened = MigrationArtifactLease::reopen(
            py,
            fixture.root.to_string_lossy().into(),
            receipt.clone(),
        )
        .unwrap();
        assert_eq!(reopened.receipt_json().unwrap(), receipt);
        reopened.release();
        lease.release();
        receipt
    })
}

#[test]
fn pair_recovery_receipts_reopen_exact_six_pcm_leaves_and_common_without_runtime_rights() {
    let fixture = Fixture::new(5);
    let pair = fixture.prepare().unwrap();
    let descriptor = pair.descriptor.clone();
    let reference = pair.descriptor_reference.clone();
    drop(pair);
    let pcm: serde_json::Value =
        serde_json::from_str(&artifact_receipt(&fixture, &descriptor.pcm_generation)).unwrap();
    let common: serde_json::Value =
        serde_json::from_str(&artifact_receipt(&fixture, &reference)).unwrap();
    assert_eq!(pcm["kind"], "stem_pcm_directory");
    assert_eq!(pcm["files"].as_array().unwrap().len(), 6);
    assert_eq!(common["kind"], "stem_pair_descriptor");
    assert_eq!(common["files"].as_array().unwrap().len(), 1);
    for value in [pcm, common] {
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value.as_object().unwrap().len(), 6);
        for forbidden in ["ack", "request_id", "publication", "source_generation"] {
            assert!(value.get(forbidden).is_none());
        }
    }
}

#[test]
fn reopening_either_pair_receipt_rechecks_instrumental_and_unknown_generation_children() {
    use super::material_migration_recovery::MigrationArtifactLease;
    for corrupt in ["instrumental.f32le", "unknown"] {
        let fixture = Fixture::new(0);
        let pair = fixture.prepare().unwrap();
        let descriptor = pair.descriptor.clone();
        let reference = pair.descriptor_reference.clone();
        drop(pair);
        let receipts = [
            artifact_receipt(&fixture, &descriptor.pcm_generation),
            artifact_receipt(&fixture, &reference),
        ];
        let directory = fixture
            .root
            .parent()
            .unwrap()
            .join(&descriptor.pcm_generation);
        let path = directory.join(corrupt);
        if corrupt == "unknown" {
            fs::write(&path, b"foreign").unwrap();
        } else {
            let mut bytes = fs::read(&path).unwrap();
            bytes[0] ^= 1;
            fs::write(&path, bytes).unwrap();
        }
        pyo3::Python::attach(|py| {
            for receipt in receipts {
                assert!(
                    MigrationArtifactLease::reopen(
                        py,
                        fixture.root.to_string_lossy().into(),
                        receipt,
                    )
                    .is_err()
                );
            }
        });
        assert!(path.is_file());
        assert!(fixture.root.parent().unwrap().join(reference).is_file());
    }
}

#[test]
fn standalone_pcm_recovery_reader_holds_both_areas_through_actual_retirement() {
    use super::material_migration_recovery::{MigrationArtifactLease, MigrationProjectGuard};
    let fixture = Fixture::new(0);
    let pair = fixture.prepare().unwrap();
    let descriptor = pair.descriptor.clone();
    let reference = pair.descriptor_reference.clone();
    drop(pair);
    pyo3::Python::initialize();
    let mut lease = pyo3::Python::attach(|py| {
        MigrationArtifactLease::capture(
            py,
            fixture.root.to_string_lossy().into(),
            descriptor.pcm_generation.clone(),
        )
        .unwrap()
    });
    let paths = [
        (
            fixture
                .root
                .parent()
                .unwrap()
                .join(descriptor.wav_generation),
            true,
        ),
        (
            fixture
                .root
                .parent()
                .unwrap()
                .join(descriptor.pcm_generation),
            true,
        ),
        (fixture.root.parent().unwrap().join(reference), false),
    ];
    let assets = super::project_assets::ProjectAssets::shared();
    pyo3::Python::attach(|py| {
        let config = fixture.root.parent().unwrap().join("project.json");
        fs::write(&config, b"{}").unwrap();
        let mut guard = MigrationProjectGuard::new(
            py,
            fixture.root.to_string_lossy().into(),
            config.to_string_lossy().into(),
        )
        .unwrap();
        let mut inventory = guard.lock_inventory(py).unwrap();
        let mut retirements = paths
            .iter()
            .map(|(path, _)| {
                MigrationArtifactLease::capture(
                    py,
                    fixture.root.to_string_lossy().into(),
                    path.to_string_lossy().into(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        for retirement in &mut retirements {
            retirement.retire(py, &inventory).unwrap();
            retirement.release();
        }
        inventory.release();
        guard.release();
    });
    assets.collect_for_test();
    assert!(paths.iter().all(|(path, _)| path.exists()));
    lease.release();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while paths.iter().any(|(path, _)| path.exists()) {
        assert!(
            std::time::Instant::now() < deadline,
            "pair retirement did not settle"
        );
        assets.collect_for_test();
        thread::yield_now();
    }
}

const RATE: u32 = 8_000;
const FRAMES: usize = 64;
const GENERATION: &str = "0123456789abcdef0123456789abcdef";

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn wav_bytes(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    assert!(samples.len().is_multiple_of(usize::from(channels)));
    let data_bytes = u32::try_from(samples.len() * 2).unwrap();
    let block_align = channels * 2;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn impulses(offset: isize) -> Vec<i16> {
    let mut values = vec![0_i16; FRAMES];
    for (frame, amplitude) in [(8_isize, i16::MIN), (24, i16::MAX), (40, 16_384)] {
        values[usize::try_from(frame + offset).unwrap()] = amplitude;
    }
    values
}

fn expected_pcm() -> Vec<u8> {
    let mut values = vec![0.0_f32; FRAMES];
    values[8] = -1.0;
    values[24] = 1.0;
    values[40] = 16_384.0_f32 / f32::from(i16::MAX);
    values
        .into_iter()
        .flat_map(|value| value.to_bits().to_le_bytes())
        .collect()
}

fn encoded_samples(samples: &SampleBuffer) -> Vec<u8> {
    samples
        .samples
        .iter()
        .flat_map(|value| value.to_bits().to_le_bytes())
        .collect()
}

fn entries(path: &Path) -> Vec<String> {
    let mut result = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    result.sort();
    result
}

pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub root: PathBuf,
    pub material: PreparedMigrationMaterial,
    pub version: String,
    pub wav_reference: String,
    pub wav_path: PathBuf,
}

impl Fixture {
    pub fn new(offset: isize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let original = root.join("old.wav");
        fs::write(&original, wav_bytes(RATE, 1, &impulses(0))).unwrap();
        let material = prepare_material(&root, &original, RATE, 1, &|| false).unwrap();
        let metadata = material.metadata();
        let material_id = metadata["material_id"].as_str().unwrap();
        let version = format!(
            "{}|sha256-v1:{}",
            metadata["new_reference"].as_str().unwrap(),
            metadata["original"]["sha256"].as_str().unwrap()
        );
        let wav_reference = format!("samples/materials/M{material_id}/stems/.ready-{GENERATION}");
        let wav_path = root.parent().unwrap().join(&wav_reference);
        fs::create_dir_all(&wav_path).unwrap();
        for name in STEM_FILE_NAMES {
            fs::write(
                wav_path.join(format!("{name}.wav")),
                wav_bytes(RATE, 1, &impulses(offset)),
            )
            .unwrap();
        }
        let fixture = Self {
            _temp: temp,
            root,
            material,
            version,
            wav_reference,
            wav_path,
        };
        fixture.marker();
        fixture
    }

    fn marker(&self) {
        let stems = STEM_FILE_NAMES
            .map(|name| {
                (
                    name.to_owned(),
                    json!(hash(
                        &fs::read(self.wav_path.join(format!("{name}.wav"))).unwrap()
                    )),
                )
            })
            .into_iter()
            .collect::<serde_json::Map<_, _>>();
        fs::write(
            self.wav_path.join(".complete.json"),
            serde_json::to_vec(&json!({"schema":"stem-set-sha256-v1", "source_version":self.version, "stems":stems})).unwrap(),
        ).unwrap();
    }

    pub fn prepare(&self) -> Result<VerifiedStemPair, String> {
        prepare_complete_pair(
            &self.root,
            &self.material,
            &self.version,
            &self.wav_reference,
            &|| false,
        )
    }

    fn open(&self, reference: &str) -> Result<VerifiedStemPair, String> {
        open_verified_pair(&self.root, reference, &self.material, &|| false)
    }

    pub fn stem_pcm_base(&self) -> PathBuf {
        self.material
            .lease
            .original_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(".pcm-cache/stems/v1")
    }
}

struct Proof {
    descriptor: StemPairDescriptor,
    reference: String,
    project: PathBuf,
    descriptor_path: PathBuf,
    pcm_path: PathBuf,
}

impl Proof {
    fn capture(fixture: &Fixture, pair: VerifiedStemPair) -> Self {
        let proof = Self {
            project: fixture.root.parent().unwrap().to_owned(),
            descriptor_path: fixture
                .root
                .parent()
                .unwrap()
                .join(&pair.descriptor_reference),
            pcm_path: fixture
                .root
                .parent()
                .unwrap()
                .join(&pair.descriptor.pcm_generation),
            reference: pair.descriptor_reference.clone(),
            descriptor: pair.descriptor.clone(),
        };
        // Windows sealed files/ordinary ancestor handles must actually end before
        // a fault mutates any WAV, PCM, manifest or common descriptor leaf.
        drop(pair);
        proof
    }

    fn manifest(&self) -> StemPcmManifest {
        serde_json::from_slice(&fs::read(self.pcm_path.join("manifest.json")).unwrap()).unwrap()
    }

    fn write_manifest(&mut self, manifest: StemPcmManifest) {
        if manifest.stem_set_identity != self.descriptor.stem_set_identity {
            // Keep even producer-derived physical names consistent, so the
            // nonfinite oracle reaches actual finite validation rather than
            // failing only because a content-derived name became stale.
            let generation = &manifest.stem_set_identity[..32];
            let new_pcm = self
                .pcm_path
                .parent()
                .unwrap()
                .join(format!(".ready-{generation}"));
            fs::rename(&self.pcm_path, &new_pcm).unwrap();
            let new_descriptor = self
                .descriptor_path
                .parent()
                .unwrap()
                .join(format!("{generation}.json"));
            fs::rename(&self.descriptor_path, &new_descriptor).unwrap();
            self.descriptor.pcm_generation = new_pcm
                .strip_prefix(&self.project)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            self.reference = new_descriptor
                .strip_prefix(&self.project)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            self.pcm_path = new_pcm;
            self.descriptor_path = new_descriptor;
        }
        let bytes = manifest.canonical_bytes().unwrap();
        self.descriptor.content = manifest.content.clone();
        self.descriptor.stem_set_identity = manifest.stem_set_identity.clone();
        self.descriptor.pcm_manifest_sha256 = hash(&bytes);
        fs::write(self.pcm_path.join("manifest.json"), bytes).unwrap();
        fs::write(
            &self.descriptor_path,
            self.descriptor.canonical_bytes().unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn complete_pair_persists_known_pcm_bits_and_actual_shared_positive_negative_zero_offsets() {
    for offset in [-4_isize, 0, 5] {
        let fixture = Fixture::new(offset);
        let expected = expected_pcm();
        let actual = prepare_complete_stems_at_project_root(
            &fixture.material.sample,
            RATE,
            &fixture.wav_reference,
            fixture.root.parent().unwrap(),
        )
        .unwrap();
        assert_eq!(actual.offset_frames, offset);
        let pair = fixture.prepare().unwrap();
        assert_eq!(
            pair.descriptor.content.alignment.offset_frames,
            offset as i64
        );
        assert_eq!(pair.descriptor.content.source.sample_rate_hz, RATE);
        assert_eq!(pair.descriptor.content.source.source_zero_frame, 0);
        assert_eq!(
            pair.descriptor.content.conversion.policy,
            "pcm16-exact-geometry-v1"
        );
        let pcm_path = fixture
            .root
            .parent()
            .unwrap()
            .join(&pair.descriptor.pcm_generation);
        assert_eq!(entries(&pcm_path).len(), 6);
        for (index, name) in STEM_FILE_NAMES.into_iter().enumerate() {
            assert_eq!(
                encoded_samples(&actual.stems[index]),
                expected,
                "offset {offset}, {name}"
            );
            let bytes = fs::read(pcm_path.join(format!("{name}.f32le"))).unwrap();
            assert_eq!(bytes, expected, "offset {offset}, {name}");
            assert_eq!(
                hash(&bytes),
                pair.descriptor.content.artifacts[index].pcm_sha256
            );
            assert_eq!(
                bytes.len() as u64,
                pair.descriptor.content.artifacts[index].pcm_bytes
            );
        }
        let components = pair
            .prepare_component_views(&fixture.material.sample)
            .unwrap();
        assert_eq!(components.len(), 4);
        for component in components {
            assert_eq!(encoded_samples(&component), expected);
        }
        assert_eq!(
            encoded_samples(&pair.read_instrumental(&fixture.material.sample).unwrap()),
            expected
        );
    }
}

#[test]
fn component_window_reads_four_ranges_and_explicit_instrumental_from_same_complete_pair() {
    let fixture = Fixture::new(0);
    let pair = fixture.prepare().unwrap();
    let source_view = fixture.material.sample.residency.as_ref().unwrap();
    let start = 6;
    let end = 26;
    let current = SampleBuffer {
        channels: 1,
        samples: Arc::from(&fixture.material.sample.samples[start..end]),
        residency: Some(Arc::new(ResidentSourceView {
            source: source_view.source.clone(),
            start_frame: start,
            window_revision: source_view.window_revision + 1,
            context: ResidentContext::FiniteLoop,
        })),
    };
    let expected = expected_pcm()[start * 4..end * 4].to_vec();
    let components = pair.prepare_component_views(&current).unwrap();
    assert_eq!(components.len(), 4);
    for component in components {
        assert_eq!(component.frame_count(), FRAMES);
        assert_eq!(component.resident_start(), start);
        assert_eq!(component.resident_end(), end);
        assert_eq!(
            component.residency.as_ref().unwrap().window_revision,
            source_view.window_revision + 1
        );
        assert_eq!(encoded_samples(&component), expected);
    }
    let instrumental = pair.read_instrumental(&current).unwrap();
    assert_eq!(instrumental.resident_start(), start);
    assert_eq!(instrumental.resident_end(), end);
    assert_eq!(encoded_samples(&instrumental), expected);
}

#[test]
fn prepare_open_and_retry_select_one_generation_with_truthful_creation_flags() {
    let fixture = Fixture::new(0);
    let marker = fs::read(fixture.wav_path.join(".complete.json")).unwrap();
    let first = fixture.prepare().unwrap();
    assert!(first.created_pcm && first.created_descriptor);
    let reference = first.descriptor_reference.clone();
    let descriptor = first.descriptor.clone();
    let before = entries(&fixture.stem_pcm_base());
    drop(first);
    let reopened = fixture.open(&reference).unwrap();
    assert!(!reopened.created_pcm && !reopened.created_descriptor);
    assert_eq!(reopened.descriptor, descriptor);
    drop(reopened);
    let retry = fixture.prepare().unwrap();
    assert!(!retry.created_pcm && !retry.created_descriptor);
    assert_eq!(retry.descriptor_reference, reference);
    assert_eq!(retry.descriptor, descriptor);
    assert_eq!(entries(&fixture.stem_pcm_base()), before);
    assert_eq!(entries(&fixture.stem_pcm_base().join(".pairs")).len(), 1);
    assert_eq!(
        fs::read(fixture.wav_path.join(".complete.json")).unwrap(),
        marker
    );
}

#[test]
fn every_pcm_leaf_including_instrumental_rejects_damage_truncation_missing_and_extra_eof() {
    for name in STEM_FILE_NAMES {
        for fault in ["changed", "truncated", "missing", "extra"] {
            let fixture = Fixture::new(0);
            let proof = Proof::capture(&fixture, fixture.prepare().unwrap());
            let path = proof.pcm_path.join(format!("{name}.f32le"));
            let mut bytes = fs::read(&path).unwrap();
            match fault {
                "changed" => {
                    bytes[0] ^= 1;
                    fs::write(&path, &bytes).unwrap();
                }
                "truncated" => {
                    bytes.pop();
                    fs::write(&path, &bytes).unwrap();
                }
                "missing" => fs::remove_file(&path).unwrap(),
                "extra" => {
                    bytes.extend_from_slice(&0_f32.to_le_bytes());
                    fs::write(&path, &bytes).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(fixture.open(&proof.reference).is_err(), "{name}: {fault}");
            assert!(proof.descriptor_path.exists());
            if fault != "missing" {
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn every_wav_leaf_including_instrumental_rejects_damage_truncation_and_missing() {
    for name in STEM_FILE_NAMES {
        for fault in ["changed", "truncated", "missing"] {
            let fixture = Fixture::new(0);
            let proof = Proof::capture(&fixture, fixture.prepare().unwrap());
            let path = fixture.wav_path.join(format!("{name}.wav"));
            let mut bytes = fs::read(&path).unwrap();
            match fault {
                "changed" => {
                    bytes[44] ^= 1;
                    fs::write(&path, &bytes).unwrap();
                }
                "truncated" => {
                    bytes.pop();
                    fs::write(&path, &bytes).unwrap();
                }
                "missing" => fs::remove_file(&path).unwrap(),
                _ => unreachable!(),
            }
            assert!(fixture.open(&proof.reference).is_err(), "{name}: {fault}");
            assert_eq!(entries(&proof.pcm_path).len(), 6);
            if fault != "missing" {
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn warm_open_rejects_self_consistent_wav_hashes_with_wrong_decoder_geometry() {
    for (index, name) in STEM_FILE_NAMES.into_iter().enumerate() {
        for fault in ["rate", "channels", "frames", "format"] {
            let fixture = Fixture::new(0);
            let mut proof = Proof::capture(&fixture, fixture.prepare().unwrap());
            let path = fixture.wav_path.join(format!("{name}.wav"));
            let mut bytes = fs::read(&path).unwrap();
            match fault {
                "rate" => bytes[24..28].copy_from_slice(&(RATE + 1).to_le_bytes()),
                "channels" => {
                    bytes[22..24].copy_from_slice(&2_u16.to_le_bytes());
                    bytes[32..34].copy_from_slice(&4_u16.to_le_bytes());
                }
                "frames" => {
                    bytes.truncate(bytes.len() - 2);
                    let length = bytes.len() as u32;
                    bytes[4..8].copy_from_slice(&(length - 8).to_le_bytes());
                    bytes[40..44].copy_from_slice(&(length - 44).to_le_bytes());
                }
                "format" => bytes[34..36].copy_from_slice(&32_u16.to_le_bytes()),
                _ => unreachable!(),
            }
            fs::write(&path, &bytes).unwrap();
            fixture.marker();
            let mut manifest = proof.manifest();
            manifest.content.artifacts[index].wav_sha256 = hash(&bytes);
            manifest.content.artifacts[index].wav_bytes = bytes.len() as u64;
            manifest.stem_set_identity = manifest.content.logical_identity().unwrap();
            proof.descriptor.wav_manifest_sha256 =
                hash(&fs::read(fixture.wav_path.join(".complete.json")).unwrap());
            proof.write_manifest(manifest);
            let message = fixture
                .open(&proof.reference)
                .err()
                .expect("warm geometry must reject");
            let expected = match fault {
                "rate" => "sample rate mismatch",
                "channels" => "channel count mismatch",
                "frames" => "frame count mismatch",
                "format" => "only 16-bit",
                _ => unreachable!(),
            };
            assert!(message.contains(expected), "{name}/{fault}: {message}");
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert!(proof.descriptor_path.exists());
        }
    }
}

#[test]
fn self_consistent_hashes_cannot_make_nonfinite_pcm_eligible_in_any_artifact() {
    for (index, name) in STEM_FILE_NAMES.into_iter().enumerate() {
        for nonfinite in [f32::NAN, f32::INFINITY] {
            let fixture = Fixture::new(0);
            let mut proof = Proof::capture(&fixture, fixture.prepare().unwrap());
            let path = proof.pcm_path.join(format!("{name}.f32le"));
            let mut bytes = fs::read(&path).unwrap();
            bytes[..4].copy_from_slice(&nonfinite.to_bits().to_le_bytes());
            fs::write(&path, &bytes).unwrap();
            let mut manifest = proof.manifest();
            manifest.content.artifacts[index].pcm_sha256 = hash(&bytes);
            manifest.stem_set_identity = manifest.content.logical_identity().unwrap();
            proof.write_manifest(manifest);
            let result = fixture.open(&proof.reference);
            assert!(result.is_err(), "{name}: {nonfinite}");
            assert!(
                result.err().unwrap().contains("finite"),
                "{name}: {nonfinite}"
            );
            assert_eq!(
                fs::read(proof.pcm_path.join(format!("{name}.f32le"))).unwrap(),
                bytes
            );
            assert!(proof.descriptor_path.exists());
        }
    }
}

#[test]
fn unknown_children_in_either_ready_area_are_preserved_and_block_eligibility() {
    for wav_area in [true, false] {
        let fixture = Fixture::new(0);
        let proof = Proof::capture(&fixture, fixture.prepare().unwrap());
        let path = if wav_area {
            &fixture.wav_path
        } else {
            &proof.pcm_path
        };
        let unknown = path.join("foreign.keep");
        fs::write(&unknown, b"unrecognized owner data").unwrap();
        let before = entries(path);
        assert!(fixture.open(&proof.reference).is_err());
        assert_eq!(entries(path), before);
        assert_eq!(fs::read(&unknown).unwrap(), b"unrecognized owner data");
        assert!(proof.descriptor_path.exists());
    }
}

#[test]
fn pcm_manifest_direct_wav_generation_binding_and_supported_schema_are_mandatory() {
    for fault in ["other_wav_generation", "schema", "unknown"] {
        let fixture = Fixture::new(0);
        let mut proof = Proof::capture(&fixture, fixture.prepare().unwrap());
        let mut manifest = proof.manifest();
        if fault == "other_wav_generation" {
            manifest.wav_generation = manifest.wav_generation.replace(GENERATION, &"f".repeat(32));
            // Keep metadata and its digest completely valid. The common
            // descriptor must still reject this different physical WAV binding.
            proof.write_manifest(manifest);
        } else {
            let mut value = serde_json::to_value(manifest).unwrap();
            if fault == "schema" {
                value["schema_version"] = json!(2);
            } else {
                value
                    .as_object_mut()
                    .unwrap()
                    .insert("acknowledged".into(), json!(true));
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            proof.descriptor.pcm_manifest_sha256 = hash(&bytes);
            fs::write(proof.pcm_path.join("manifest.json"), bytes).unwrap();
            fs::write(
                &proof.descriptor_path,
                proof.descriptor.canonical_bytes().unwrap(),
            )
            .unwrap();
        }
        assert!(fixture.open(&proof.reference).is_err(), "{fault}");
        assert_eq!(entries(&proof.pcm_path).len(), 6);
        assert_eq!(entries(&fixture.wav_path).len(), 6);
    }
}

#[test]
fn missing_or_incompatible_common_descriptor_never_exposes_a_half_pair() {
    for fault in [
        "missing",
        "unsupported",
        "source_hash",
        "offset",
        "origin",
        "extent",
    ] {
        let fixture = Fixture::new(0);
        let proof = Proof::capture(&fixture, fixture.prepare().unwrap());
        let pcm_before = entries(&proof.pcm_path);
        let wav_before = entries(&fixture.wav_path);
        if fault == "missing" {
            fs::remove_file(&proof.descriptor_path).unwrap();
        } else {
            let mut value = serde_json::to_value(&proof.descriptor).unwrap();
            match fault {
                "unsupported" => value["encoding"] = json!("aligned-stem-pair-v2"),
                "source_hash" => {
                    value["content"]["source"]["playback_sha256"] = json!("f".repeat(64))
                }
                "offset" => value["content"]["alignment"]["offset_frames"] = json!(i64::MIN),
                "origin" => value["content"]["source"]["source_zero_frame"] = json!(1),
                "extent" => value["content"]["artifacts"][4]["pcm_bytes"] = json!(4),
                _ => unreachable!(),
            }
            fs::write(&proof.descriptor_path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        assert!(fixture.open(&proof.reference).is_err(), "{fault}");
        assert_eq!(entries(&proof.pcm_path), pcm_before);
        assert_eq!(entries(&fixture.wav_path), wav_before);
        if fault == "missing" {
            let before = entries(&fixture.stem_pcm_base());
            let recovered = fixture.prepare().unwrap();
            assert!(!recovered.created_pcm);
            assert!(recovered.created_descriptor);
            assert_eq!(
                recovered.descriptor.pcm_generation,
                proof.descriptor.pcm_generation
            );
            assert_eq!(entries(&fixture.stem_pcm_base()), before);
            assert_eq!(entries(&proof.pcm_path), pcm_before);
        }
    }
}

#[test]
fn cancellation_preserves_wavs_unknown_staging_and_existing_selected_pair() {
    let fixture = Fixture::new(0);
    let first = fixture.prepare().unwrap();
    let reference = first.descriptor_reference.clone();
    let descriptor = first.descriptor.clone();
    drop(first);
    let base = fixture.stem_pcm_base();
    let unknown = base.join(format!(".generation-{}", "e".repeat(32)));
    fs::create_dir(&unknown).unwrap();
    fs::write(
        unknown.join("foreign.keep"),
        b"preserve unfinished foreign generation",
    )
    .unwrap();
    let before = entries(&base);
    let marker = fs::read(fixture.wav_path.join(".complete.json")).unwrap();
    assert!(
        prepare_complete_pair(
            &fixture.root,
            &fixture.material,
            &fixture.version,
            &fixture.wav_reference,
            &|| true
        )
        .is_err()
    );
    assert_eq!(entries(&base), before);
    assert_eq!(
        fs::read(unknown.join("foreign.keep")).unwrap(),
        b"preserve unfinished foreign generation"
    );
    assert_eq!(
        fs::read(fixture.wav_path.join(".complete.json")).unwrap(),
        marker
    );
    assert_eq!(fixture.open(&reference).unwrap().descriptor, descriptor);
}

#[test]
fn concurrent_same_material_callers_create_one_complete_pair_and_one_follower() {
    let fixture = Fixture::new(0);
    let barrier = Barrier::new(3);
    let (first, second) = thread::scope(|scope| {
        let run = || {
            barrier.wait();
            fixture.prepare().unwrap()
        };
        let first = scope.spawn(run);
        let second = scope.spawn(run);
        barrier.wait();
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(first.descriptor, second.descriptor);
    assert_eq!(first.descriptor_reference, second.descriptor_reference);
    assert_eq!(
        usize::from(first.created_pcm) + usize::from(second.created_pcm),
        1
    );
    assert_eq!(
        usize::from(first.created_descriptor) + usize::from(second.created_descriptor),
        1
    );
    assert_eq!(entries(&fixture.stem_pcm_base().join(".pairs")).len(), 1);
    assert_eq!(
        entries(&fixture.stem_pcm_base())
            .iter()
            .filter(|name| name.starts_with(".ready-"))
            .count(),
        1
    );
    let a = first
        .prepare_component_views(&fixture.material.sample)
        .unwrap();
    let b = second
        .prepare_component_views(&fixture.material.sample)
        .unwrap();
    assert_eq!(encoded_samples(&a[3]), encoded_samples(&b[3]));
}

#[test]
fn typed_root_material_original_digest_and_ready_reference_cannot_be_substituted() {
    let fixture = Fixture::new(0);
    let pair = fixture.prepare().unwrap();
    let reference = pair.descriptor_reference.clone();
    drop(pair);
    let other = Fixture::new(5);
    assert!(open_verified_pair(&other.root, &reference, &other.material, &|| false).is_err());
    for invalid in [
        reference.replace("/materials/", "/materials/../materials/"),
        reference.replace("/materials/", "/materials/./"),
        fixture.wav_reference.clone(),
        format!("{reference}/manifest.json"),
    ] {
        assert!(fixture.open(&invalid).is_err(), "{invalid}");
    }
    let wrong_digest = fixture.version.replace(
        fixture.material.metadata()["original"]["sha256"]
            .as_str()
            .unwrap(),
        &"f".repeat(64),
    );
    assert!(
        prepare_complete_pair(
            &fixture.root,
            &fixture.material,
            &wrong_digest,
            &fixture.wav_reference,
            &|| false
        )
        .is_err()
    );
    let wrong_generation = fixture.wav_reference.replace(GENERATION, &"e".repeat(32));
    assert!(
        prepare_complete_pair(
            &fixture.root,
            &fixture.material,
            &fixture.version,
            &wrong_generation,
            &|| false
        )
        .is_err()
    );
    assert!(material_paths::resolve(&fixture.root, Path::new(&reference)).is_ok());
    assert!(fixture.open(&reference).is_ok());
}

#[test]
fn component_views_reject_changed_complete_source_and_invalid_empty_window() {
    let fixture = Fixture::new(0);
    let pair = fixture.prepare().unwrap();
    let current = &fixture.material.sample;
    let view = current.residency.as_ref().unwrap();
    for field in [
        "original",
        "playback",
        "mono",
        "transform",
        "origin",
        "rate",
    ] {
        let source = &view.source;
        let mut changed = CompleteSourceIdentity {
            frame_count: source.frame_count,
            channels: source.channels,
            sample_rate_hz: source.sample_rate_hz,
            original_sha256: source.original_sha256,
            playback_sha256: source.playback_sha256,
            mono_sha256: source.mono_sha256,
            transform_sha256: source.transform_sha256,
            source_zero_frame: source.source_zero_frame,
        };
        match field {
            "original" => changed.original_sha256[0] ^= 1,
            "playback" => changed.playback_sha256[0] ^= 1,
            "mono" => changed.mono_sha256[0] ^= 1,
            "transform" => changed.transform_sha256[0] ^= 1,
            "origin" => changed.source_zero_frame = 1,
            "rate" => changed.sample_rate_hz += 1,
            _ => unreachable!(),
        }
        let mut rejected = current.clone();
        let mut changed_view = view.as_ref().clone();
        changed_view.source = Arc::new(changed);
        rejected.residency = Some(Arc::new(changed_view));
        assert!(pair.prepare_component_views(&rejected).is_err(), "{field}");
        assert!(pair.read_instrumental(&rejected).is_err(), "{field}");
    }
    let mut empty = current.clone();
    empty.samples = Arc::from([]);
    assert!(pair.prepare_component_views(&empty).is_err());
    let mut unbound = current.clone();
    unbound.residency = None;
    assert!(pair.prepare_component_views(&unbound).is_err());
    assert!(pair.prepare_component_views(current).is_ok());
}

#[test]
fn exact_wav_loaded_rate_channel_and_complete_frame_geometry_are_required_before_creation() {
    for mismatch in ["rate", "channels", "frames"] {
        let fixture = Fixture::new(0);
        let path = fixture.wav_path.join("instrumental.wav");
        let bytes = match mismatch {
            "rate" => wav_bytes(RATE + 1, 1, &impulses(0)),
            "channels" => wav_bytes(RATE, 2, &impulses(0)),
            "frames" => wav_bytes(RATE, 1, &impulses(0)[..FRAMES - 1]),
            _ => unreachable!(),
        };
        fs::write(&path, &bytes).unwrap();
        fixture.marker();
        assert!(fixture.prepare().is_err(), "{mismatch}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let base = fixture.stem_pcm_base();
        if base.exists() {
            assert!(
                !entries(&base)
                    .iter()
                    .any(|name| name.starts_with(".ready-"))
            );
            if base.join(".pairs").exists() {
                assert!(entries(&base.join(".pairs")).is_empty());
            }
        }
    }
}
