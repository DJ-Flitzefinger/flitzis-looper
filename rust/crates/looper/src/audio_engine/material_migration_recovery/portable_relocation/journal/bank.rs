//! Strict current Bank evidence; installer recognition creates no runtime authority.
use super::*;

pub(super) const KEYS: &[&str] = &[
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "intent_revision",
    "captured_config_sha256",
    "bank",
    "captured_project_image_json",
    "project_image_json",
    "image_role",
    "phase",
    "committed_revision",
    "committed_config_sha256",
    "error",
];
pub(super) const SEALS: &[&str] = &["bank", "captured_project_image_json"];
const BANK_SLOTS: usize = 36;

fn current_project(project: &Value) -> io::Result<()> {
    if project.get("key_lock_start_behavior").is_none()
        || project["pad_key_intent"].as_array().is_none_or(|rows| {
            rows.iter()
                .any(|row| row["extra_retrigger"].as_bool().is_none())
        })
        || project["stem_cache"].as_array().is_none_or(|rows| {
            rows.iter()
                .any(|row| !row.is_null() && row["available"].as_bool().is_none())
        })
    {
        return Err(invalid(
            "portable Bank requires the complete current project schema; preserved",
        ));
    }
    Ok(())
}

fn slot(image: &Value) -> io::Result<()> {
    validators::slot(image)?;
    if image["key_intent"]["extra_retrigger"].as_bool().is_none()
        || image["stem_cache"].is_object() && image["stem_cache"]["available"] != false
    {
        return Err(invalid(
            "portable Bank musical seal is incomplete or claims availability; preserved",
        ));
    }
    Ok(())
}

fn canonical_empty() -> Value {
    let mut image = validators::empty_slot();
    image["key_intent"]["extra_retrigger"] = json!(false);
    image
}

fn insert_original(ids: &mut BTreeSet<String>, image: &Value) -> io::Result<()> {
    if let Some(id) = image["source_identity"]["instance_id"].as_str()
        && !ids.insert(id.to_owned())
    {
        return Err(invalid(
            "portable Bank repeats an original participant identity; preserved",
        ));
    }
    Ok(())
}

fn apply_rows(project: &Value, slots: &[Value], role: &str) -> Value {
    let mut projected = project.clone();
    for row in slots {
        let id = row["target_id"].as_u64().unwrap() as usize;
        for (name, field) in validators::CONTENT {
            projected[*field][id] = row[role][*name].clone();
        }
        if projected.get("pad_source_beatmap_intent").is_some()
            || row[role].get("source_beatmap_intent").is_some()
        {
            if projected.get("pad_source_beatmap_intent").is_none() {
                projected["pad_source_beatmap_intent"] = json!(vec![Value::Null; 216]);
            }
            projected["pad_source_beatmap_intent"][id] = row[role]
                .get("source_beatmap_intent")
                .cloned()
                .unwrap_or(Value::Null);
        }
    }
    projected
}

fn bank_intent(samples: &Path, record: &Value, slots: &[Value]) -> io::Result<Value> {
    let mut project = validators::project_intent(samples, record)?;
    // The existing save normalization applies to the entire project. Only the
    // target bank additionally excludes transient stem playback eligibility.
    for row in slots {
        let id = row["target_id"].as_u64().unwrap() as usize;
        if project["stem_cache"][id].is_object() {
            project["stem_cache"][id]["available"] = json!(false);
        }
    }
    Ok(project)
}

fn rows_match(
    samples: &Path,
    record: &Value,
    project: &Value,
    slots: &[Value],
    role: &str,
) -> io::Result<bool> {
    let mut projected = record.clone();
    projected["project_image_json"] = json!(apply_rows(project, slots, role).to_string());
    Ok(bank_intent(samples, record, slots)? == bank_intent(samples, &projected, slots)?)
}

pub(super) fn validate(
    samples: &Path,
    record: &Value,
    project: &Value,
    committed: bool,
) -> io::Result<()> {
    current_project(project)?;
    let captured = validators::project(
        record["captured_project_image_json"]
            .as_str()
            .ok_or_else(|| invalid("portable Bank captured project missing; preserved"))?,
    )?;
    current_project(&captured)?;
    let bank = validators::seal(&record["bank"])?;
    exact_keys(&bank, &["source_bank", "target_bank", "slots"])?;
    let source_bank = bank["source_bank"].as_u64().filter(|id| *id < 6);
    let target_bank = bank["target_bank"].as_u64().filter(|id| *id < 6);
    let slots = bank["slots"]
        .as_array()
        .filter(|rows| rows.len() == BANK_SLOTS)
        .ok_or_else(|| invalid("portable Bank requires exactly36 ordered rows; preserved"))?;
    if target_bank.is_none()
        || !bank["source_bank"].is_null() && source_bank.is_none()
        || source_bank == target_bank
        || captured["config_revision"].as_u64() > record["captured_revision"].as_u64()
        || record["phase"] == "captured"
            && record["resume_of"].is_null()
            && record["project_image_json"] != record["captured_project_image_json"]
    {
        return Err(invalid(
            "portable Bank original bank/revision/image binding invalid; preserved",
        ));
    }
    let empty = canonical_empty();
    let mut original_ids = BTreeSet::new();
    let mut after_ids = BTreeSet::new();
    for (offset, row) in slots.iter().enumerate() {
        exact_keys(
            row,
            &[
                "source_id",
                "target_id",
                "source",
                "before",
                "after",
                "resolved_key_lock",
            ],
        )?;
        let target = target_bank.unwrap() as usize * BANK_SLOTS + offset;
        let source = source_bank.map(|bank| bank as usize * BANK_SLOTS + offset);
        if row["target_id"].as_u64() != Some(target as u64)
            || row["source_id"] != source.map_or(Value::Null, |id| json!(id))
            || row["source"].is_null() != source.is_none()
        {
            return Err(invalid(
                "portable Bank changes exact ordered participants; preserved",
            ));
        }
        slot(&row["before"])?;
        slot(&row["after"])?;
        insert_original(&mut original_ids, &row["before"])?;
        if validators::project_slot(&captured, target)? != row["before"] {
            return Err(invalid(
                "portable Bank captured target differs from its original seal; preserved",
            ));
        }
        if let Some(source) = source {
            slot(&row["source"])?;
            insert_original(&mut original_ids, &row["source"])?;
            if validators::project_slot(&captured, source)? != row["source"] {
                return Err(invalid(
                    "portable Bank captured source differs from its original seal; preserved",
                ));
            }
        }
        let occupied = !row["source"].is_null() && !row["source"]["source_identity"].is_null();
        if !occupied {
            if row["after"] != empty || !row["resolved_key_lock"].is_null() {
                return Err(invalid(
                    "portable Bank Empty/Clear must create canonical Empty; preserved",
                ));
            }
            continue;
        }
        let mut expected = row["source"].clone();
        expected["source_identity"] = row["after"]["source_identity"].clone();
        let after = &row["after"]["source_identity"];
        let resolved = if row["source"]["key_lock"].is_null() {
            &captured["key_lock"]
        } else {
            &row["source"]["key_lock"]
        };
        if after.is_null()
            || row["after"] != expected
            || after["material_id"] != row["source"]["source_identity"]["material_id"]
            || after["material_creation"] != row["source"]["source_identity"]["material_creation"]
            || after["copied_from"] != row["source"]["source_identity"]["instance_id"]
            || row["resolved_key_lock"].as_bool().is_none()
            || &row["resolved_key_lock"] != resolved
            || !after_ids.insert(after["instance_id"].as_str().unwrap().to_owned())
            || record["phase"] != "cancelled"
                && row["resolved_key_lock"]
                    != if row["after"]["key_lock"].is_null() {
                        project["key_lock"].clone()
                    } else {
                        row["after"]["key_lock"].clone()
                    }
        {
            return Err(invalid(
                "portable Bank fresh lineage, musical image or inheritance differs; preserved",
            ));
        }
    }
    if !original_ids.is_disjoint(&after_ids)
        || captured["pad_content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["instance_id"]
                    .as_str()
                    .is_some_and(|id| after_ids.contains(id))
            })
        || !committed
            && project["pad_content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| {
                    item["instance_id"]
                        .as_str()
                        .is_some_and(|id| after_ids.contains(id))
                })
        || !rows_match(
            samples,
            record,
            project,
            slots,
            if committed { "after" } else { "before" },
        )?
    {
        return Err(invalid(
            "portable Bank whole image or global fresh identities differ; preserved",
        ));
    }
    if record["phase"] == "captured" {
        for row in slots {
            if let Some(source) = row["source_id"].as_u64()
                && !validators::slot_matches(
                    samples,
                    record,
                    &validators::project_slot(project, source as usize)?,
                    &row["source"],
                )?
            {
                return Err(invalid(
                    "portable Bank captured ordinal changes source witnesses; preserved",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn changed_commit(
    samples: &Path,
    previous: &Value,
    current: &Value,
) -> io::Result<bool> {
    if previous["phase"] == "native_ack_observed" && current["phase"] == "config_committed" {
        let bank = validators::seal(&previous["bank"])?;
        let slots = bank["slots"].as_array().unwrap();
        let before = validators::project(previous["project_image_json"].as_str().unwrap())?;
        let mut after = apply_rows(&before, slots, "after");
        after["config_revision"] = previous["intent_revision"].clone();
        let mut intended = previous.clone();
        intended["project_image_json"] = json!(after.to_string());
        if current["intent_revision"] != previous["intent_revision"]
            || bank_intent(samples, current, slots)? != bank_intent(samples, &intended, slots)?
        {
            return Ok(true);
        }
    }
    if previous["committed_revision"].is_null() {
        return Ok(false);
    }
    if current["phase"] == "native_ack_observed" {
        return Ok(current["intent_revision"].as_u64() <= previous["intent_revision"].as_u64());
    }
    if current["phase"] == "config_committed" {
        return Ok(current["intent_revision"] != previous["intent_revision"]
            || validators::project_intent(samples, current)?
                != validators::project_intent(samples, previous)?);
    }
    Ok([
        "committed_revision",
        "committed_config_sha256",
        "project_image_json",
    ]
    .iter()
    .any(|key| current[*key] != previous[*key]))
}
