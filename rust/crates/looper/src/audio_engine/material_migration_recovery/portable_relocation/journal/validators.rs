//! Exact durable schemas used solely by the off-realtime Portable recognizer.
use super::*;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use std::fmt;

pub(super) const CONTENT: &[(&str, &str)] = &[
    ("source_path", "sample_paths"),
    ("source_identity", "pad_content"),
    ("duration", "sample_durations"),
    ("analysis", "sample_analysis"),
    ("stem_cache", "stem_cache"),
    ("stem_mix_mode", "pad_stem_mix_mode"),
    ("stem_intent", "pad_stem_intent"),
    ("key_lock", "pad_key_lock"),
    ("manual_bpm", "manual_bpm"),
    ("timing_intent", "pad_timing_intent"),
    ("key_intent", "pad_key_intent"),
    ("gain_db", "pad_gain_db"),
    ("eq_low_db", "pad_eq_low_db"),
    ("eq_mid_db", "pad_eq_mid_db"),
    ("eq_high_db", "pad_eq_high_db"),
    ("loop_start_s", "pad_loop_start_s"),
    ("loop_end_s", "pad_loop_end_s"),
    ("loop_auto", "pad_loop_auto"),
    ("loop_bars", "pad_loop_bars"),
    ("grid_offset_samples", "pad_grid_offset_samples"),
    ("grid_anchor_s", "pad_grid_anchor_s"),
];
const SETTINGS: &[&str] = &[
    "config_revision",
    "material_migrations",
    "key_lock_intent_version",
    "multi_loop",
    "key_lock",
    "bpm_lock",
    "trigger_quantization_enabled",
    "trigger_quantization_step",
    "input_mapping_enabled",
    "stem_separator",
    "preload_stem_loops_on_startup",
    "resident_pcm_budget_mib",
    "demucs_shifts",
    "demucs_overlap",
    "volume",
    "speed",
    "selected_pad",
    "selected_bank",
    "sidebar_left_expanded",
    "sidebar_right_expanded",
];

// serde_json::Value alone overwrites duplicate map keys. Admission must preserve
// ambiguous evidence, including duplicates inside embedded musical/project JSON.
struct Unambiguous(Value);
impl<'de> Deserialize<'de> for Unambiguous {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = Unambiguous;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("unambiguous finite JSON")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(Unambiguous(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(Unambiguous(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(Unambiguous(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| Unambiguous(Value::Number(number)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(Unambiguous(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(Unambiguous(Value::String(value)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Unambiguous(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Unambiguous(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<Unambiguous>()? {
                    values.push(value.0);
                }
                Ok(Unambiguous(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, Unambiguous>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                }
                Ok(Unambiguous(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}

pub(super) fn json(text: &str, limit: usize) -> io::Result<Value> {
    if text.is_empty() || text.len() > limit {
        return Err(invalid(
            "portable journal embedded JSON exceeds bound; preserved",
        ));
    }
    let value = serde_json::from_str::<Unambiguous>(text)
        .map_err(io::Error::other)?
        .0;
    if !value.is_object() {
        return Err(invalid(
            "portable journal JSON must be an object; preserved",
        ));
    }
    Ok(value)
}
pub(super) fn id(value: &Value) -> bool {
    value.as_str().is_some_and(material_paths::valid_id)
}
pub(super) fn optional_id(value: &Value) -> bool {
    value.is_null() || id(value)
}
pub(super) fn optional_digest(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(digest_valid)
}
fn text(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.is_empty())
}
fn optional_text(value: &Value) -> bool {
    value.is_null() || value.is_string()
}
fn boolean(value: &Value) -> bool {
    value.as_bool().is_some()
}
fn number(value: &Value, minimum: f64, maximum: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|value| value.is_finite() && minimum <= value && value <= maximum)
}
fn optional_number(value: &Value, minimum: f64) -> bool {
    value.is_null() || number(value, minimum, f64::MAX)
}
fn unsigned(value: &Value, minimum: u64, maximum: u64) -> bool {
    value
        .as_u64()
        .is_some_and(|value| minimum <= value && value <= maximum)
}

pub(super) fn slots(value: &Value) -> io::Result<(usize, usize)> {
    let source = value["source_id"].as_u64().filter(|id| *id < 216);
    let target = value["target_id"].as_u64().filter(|id| *id < 216);
    match (source, target) {
        (Some(source), Some(target)) if source != target => Ok((source as usize, target as usize)),
        _ => Err(invalid(
            "portable arrangement requires distinct216-range slots",
        )),
    }
}
pub(super) fn seal(value: &Value) -> io::Result<Value> {
    exact_keys(value, &["schema_version", "payload_json"])?;
    if value["schema_version"].as_u64() != Some(1) {
        return Err(invalid("portable musical seal version invalid"));
    }
    json(
        value["payload_json"]
            .as_str()
            .ok_or_else(|| invalid("portable musical seal payload missing"))?,
        8 * 1024 * 1024,
    )
}

fn identity(value: &Value) -> io::Result<()> {
    exact_keys(
        value,
        &[
            "instance_id",
            "material_id",
            "material_creation",
            "copied_from",
        ],
    )?;
    if !id(&value["instance_id"])
        || !optional_id(&value["material_id"])
        || !optional_id(&value["copied_from"])
        || value["copied_from"] == value["instance_id"]
    {
        return Err(invalid("portable durable content identity invalid"));
    }
    if !value["material_creation"].is_null() {
        birth(&value["material_creation"])?;
    }
    Ok(())
}
fn birth(value: &Value) -> io::Result<()> {
    let receipt: crate::audio_engine::cold_store::MaterialCreationReceipt =
        serde_json::from_value(value.clone()).map_err(io::Error::other)?;
    let root = Path::new(&receipt.root_reference);
    if receipt.schema_version != 1
        || receipt.root_reference.len() > PATH_LIMIT
        || material_paths::classify(&root.join("original.wav")).is_err()
        || root.components().count() != 2
        || receipt.binding.name != ".material.json"
        || receipt.binding.bytes == 0
        || !digest_valid(&receipt.binding.sha256)
    {
        return Err(invalid("portable material birth receipt invalid"));
    }
    Ok(())
}
fn analysis(value: &Value) -> io::Result<()> {
    exact_keys(value, &["bpm", "key", "beat_grid", "accepted_timing"])?;
    exact_keys(&value["beat_grid"], &["beats", "downbeats", "bars"])?;
    if !number(&value["bpm"], -f64::MAX, f64::MAX)
        || !value["key"].is_string()
        || ["beats", "downbeats", "bars"].iter().any(|key| {
            !value["beat_grid"][*key]
                .as_array()
                .is_some_and(|items| items.iter().all(|item| number(item, -f64::MAX, f64::MAX)))
        })
    {
        return Err(invalid("portable analysis metadata invalid"));
    }
    let accepted = &value["accepted_timing"];
    if !accepted.is_null() {
        exact_keys(accepted, &["schema_version", "encoding", "record"])?;
        if accepted["schema_version"].as_u64() != Some(1)
            || accepted["encoding"] != "accepted-constant-timing-qm-raw-v1"
            || !accepted["record"]
                .as_object()
                .is_some_and(|object| !object.is_empty())
        {
            return Err(invalid("portable historical timing encoding invalid"));
        }
    }
    Ok(())
}
fn key_intent(value: &Value) -> io::Result<()> {
    let mut keys = vec![
        "schema_version",
        "source",
        "correction",
        "analysis_epoch",
        "correction_epoch",
        "base_shift",
        "extra_shift",
        "retrigger",
    ];
    if value.get("extra_retrigger").is_some() {
        keys.push("extra_retrigger");
    }
    exact_keys(value, &keys)?;
    let pitch_valid = value["base_shift"]
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .zip(
            value["extra_shift"]
                .as_i64()
                .and_then(|number| i32::try_from(number).ok()),
        )
        .is_some_and(|(base, extra)| {
            crate::audio_engine::pitch_intent::PitchIntent::new(base, extra).is_ok()
        });
    if value["schema_version"].as_u64() != Some(1)
        || !optional_text(&value["correction"])
        || value["analysis_epoch"].as_u64().is_none()
        || value["correction_epoch"].as_u64().is_none()
        || !pitch_valid
        || !boolean(&value["retrigger"])
        || value
            .get("extra_retrigger")
            .is_some_and(|value| !boolean(value))
    {
        return Err(invalid("portable durable key intent invalid"));
    }
    if !value["source"].is_null() {
        exact_keys(&value["source"], &["version", "raw_key"])?;
        if value["source"]["version"].as_u64().is_none() || !value["source"]["raw_key"].is_string()
        {
            return Err(invalid("portable historical key source invalid"));
        }
    }
    Ok(())
}
fn stem_intent(value: &Value) -> io::Result<()> {
    exact_keys(
        value,
        &[
            "schema_version",
            "enabled_mask",
            "last_custom_mask",
            "display_mode",
        ],
    )?;
    let mask = value["enabled_mask"].as_u64();
    if value["schema_version"].as_u64() != Some(1)
        || !unsigned(&value["enabled_mask"], 0, 15)
        || !unsigned(&value["last_custom_mask"], 0, 15)
        || !matches!(
            (value["display_mode"].as_str(), mask),
            (Some("custom"), _) | (Some("all"), Some(15)) | (Some("instrumental"), Some(14))
        )
    {
        return Err(invalid("portable durable component intent invalid"));
    }
    Ok(())
}
fn stem_cache(value: &Value) -> io::Result<()> {
    exact_keys(
        value,
        &[
            "source_version",
            "cache_dir",
            "stems",
            "available",
            "pair",
            "fingerprint_schema",
            "generation_fingerprint",
            "producer_identity",
        ],
    )?;
    exact_keys(
        &value["stems"],
        &["vocals", "melody", "bass", "drums", "instrumental"],
    )?;
    if !text(&value["source_version"])
        || !text(&value["cache_dir"])
        || !boolean(&value["available"])
        || ["vocals", "melody", "bass", "drums", "instrumental"]
            .iter()
            .any(|key| !optional_text(&value["stems"][*key]))
        || !optional_digest(&value["generation_fingerprint"])
        || !optional_digest(&value["producer_identity"])
        || !value["fingerprint_schema"].is_null()
            && (value["fingerprint_schema"] != "effective-stem-v3"
                || value["generation_fingerprint"].is_null()
                || value["producer_identity"].is_null())
    {
        return Err(invalid("portable stem metadata invalid"));
    }
    let pair = &value["pair"];
    if !pair.is_null() {
        let mut keys = vec![
            "schema_version",
            "descriptor_reference",
            "stem_set_identity",
            "wav_generation",
            "pcm_generation",
        ];
        if pair.get("provenance").is_some() {
            keys.push("provenance");
        }
        exact_keys(pair, &keys)?;
        let descriptor = pair["descriptor_reference"]
            .as_str()
            .ok_or_else(|| invalid("portable stem descriptor missing"))?;
        let wav = pair["wav_generation"]
            .as_str()
            .ok_or_else(|| invalid("portable stem WAV generation missing"))?;
        let pcm = pair["pcm_generation"]
            .as_str()
            .ok_or_else(|| invalid("portable stem PCM generation missing"))?;
        let root = descriptor.rsplit_once("/.pcm-cache/").map(|(root, _)| root);
        if pair["schema_version"].as_u64() != Some(1)
            || !pair["stem_set_identity"].as_str().is_some_and(digest_valid)
            || !matches!(
                material_paths::classify(Path::new(descriptor)),
                Ok(material_paths::AssetKind::StemPairDescriptor { .. })
            )
            || !matches!(
                material_paths::classify(Path::new(wav)),
                Ok(material_paths::AssetKind::StemDirectory { .. })
            )
            || !matches!(
                material_paths::classify(Path::new(pcm)),
                Ok(material_paths::AssetKind::StemPcmDirectory { .. })
            )
            || root != wav.rsplit_once("/stems/").map(|(root, _)| root)
            || root != pcm.rsplit_once("/.pcm-cache/").map(|(root, _)| root)
            || material_paths::material_reference_root(Path::new(descriptor)).is_none()
        {
            return Err(invalid("portable complete stem pair invalid"));
        }
        if let Some(provenance) = pair.get("provenance") {
            exact_keys(
                provenance,
                &["schema", "generation_fingerprint", "producer_identity"],
            )?;
            if provenance["schema"] != "effective-stem-v3"
                || !provenance["generation_fingerprint"]
                    .as_str()
                    .is_some_and(digest_valid)
                || !provenance["producer_identity"]
                    .as_str()
                    .is_some_and(digest_valid)
            {
                return Err(invalid("portable stem provenance invalid"));
            }
        }
    }
    Ok(())
}

pub(super) fn slot(value: &Value) -> io::Result<()> {
    let keys: Vec<&str> = CONTENT.iter().map(|(key, _)| *key).collect();
    exact_keys(value, &keys)?;
    if value["source_path"].is_null() != value["source_identity"].is_null()
        || !value["source_path"].is_null() && !text(&value["source_path"])
        || !optional_number(&value["duration"], 0.0)
        || !["full_mix", "all_stems"].contains(&value["stem_mix_mode"].as_str().unwrap_or(""))
        || !value["key_lock"].is_null() && !boolean(&value["key_lock"])
        || !value["manual_bpm"].is_null()
            && !value["manual_bpm"]
                .as_f64()
                .is_some_and(|number| number.is_finite() && number > 0.0)
        || !["automatic", "manual", "tap", "legacy"]
            .contains(&value["timing_intent"].as_str().unwrap_or(""))
        || !number(&value["gain_db"], -60.0, 12.0)
        || ["eq_low_db", "eq_mid_db", "eq_high_db"]
            .iter()
            .any(|key| !number(&value[*key], -60.0, 6.0))
        || !number(&value["loop_start_s"], 0.0, f64::MAX)
        || !optional_number(&value["loop_end_s"], 0.0)
        || !boolean(&value["loop_auto"])
        || !number(&value["loop_bars"], 0.5, f64::MAX)
        || !value["loop_bars"]
            .as_f64()
            .is_some_and(|bars| (bars * 2.0 - (bars * 2.0).round()).abs() <= 1e-9)
        || value["grid_offset_samples"].as_i64().is_none()
        || !optional_number(&value["grid_anchor_s"], 0.0)
    {
        return Err(invalid("portable musical21 schema invalid"));
    }
    if !value["source_identity"].is_null() {
        identity(&value["source_identity"])?;
    }
    if !value["analysis"].is_null() {
        analysis(&value["analysis"])?;
    }
    if !value["stem_cache"].is_null() {
        stem_cache(&value["stem_cache"])?;
    }
    key_intent(&value["key_intent"])?;
    stem_intent(&value["stem_intent"])
}

pub(super) fn empty_slot() -> Value {
    json!({"source_path":null,"source_identity":null,"duration":null,"analysis":null,"stem_cache":null,
        "stem_mix_mode":"full_mix","stem_intent":{"schema_version":1,"enabled_mask":15,"last_custom_mask":15,"display_mode":"all"},
        "key_lock":null,"manual_bpm":null,"timing_intent":"legacy",
        "key_intent":{"schema_version":1,"source":null,"correction":null,"analysis_epoch":0,"correction_epoch":0,"base_shift":0,"extra_shift":0,"retrigger":false},
        "gain_db":0.0,"eq_low_db":0.0,"eq_mid_db":0.0,"eq_high_db":0.0,"loop_start_s":0.0,"loop_end_s":null,"loop_auto":false,"loop_bars":8.0,"grid_offset_samples":0,"grid_anchor_s":null})
}
pub(super) fn project_slot(project: &Value, sample: usize) -> io::Result<Value> {
    let mut result = serde_json::Map::new();
    for (name, field) in CONTENT {
        let array = project[*field]
            .as_array()
            .filter(|array| array.len() == 216)
            .ok_or_else(|| invalid("portable project lacks complete216 musical table"))?;
        result.insert((*name).to_owned(), array[sample].clone());
    }
    let mut result = Value::Object(result);
    if result["stem_cache"].is_object() {
        result["stem_cache"]["available"] = json!(false);
    }
    Ok(result)
}
pub(super) fn project(text: &str) -> io::Result<Value> {
    let project = json(text, 8 * 1024 * 1024)?;
    let mut keys: Vec<&str> = CONTENT
        .iter()
        .map(|(_, field)| *field)
        .chain(SETTINGS.iter().copied())
        .collect();
    if project.get("manual_key").is_some() {
        keys.push("manual_key");
    }
    // Current project images persist this policy. Historical Pair/Copy images
    // predate it and retain their established omission semantics.
    if project.get("key_lock_start_behavior").is_some() {
        keys.push("key_lock_start_behavior");
    }
    exact_keys(&project, &keys)?;
    let mut instances = BTreeSet::new();
    for sample in 0..216 {
        let image = project_slot(&project, sample)?;
        slot(&image)?;
        if let Some(instance) = image["source_identity"]["instance_id"].as_str() {
            if !instances.insert(instance.to_owned()) {
                return Err(invalid("portable project has duplicate content identities"));
            }
        }
    }
    if project["config_revision"].as_u64().is_none()
        || project["key_lock_intent_version"].as_u64() != Some(2)
        || [
            "multi_loop",
            "key_lock",
            "bpm_lock",
            "trigger_quantization_enabled",
            "input_mapping_enabled",
            "preload_stem_loops_on_startup",
            "sidebar_left_expanded",
            "sidebar_right_expanded",
        ]
        .iter()
        .any(|key| !boolean(&project[*key]))
        || !["1_64", "1_32", "1_16"]
            .contains(&project["trigger_quantization_step"].as_str().unwrap_or(""))
        || !["demucs:htdemucs", "bs-roformer:musdb18hq"]
            .contains(&project["stem_separator"].as_str().unwrap_or(""))
        || !unsigned(&project["resident_pcm_budget_mib"], 128, 16384)
        || !number(&project["demucs_shifts"], 1.0, 20.0)
        || project["demucs_shifts"].as_u64().is_none()
        || !number(&project["demucs_overlap"], 0.25, 0.95)
        || !number(&project["volume"], 0.0, 1.0)
        || !number(&project["speed"], 0.5, 2.0)
        || !unsigned(&project["selected_pad"], 0, 215)
        || !unsigned(&project["selected_bank"], 0, 5)
        || !project["material_migrations"]
            .as_object()
            .is_some_and(|items| items.len() <= 216)
        || project.get("manual_key").is_some_and(|value| {
            !value
                .as_array()
                .is_some_and(|array| array.len() == 216 && array.iter().all(optional_text))
        })
        || project.get("key_lock_start_behavior").is_some_and(|value| {
            !["first_sound_at_grid", "attack_at_grid"].contains(&value.as_str().unwrap_or(""))
        })
    {
        return Err(invalid("portable complete project settings invalid"));
    }
    for (id, alias) in project["material_migrations"].as_object().unwrap() {
        exact_keys(
            alias,
            &[
                "schema_version",
                "transaction_id",
                "material_id",
                "old_reference",
                "new_reference",
                "original_sha256",
                "original_bytes",
                "decoder_identity",
                "playback_identity",
                "cache_path",
                "old_source_version",
                "new_source_version",
                "resume_of",
                "material_creation",
            ],
        )?;
        if !material_paths::valid_id(id)
            || alias["transaction_id"].as_str() != Some(id.as_str())
            || alias["schema_version"].as_u64() != Some(1)
            || !self::id(&alias["material_id"])
            || !optional_id(&alias["resume_of"])
            || !unsigned(&alias["original_bytes"], 1, u64::MAX)
            || ["original_sha256", "decoder_identity", "playback_identity"]
                .iter()
                .any(|key| !alias[*key].as_str().is_some_and(digest_valid))
            || [
                "old_reference",
                "new_reference",
                "cache_path",
                "old_source_version",
                "new_source_version",
            ]
            .iter()
            .any(|key| !self::text(&alias[*key]))
        {
            return Err(invalid("portable project material alias invalid"));
        }
        if !alias["material_creation"].is_null() {
            birth(&alias["material_creation"])?;
        }
    }
    Ok(project)
}

fn path_key(samples: &Path, record: &Value, path: &str) -> io::Result<String> {
    let reference = Path::new(path);
    if path
        .split(['/', '\\'])
        .any(|part| [".", ".."].contains(&part))
        || reference
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid("portable musical path contains traversal"));
    }
    let historical = Path::new(record["config_reference"].as_str().unwrap());
    let historical_samples = historical
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "samples"));
    let mapped = if reference.is_absolute() {
        historical_samples
            .and_then(|root| reference.strip_prefix(root).ok())
            .map(|relative| samples.join(relative))
            .unwrap_or_else(|| reference.to_owned())
    } else {
        samples
            .parent()
            .ok_or_else(|| invalid("portable Samples parent missing"))?
            .join(reference)
    };
    Ok(reference_key(&mapped))
}
fn normalized_slot(mut image: Value) -> Value {
    if image["analysis"].is_object() {
        image["analysis"]["accepted_timing"] = Value::Null;
        if image["analysis"]
            == json!({"bpm":0.0,"key":"","beat_grid":{"beats":[],"downbeats":[],"bars":[]},"accepted_timing":null})
        {
            image["analysis"] = Value::Null;
        }
    }
    // Schema 1 predates the independently persisted offset preference. Its
    // explicit migration default must compare equal after Python reserializes
    // the same musical intent; a genuinely different preference stays distinct.
    if image["key_intent"].is_object() && image["key_intent"].get("extra_retrigger").is_none() {
        image["key_intent"]["extra_retrigger"] = image["key_intent"]["retrigger"].clone();
    }
    image
}
pub(super) fn slot_matches(
    samples: &Path,
    record: &Value,
    actual: &Value,
    expected: &Value,
) -> io::Result<bool> {
    let mut actual = normalized_slot(actual.clone());
    let expected = normalized_slot(expected.clone());
    if let (Some(left), Some(right)) = (
        actual["source_path"].as_str(),
        expected["source_path"].as_str(),
    ) {
        if path_key(samples, record, left)? != path_key(samples, record, right)? {
            return Ok(false);
        }
        actual["source_path"] = expected["source_path"].clone();
    }
    Ok(actual == expected)
}

pub(super) fn project_intent(samples: &Path, record: &Value) -> io::Result<Value> {
    let mut project = project(record["project_image_json"].as_str().unwrap())?;
    // Comparison-only migration. Raw historical images/seals and their SHA bytes
    // remain untouched; only the previously absent neutral table is supplied here.
    project.as_object_mut().unwrap().remove("manual_key");
    for sample in 0..216 {
        if let Some(path) = project["sample_paths"][sample].as_str() {
            project["sample_paths"][sample] = json!(path_key(samples, record, path)?);
        }
        let normalized = normalized_slot(project_slot(&project, sample)?);
        project["sample_analysis"][sample] = normalized["analysis"].clone();
        project["pad_key_intent"][sample] = normalized["key_intent"].clone();
        if project["sample_paths"][sample].is_null() {
            project["pad_key_lock"][sample] = Value::Null;
            project["pad_timing_intent"][sample] = json!("legacy");
        }
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn complete_project() -> Value {
        let empty = empty_slot();
        let mut project = json!({"config_revision":1,"material_migrations":{},"key_lock_intent_version":2,
            "multi_loop":false,"key_lock":false,"bpm_lock":false,"trigger_quantization_enabled":false,
            "trigger_quantization_step":"1_32","input_mapping_enabled":true,"stem_separator":"demucs:htdemucs",
            "preload_stem_loops_on_startup":false,"resident_pcm_budget_mib":512,"demucs_shifts":1,
            "demucs_overlap":0.5,"volume":1.0,"speed":1.0,"selected_pad":0,"selected_bank":0,
            "sidebar_left_expanded":true,"sidebar_right_expanded":true});
        for (name, field) in CONTENT {
            project[*field] = json!(vec![empty[*name].clone(); 216]);
        }
        project
    }

    #[test]
    fn key_intent_accepts_legacy_and_independent_preferences_but_never_an_expanded_total() {
        let mut slot = empty_slot();
        slot["key_intent"]["retrigger"] = json!(true);
        assert!(key_intent(&slot["key_intent"]).is_ok());
        let legacy = slot.clone();
        slot["key_intent"]["extra_retrigger"] = json!(true);
        assert!(key_intent(&slot["key_intent"]).is_ok());
        assert_eq!(normalized_slot(legacy), normalized_slot(slot.clone()));
        slot["key_intent"]["extra_retrigger"] = json!(false);
        assert!(key_intent(&slot["key_intent"]).is_ok());
        slot["key_intent"]["extra_retrigger"] = json!(1);
        assert!(key_intent(&slot["key_intent"]).is_err());
        slot["key_intent"]["extra_retrigger"] = json!(false);
        slot["key_intent"]["base_shift"] = json!(23);
        assert!(key_intent(&slot["key_intent"]).is_ok());
        slot["key_intent"]["extra_shift"] = json!(18);
        assert!(key_intent(&slot["key_intent"]).is_err());
        slot["key_intent"]["extra_shift"] = json!(0);
        slot["key_intent"]["base_shift"] = json!(-23);
        assert!(key_intent(&slot["key_intent"]).is_ok());
        slot["key_intent"]["extra_shift"] = json!(-1);
        assert!(key_intent(&slot["key_intent"]).is_err());
        slot["key_intent"]["extra_shift"] = json!(0);
        slot["key_intent"]["unexpected"] = json!(false);
        assert!(key_intent(&slot["key_intent"]).is_err());
    }
}
