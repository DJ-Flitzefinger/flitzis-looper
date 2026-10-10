//! Strict durable metadata for complete, aligned five-artifact stem pairs.
//!
//! Validation here is pure metadata validation. It neither verifies file bytes nor
//! creates source, timing, window, publication or retirement authority. Store readers
//! must verify the actual sealed WAV/PCM artifacts before using these descriptions.

use super::material_paths::{self, AssetKind};
use super::stem_cache::STEM_FILE_NAMES;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPairSource {
    pub(super) frame_count: u64,
    pub(super) channels: u32,
    pub(super) sample_rate_hz: u32,
    pub(super) original_bytes: u64,
    pub(super) original_sha256: String,
    pub(super) playback_sha256: String,
    pub(super) mono_sha256: String,
    pub(super) transform_sha256: String,
    pub(super) source_zero_frame: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPairArtifact {
    pub(super) name: String,
    pub(super) wav_sha256: String,
    pub(super) wav_bytes: u64,
    pub(super) pcm_sha256: String,
    pub(super) pcm_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPairConversion {
    pub(super) schema_version: u32,
    pub(super) policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPairAlignment {
    pub(super) schema_version: u32,
    pub(super) policy: String,
    pub(super) frame_domain: String,
    pub(super) offset_frames: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPairContent {
    pub(super) material_id: String,
    pub(super) source_version: String,
    pub(super) source: StemPairSource,
    pub(super) conversion: StemPairConversion,
    pub(super) alignment: StemPairAlignment,
    pub(super) artifacts: [StemPairArtifact; 5],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPcmManifest {
    pub(super) schema_version: u32,
    pub(super) encoding: String,
    pub(super) content: StemPairContent,
    pub(super) stem_set_identity: String,
    pub(super) wav_generation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StemPairDescriptor {
    pub(super) schema_version: u32,
    pub(super) encoding: String,
    pub(super) content: StemPairContent,
    pub(super) stem_set_identity: String,
    pub(super) wav_generation: String,
    pub(super) pcm_generation: String,
    pub(super) wav_manifest_sha256: String,
    pub(super) pcm_manifest_sha256: String,
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_hash(value: &str, field: &str) -> Result<(), String> {
    if valid_sha256(value) {
        Ok(())
    } else {
        Err(format!(
            "stem pair {field} must be a lowercase SHA-256 digest"
        ))
    }
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    // DTO field order and integer/string representations are fixed. Deserializing
    // first removes input whitespace/key ordering from this canonical encoding.
    serde_json::to_vec(value).map_err(|error| format!("cannot encode stem pair metadata: {error}"))
}

fn validate_generation(reference: &str, material_id: &str, area: &str) -> Result<(), String> {
    let prefix = format!("samples/materials/M{material_id}/{area}/.ready-");
    if reference
        .strip_prefix(&prefix)
        .is_some_and(material_paths::valid_id)
    {
        Ok(())
    } else {
        Err(format!(
            "stem pair generation must name the exact canonical {area} ready directory"
        ))
    }
}

impl StemPairContent {
    pub(super) fn validate(&self) -> Result<(), String> {
        if !material_paths::valid_id(&self.material_id) {
            return Err("stem pair material_id must be 32 lowercase hex characters".into());
        }
        let source = &self.source;
        if source.frame_count == 0
            || source.channels == 0
            || source.sample_rate_hz == 0
            || source.original_bytes == 0
        {
            return Err(
                "stem pair source geometry and original byte length must be positive".into(),
            );
        }
        if source.source_zero_frame != 0 {
            return Err("stem pair requires the complete source origin at frame zero".into());
        }
        for (field, digest) in [
            ("original_sha256", &source.original_sha256),
            ("playback_sha256", &source.playback_sha256),
            ("mono_sha256", &source.mono_sha256),
            ("transform_sha256", &source.transform_sha256),
        ] {
            validate_hash(digest, field)?;
        }
        let (reference, digest) = self
            .source_version
            .rsplit_once("|sha256-v1:")
            .ok_or("stem pair source_version requires a complete original digest")?;
        if digest != source.original_sha256 || reference.contains('\\') {
            return Err(
                "stem pair source_version must bind the normalized original reference and digest"
                    .into(),
            );
        }
        let prefix = format!("samples/materials/M{}/original/", self.material_id);
        if !reference
            .strip_prefix(&prefix)
            .is_some_and(material_paths::filename)
            || !matches!(
                material_paths::classify(Path::new(reference)),
                Ok(AssetKind::Original { material: Some(material) }) if material == self.material_id
            )
        {
            return Err(
                "stem pair source_version must name this material's canonical original".into(),
            );
        }
        // The executed WAV reader only converts PCM16 to f32: MIN becomes -1,
        // other samples divide by MAX. It requires exact loaded rate/channels/
        // complete frame extent and performs no resampling or channel remix.
        if self.conversion.schema_version != 1
            || self.conversion.policy != "pcm16-exact-geometry-v1"
        {
            return Err("unsupported stem pair conversion policy or revision".into());
        }
        if self.alignment.schema_version != 1
            || self.alignment.policy != "shared-onset-v1"
            || self.alignment.frame_domain != "complete-playback-frames"
        {
            return Err("unsupported stem pair alignment policy, revision or frame domain".into());
        }
        // This is a conservative syntax bound at the loaded/output rate, not
        // the original decoder rate or the exact algorithmic search bound.
        // The producer records its executed round(f32 rate * .25)/analysis-len
        // offset. unsigned_abs keeps even i64::MIN safe to validate here.
        let maximum_offset = u64::from(source.sample_rate_hz).div_ceil(4);
        if self.alignment.offset_frames.unsigned_abs() > maximum_offset
            || self.alignment.offset_frames.unsigned_abs() >= source.frame_count
        {
            return Err(
                "stem pair alignment offset exceeds loaded-rate or complete-frame bounds".into(),
            );
        }
        let pcm_bytes = source
            .frame_count
            .checked_mul(u64::from(source.channels))
            .and_then(|samples| samples.checked_mul(4))
            .ok_or("stem pair complete PCM byte extent overflow")?;
        for (artifact, expected_name) in self.artifacts.iter().zip(STEM_FILE_NAMES) {
            if artifact.name != expected_name {
                return Err(
                    "stem pair requires exactly vocals, melody, bass, drums, instrumental in order"
                        .into(),
                );
            }
            validate_hash(&artifact.wav_sha256, "wav_sha256")?;
            validate_hash(&artifact.pcm_sha256, "pcm_sha256")?;
            if artifact.wav_bytes == 0 || artifact.pcm_bytes != pcm_bytes {
                return Err(format!(
                    "stem pair {} byte extent does not match complete geometry",
                    artifact.name
                ));
            }
        }
        Ok(())
    }

    pub(super) fn logical_identity(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!("{:x}", Sha256::digest(encode(self)?)))
    }
}

impl StemPcmManifest {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.encoding != "aligned-stem-pcm-v1" {
            return Err("unsupported stem PCM manifest encoding or revision".into());
        }
        validate_hash(&self.stem_set_identity, "stem_set_identity")?;
        if self.content.logical_identity()? != self.stem_set_identity {
            return Err("stem PCM manifest complete content identity mismatch".into());
        }
        validate_generation(&self.wav_generation, &self.content.material_id, "stems")?;
        Ok(())
    }

    pub(super) fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        encode(self)
    }
}

impl StemPairDescriptor {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.encoding != "aligned-stem-pair-v1" {
            return Err("unsupported stem pair descriptor encoding or revision".into());
        }
        validate_hash(&self.stem_set_identity, "stem_set_identity")?;
        if self.content.logical_identity()? != self.stem_set_identity {
            return Err("stem pair descriptor complete content identity mismatch".into());
        }
        validate_generation(&self.wav_generation, &self.content.material_id, "stems")?;
        validate_generation(
            &self.pcm_generation,
            &self.content.material_id,
            ".pcm-cache/stems/v1",
        )?;
        validate_hash(&self.wav_manifest_sha256, "wav_manifest_sha256")?;
        validate_hash(&self.pcm_manifest_sha256, "pcm_manifest_sha256")?;
        Ok(())
    }

    pub(super) fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        encode(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn content() -> StemPairContent {
        let material_id = "1".repeat(32);
        let original_sha256 = "a".repeat(64);
        StemPairContent {
            source_version: format!(
                "samples/materials/M{material_id}/original/Mélodie 音.wav|sha256-v1:{original_sha256}"
            ),
            material_id,
            source: StemPairSource {
                frame_count: 128,
                channels: 2,
                sample_rate_hz: 48_000,
                original_bytes: 556,
                original_sha256,
                playback_sha256: "b".repeat(64),
                mono_sha256: "c".repeat(64),
                transform_sha256: "d".repeat(64),
                source_zero_frame: 0,
            },
            conversion: StemPairConversion {
                schema_version: 1,
                policy: "pcm16-exact-geometry-v1".into(),
            },
            alignment: StemPairAlignment {
                schema_version: 1,
                policy: "shared-onset-v1".into(),
                frame_domain: "complete-playback-frames".into(),
                offset_frames: 0,
            },
            artifacts: std::array::from_fn(|index| StemPairArtifact {
                name: STEM_FILE_NAMES[index].into(),
                wav_sha256: format!("{index:x}").repeat(64),
                wav_bytes: 556,
                pcm_sha256: format!("{:x}", index + 5).repeat(64),
                pcm_bytes: 1024,
            }),
        }
    }

    fn manifest(content: StemPairContent) -> StemPcmManifest {
        StemPcmManifest {
            schema_version: 1,
            encoding: "aligned-stem-pcm-v1".into(),
            stem_set_identity: content.logical_identity().unwrap(),
            wav_generation: format!(
                "samples/materials/M{}/stems/.ready-{}",
                content.material_id,
                "a".repeat(32)
            ),
            content,
        }
    }

    fn descriptor() -> StemPairDescriptor {
        let content = content();
        StemPairDescriptor {
            schema_version: 1,
            encoding: "aligned-stem-pair-v1".into(),
            stem_set_identity: content.logical_identity().unwrap(),
            wav_generation: format!(
                "samples/materials/M{}/stems/.ready-{}",
                content.material_id,
                "2".repeat(32)
            ),
            pcm_generation: format!(
                "samples/materials/M{}/.pcm-cache/stems/v1/.ready-{}",
                content.material_id,
                "3".repeat(32)
            ),
            wav_manifest_sha256: "e".repeat(64),
            pcm_manifest_sha256: "f".repeat(64),
            content,
        }
    }

    #[test]
    fn complete_five_artifact_descriptors_roundtrip_canonically() {
        let original = descriptor();
        let bytes = original.canonical_bytes().unwrap();
        let reopened: StemPairDescriptor = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reopened, original);
        assert_eq!(reopened.canonical_bytes().unwrap(), bytes);
        let manifest = manifest(original.content);
        let bytes = manifest.canonical_bytes().unwrap();
        let reopened: StemPcmManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reopened, manifest);
        assert_eq!(reopened.canonical_bytes().unwrap(), bytes);
    }

    #[test]
    fn pcm_manifest_requires_exact_canonical_wav_generation_in_same_material() {
        let original = manifest(content());
        let mut missing = serde_json::to_value(&original).unwrap();
        missing.as_object_mut().unwrap().remove("wav_generation");
        assert!(serde_json::from_value::<StemPcmManifest>(missing).is_err());
        for path in [
            "samples/legacy/stems/.ready-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            format!(
                "samples/materials/M{}/stems/.ready-{}",
                "2".repeat(32),
                "a".repeat(32)
            ),
            format!(
                "samples/materials/M{}/stems/.generation-{}",
                original.content.material_id,
                "a".repeat(32)
            ),
            original
                .wav_generation
                .replace("/stems/", "/.pcm-cache/stems/v1/"),
        ] {
            let mut invalid = original.clone();
            invalid.wav_generation = path;
            assert!(invalid.validate().is_err());
        }
        let mut another_ready = original.clone();
        another_ready.wav_generation = original
            .wav_generation
            .replace(&"a".repeat(32), &"b".repeat(32));
        assert!(another_ready.validate().is_ok());
        assert_eq!(another_ready.stem_set_identity, original.stem_set_identity);
        assert_ne!(
            another_ready.canonical_bytes().unwrap(),
            original.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn each_wav_and_pcm_digest_including_instrumental_binds_logical_identity() {
        let original = descriptor();
        for index in 0..5 {
            for wav in [true, false] {
                let mut changed = original.clone();
                if wav {
                    changed.content.artifacts[index].wav_sha256 = "f".repeat(64);
                } else {
                    changed.content.artifacts[index].pcm_sha256 = "e".repeat(64);
                }
                assert_ne!(
                    changed.content.logical_identity().unwrap(),
                    original.stem_set_identity
                );
                assert!(changed.validate().is_err());
                assert!(manifest(changed.content).validate().is_ok());
            }
        }
    }

    #[test]
    fn every_artifact_requires_valid_hashes_and_complete_byte_extents() {
        for index in 0..5 {
            for field in ["wav_sha256", "pcm_sha256", "wav_bytes", "pcm_bytes"] {
                let mut value = serde_json::to_value(content()).unwrap();
                value["artifacts"][index][field] = if field.ends_with("sha256") {
                    json!("F".repeat(64))
                } else {
                    json!(0)
                };
                let parsed: StemPairContent = serde_json::from_value(value).unwrap();
                assert!(parsed.validate().is_err(), "artifact {index} {field}");
            }
            let mut truncated = content();
            truncated.artifacts[index].pcm_bytes -= 4;
            assert!(truncated.validate().is_err());
            let mut extra = content();
            extra.artifacts[index].pcm_bytes += 4;
            assert!(extra.validate().is_err());
        }
    }

    #[test]
    fn five_distinct_artifact_names_have_one_strict_order() {
        for index in 0..5 {
            let mut unknown = content();
            unknown.artifacts[index].name = "unknown".into();
            assert!(unknown.validate().is_err());
            let mut duplicate = content();
            duplicate.artifacts[index].name = STEM_FILE_NAMES[(index + 1) % 5].into();
            assert!(duplicate.validate().is_err());
        }
        let mut reordered = content();
        reordered.artifacts.swap(0, 4);
        assert!(reordered.validate().is_err());
        for count in [4, 6] {
            let mut value = serde_json::to_value(content()).unwrap();
            let artifacts = value["artifacts"].as_array_mut().unwrap();
            if count == 4 {
                artifacts.pop();
            } else {
                artifacts.push(artifacts[0].clone());
            }
            assert!(serde_json::from_value::<StemPairContent>(value).is_err());
        }
    }

    #[test]
    fn complete_source_digests_and_geometry_are_part_of_identity() {
        let original = content();
        let identity = original.logical_identity().unwrap();
        for field in [
            "playback_sha256",
            "mono_sha256",
            "transform_sha256",
            "original_bytes",
            "sample_rate_hz",
        ] {
            let mut value = serde_json::to_value(&original).unwrap();
            value["source"][field] = if field.ends_with("sha256") {
                json!("f".repeat(64))
            } else {
                json!(original.source.sample_rate_hz + 1)
            };
            let changed: StemPairContent = serde_json::from_value(value).unwrap();
            assert_ne!(changed.logical_identity().unwrap(), identity, "{field}");
        }
        let mut different_original = original.clone();
        different_original.source.original_sha256 = "f".repeat(64);
        assert!(different_original.validate().is_err());
        different_original.source_version = different_original
            .source_version
            .replace(&"a".repeat(64), &"f".repeat(64));
        assert_ne!(different_original.logical_identity().unwrap(), identity);
        let mut different_frames = original.clone();
        different_frames.source.frame_count += 1;
        for artifact in &mut different_frames.artifacts {
            artifact.pcm_bytes += 8;
        }
        assert_ne!(different_frames.logical_identity().unwrap(), identity);
    }

    #[test]
    fn geometry_is_positive_checked_and_requires_complete_origin() {
        for field in [
            "frame_count",
            "channels",
            "sample_rate_hz",
            "original_bytes",
        ] {
            let mut value = serde_json::to_value(content()).unwrap();
            value["source"][field] = json!(0);
            let parsed: StemPairContent = serde_json::from_value(value).unwrap();
            assert!(parsed.validate().is_err(), "{field}");
        }
        let mut shifted_origin = content();
        shifted_origin.source.source_zero_frame = 1;
        assert!(shifted_origin.validate().is_err());
        let mut one_frame = content();
        one_frame.source.frame_count = 1;
        for artifact in &mut one_frame.artifacts {
            artifact.pcm_bytes = 8;
        }
        one_frame.validate().unwrap();
        for offset in [-1, 1] {
            one_frame.alignment.offset_frames = offset;
            assert!(one_frame.validate().is_err());
        }
        for channels in [1, u32::MAX] {
            let mut overflow = content();
            overflow.source.frame_count = u64::MAX;
            overflow.source.channels = channels;
            assert!(overflow.logical_identity().is_err());
        }
    }

    #[test]
    fn source_version_requires_same_typed_material_original_and_digest() {
        let original = content();
        let prefix = format!("samples/materials/M{}", original.material_id);
        for reference in [
            "samples/legacy.wav".to_owned(),
            format!("samples/materials/M{}/original/Take.wav", "2".repeat(32)),
            format!("{prefix}/original/../Take.wav"),
            format!("{prefix}/original/./Take.wav"),
            format!("{prefix}/original//Take.wav"),
            format!("{prefix}/original/Take.wav/"),
            format!("{prefix}/original/Take.wav:stream"),
            format!("{prefix}/original/Take.wav."),
            format!("{prefix}/original/CON.wav"),
            format!("{prefix}/stems/vocals.wav"),
            format!("C:/{prefix}/original/Take.wav"),
            format!("/{prefix}/original/Take.wav"),
            format!("{prefix}/original/Take.wav").replace('/', "\\"),
        ] {
            let mut invalid = original.clone();
            invalid.source_version =
                format!("{reference}|sha256-v1:{}", original.source.original_sha256);
            assert!(invalid.validate().is_err(), "{reference}");
        }
        for version in [
            String::new(),
            format!("{prefix}/original/Take.wav"),
            format!("{prefix}/original/Take.wav|sha256-v1:{}", "b".repeat(64)),
        ] {
            let mut invalid = original.clone();
            invalid.source_version = version;
            assert!(invalid.validate().is_err());
        }
        let mut valid_dotfile = original;
        valid_dotfile.source_version = format!(
            "{prefix}/original/.Take.wav|sha256-v1:{}",
            valid_dotfile.source.original_sha256
        );
        valid_dotfile.validate().unwrap();
    }

    #[test]
    fn hashes_and_material_ids_are_exact_lowercase_hex() {
        for invalid in [
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            "é".repeat(32),
        ] {
            let mut candidate = content();
            candidate.source.playback_sha256 = invalid;
            assert!(candidate.validate().is_err());
        }
        for invalid in [
            "a".repeat(31),
            "a".repeat(33),
            "A".repeat(32),
            "g".repeat(32),
            format!("M{}", "a".repeat(32)),
        ] {
            let mut candidate = content();
            candidate.material_id = invalid;
            assert!(candidate.validate().is_err());
        }
        for field in [
            "stem_set_identity",
            "wav_manifest_sha256",
            "pcm_manifest_sha256",
        ] {
            let mut value = serde_json::to_value(descriptor()).unwrap();
            value[field] = json!("F".repeat(64));
            let candidate: StemPairDescriptor = serde_json::from_value(value).unwrap();
            assert!(candidate.validate().is_err(), "{field}");
        }
    }

    #[test]
    fn alignment_offset_is_in_loaded_rate_frames_with_checked_signed_bounds() {
        for rate in [1_u32, 3, 4, 8_000, 44_101, 48_000, u32::MAX] {
            let bound = u64::from(rate).div_ceil(4) as i64;
            for offset in [-bound, 0, bound] {
                let mut candidate = content();
                candidate.source.sample_rate_hz = rate;
                candidate.source.frame_count = bound as u64 + 2;
                for artifact in &mut candidate.artifacts {
                    artifact.pcm_bytes = candidate.source.frame_count * 8;
                }
                candidate.alignment.offset_frames = offset;
                candidate.validate().unwrap();
            }
            for offset in [-bound - 1, bound + 1, i64::MIN, i64::MAX] {
                let mut candidate = content();
                candidate.source.sample_rate_hz = rate;
                candidate.source.frame_count = bound as u64 + 2;
                for artifact in &mut candidate.artifacts {
                    artifact.pcm_bytes = candidate.source.frame_count * 8;
                }
                candidate.alignment.offset_frames = offset;
                assert!(candidate.validate().is_err(), "{rate}: {offset}");
            }
        }
        let mut at_loaded_bound = content();
        at_loaded_bound.source.sample_rate_hz = 8_000;
        at_loaded_bound.source.frame_count = 12_001;
        for artifact in &mut at_loaded_bound.artifacts {
            artifact.pcm_bytes = at_loaded_bound.source.frame_count * 8;
        }
        at_loaded_bound.alignment.offset_frames = 2_000;
        at_loaded_bound.validate().unwrap();
        at_loaded_bound.alignment.offset_frames = 12_000;
        assert!(at_loaded_bound.validate().is_err());
        let mut negative = content();
        negative.alignment.offset_frames = -7;
        let mut positive = negative.clone();
        positive.alignment.offset_frames = 7;
        assert_ne!(
            negative.logical_identity().unwrap(),
            positive.logical_identity().unwrap()
        );
    }

    #[test]
    fn only_executed_conversion_alignment_and_schema_revisions_are_supported() {
        for pointer in ["/conversion/schema_version", "/alignment/schema_version"] {
            for revision in [0, 2] {
                let mut value = serde_json::to_value(content()).unwrap();
                *value.pointer_mut(pointer).unwrap() = json!(revision);
                let candidate: StemPairContent = serde_json::from_value(value).unwrap();
                assert!(candidate.validate().is_err(), "{pointer}: {revision}");
            }
        }
        for (pointer, unsupported) in [
            ("/conversion/policy", "resampled-pcm16-v1"),
            ("/alignment/policy", "independent-stem-onset-v1"),
            ("/alignment/frame_domain", "original-decoder-frames"),
        ] {
            let mut value = serde_json::to_value(content()).unwrap();
            *value.pointer_mut(pointer).unwrap() = json!(unsupported);
            let candidate: StemPairContent = serde_json::from_value(value).unwrap();
            assert!(candidate.validate().is_err(), "{pointer}");
        }
        let mut pair = descriptor();
        pair.schema_version = 2;
        assert!(pair.validate().is_err());
        pair.schema_version = 1;
        pair.encoding = "aligned-stem-pcm-v1".into();
        assert!(pair.validate().is_err());
        let mut pcm = manifest(content());
        pcm.schema_version = 0;
        assert!(pcm.validate().is_err());
        pcm.schema_version = 1;
        pcm.encoding = "aligned-stem-pair-v1".into();
        assert!(pcm.validate().is_err());
    }

    #[test]
    fn generations_require_same_material_exact_ready_names_and_distinct_areas() {
        let original = descriptor();
        for field in ["wav_generation", "pcm_generation"] {
            let reference = if field == "wav_generation" {
                &original.wav_generation
            } else {
                &original.pcm_generation
            };
            for invalid in [
                reference.replace(".ready-", ".generation-"),
                reference.replace(&original.content.material_id, &"a".repeat(32)),
                reference.replace("/materials/", "/materials/../materials/"),
                reference.replace("/materials/", "/materials/./"),
                reference.replace('/', "\\"),
                reference.to_uppercase(),
                format!("{reference}/"),
                format!("{reference}/manifest.json"),
                format!("/{reference}"),
                format!("C:/{reference}"),
                reference[..reference.len() - 1].to_owned(),
            ] {
                let mut value = serde_json::to_value(&original).unwrap();
                value[field] = json!(invalid);
                let candidate: StemPairDescriptor = serde_json::from_value(value).unwrap();
                assert!(candidate.validate().is_err(), "{field}: {reference}");
            }
        }
        let mut swapped = original.clone();
        std::mem::swap(&mut swapped.wav_generation, &mut swapped.pcm_generation);
        assert!(swapped.validate().is_err());
        let mut legacy_pcm = original;
        legacy_pcm.pcm_generation = format!("samples/.pcm-cache/v1/.ready-{}", "3".repeat(32));
        assert!(legacy_pcm.validate().is_err());
    }

    #[test]
    fn physical_generation_names_and_manifest_hashes_do_not_change_logical_identity() {
        let original = descriptor();
        let mut retry = original.clone();
        retry.wav_generation = retry
            .wav_generation
            .replace(&"2".repeat(32), &"a".repeat(32));
        retry.pcm_generation = retry
            .pcm_generation
            .replace(&"3".repeat(32), &"b".repeat(32));
        retry.wav_manifest_sha256 = "c".repeat(64);
        retry.pcm_manifest_sha256 = "d".repeat(64);
        retry.validate().unwrap();
        assert_eq!(retry.stem_set_identity, original.stem_set_identity);
        assert_eq!(
            retry.content.logical_identity().unwrap(),
            original.content.logical_identity().unwrap()
        );
        assert_ne!(
            retry.canonical_bytes().unwrap(),
            original.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn unknown_fields_in_every_dto_reject_runtime_authority_and_unrecognized_metadata() {
        for pointer in [
            "",
            "/content",
            "/content/source",
            "/content/conversion",
            "/content/alignment",
            "/content/artifacts/4",
        ] {
            let mut value = serde_json::to_value(descriptor()).unwrap();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("native_ack".into(), json!(2));
            assert!(
                serde_json::from_value::<StemPairDescriptor>(value).is_err(),
                "{pointer}"
            );
        }
        let mut value = serde_json::to_value(manifest(content())).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("request_id".into(), json!(1));
        assert!(serde_json::from_value::<StemPcmManifest>(value).is_err());
    }

    #[test]
    fn integer_fields_reject_bool_string_float_null_and_unsigned_overflow() {
        for pointer in [
            "/schema_version",
            "/content/source/frame_count",
            "/content/source/channels",
            "/content/source/sample_rate_hz",
            "/content/source/original_bytes",
            "/content/source/source_zero_frame",
            "/content/conversion/schema_version",
            "/content/alignment/schema_version",
            "/content/artifacts/4/wav_bytes",
            "/content/artifacts/4/pcm_bytes",
        ] {
            for invalid in [
                json!(true),
                json!(false),
                json!("1"),
                json!(1.0),
                json!(-1),
                Value::Null,
            ] {
                let mut value = serde_json::to_value(descriptor()).unwrap();
                *value.pointer_mut(pointer).unwrap() = invalid;
                assert!(
                    serde_json::from_value::<StemPairDescriptor>(value).is_err(),
                    "{pointer}"
                );
            }
        }
        for pointer in [
            "/schema_version",
            "/content/source/channels",
            "/content/source/sample_rate_hz",
        ] {
            let mut value = serde_json::to_value(descriptor()).unwrap();
            *value.pointer_mut(pointer).unwrap() = json!(u64::from(u32::MAX) + 1);
            assert!(serde_json::from_value::<StemPairDescriptor>(value).is_err());
        }
        for invalid in [
            json!(true),
            json!("-1"),
            json!(-1.0),
            json!(u64::MAX),
            Value::Null,
        ] {
            let mut value = serde_json::to_value(descriptor()).unwrap();
            value["content"]["alignment"]["offset_frames"] = invalid;
            assert!(serde_json::from_value::<StemPairDescriptor>(value).is_err());
        }
    }

    #[test]
    fn missing_fields_and_wrong_string_types_are_not_inferred() {
        for pointer in [
            "",
            "/content",
            "/content/source",
            "/content/conversion",
            "/content/alignment",
            "/content/artifacts/4",
        ] {
            let original = serde_json::to_value(descriptor()).unwrap();
            let object = original.pointer(pointer).unwrap().as_object().unwrap();
            for field in object.keys() {
                let mut value = original.clone();
                value
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
                assert!(
                    serde_json::from_value::<StemPairDescriptor>(value).is_err(),
                    "{pointer}/{field}"
                );
            }
        }
        for pointer in [
            "/encoding",
            "/content/material_id",
            "/content/source_version",
            "/content/source/playback_sha256",
            "/content/conversion/policy",
            "/content/alignment/frame_domain",
            "/content/artifacts/4/name",
            "/wav_generation",
        ] {
            for invalid in [json!(true), json!(1), json!({}), json!([]), Value::Null] {
                let mut value = serde_json::to_value(descriptor()).unwrap();
                *value.pointer_mut(pointer).unwrap() = invalid;
                assert!(
                    serde_json::from_value::<StemPairDescriptor>(value).is_err(),
                    "{pointer}"
                );
            }
        }
    }
}
