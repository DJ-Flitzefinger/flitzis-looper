//! Bounded installer recognition only. Journal data never supplies runtime authority.
use super::*;

mod bank;
mod validators;

#[cfg(test)]
mod bank_tests;

const PAIR_KEYS: &[&str] = &[
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "intent_revision",
    "captured_config_sha256",
    "before",
    "after",
    "project_image_json",
    "image_role",
    "phase",
    "committed_revision",
    "committed_config_sha256",
    "error",
];
const COPY_KEYS: &[&str] = &[
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "intent_revision",
    "captured_config_sha256",
    "source_id",
    "target_id",
    "source",
    "predecessor",
    "after",
    "resolved_key_lock",
    "captured_project_image_json",
    "project_image_json",
    "image_role",
    "phase",
    "committed_revision",
    "committed_config_sha256",
    "error",
];
const ORIGINAL_COMMON: &[&str] = &[
    "kind",
    "schema_version",
    "transaction_id",
    "resume_of",
    "config_reference",
    "captured_revision",
    "captured_config_sha256",
    "after",
];
const COPY_SEALS: &[&str] = &[
    "source_id",
    "target_id",
    "source",
    "predecessor",
    "after",
    "resolved_key_lock",
    "captured_project_image_json",
];

pub(super) struct History {
    pub(super) config_reference: String,
    kind: Option<String>,
    first: Value,
    last: Value,
}

impl History {
    pub(super) fn validate(samples: &Path, id: &str, encoded: &[String]) -> io::Result<Self> {
        if encoded.is_empty() || encoded.len() > 16 {
            return Err(invalid(
                "portable journal history is empty or exceeds16; preserved",
            ));
        }
        let records: Vec<Value> = encoded
            .iter()
            .map(|text| validators::json(text, RECORD_BYTES as usize))
            .collect::<io::Result<_>>()?;
        let first = &records[0];
        let kind = match first.get("kind") {
            None => None,
            Some(Value::String(kind))
                if ["pad_pair", "pad_copy_overwrite", "pad_bank"].contains(&kind.as_str()) =>
            {
                Some(kind.clone())
            }
            _ => return Err(invalid("unknown portable journal kind; preserved")),
        };
        let config_reference = first["config_reference"]
            .as_str()
            .ok_or_else(|| invalid("journal config binding missing"))?
            .to_owned();
        for (index, record) in records.iter().enumerate() {
            if record["transaction_id"].as_str() != Some(id)
                || record["schema_version"].as_u64() != Some(1)
                || record.get("kind").and_then(Value::as_str) != kind.as_deref()
                || record.get("kind").is_some() != kind.is_some()
                || record["config_reference"].as_str() != Some(config_reference.as_str())
            {
                return Err(invalid(
                    "portable history changes kind, schema, Config or directory binding; preserved",
                ));
            }
            match kind.as_deref() {
                None => legacy(record)?,
                Some(kind) => {
                    arrangement(samples, record, kind)?;
                    if index == 0 && record["phase"] != "captured" {
                        return Err(invalid(
                            "portable arrangement history lacks captured baseline; preserved",
                        ));
                    }
                    if index != 0 {
                        let previous = &records[index - 1];
                        let seals = match kind {
                            "pad_pair" => &["before"][..],
                            "pad_bank" => bank::SEALS,
                            _ => COPY_SEALS,
                        };
                        if ORIGINAL_COMMON
                            .iter()
                            .chain(seals)
                            .any(|key| record[*key] != first[*key])
                            || !next_phase(
                                kind,
                                previous["phase"].as_str().unwrap(),
                                record["phase"].as_str().unwrap(),
                            )
                            || record["intent_revision"].as_u64()
                                < previous["intent_revision"].as_u64()
                            || changed_commit(samples, previous, record)?
                        {
                            return Err(invalid(
                                "portable arrangement history changes original seals or phase/revision; preserved",
                            ));
                        }
                    }
                }
            }
        }
        Ok(Self {
            config_reference,
            kind,
            first: first.clone(),
            last: records.last().unwrap().clone(),
        })
    }
}

fn next_phase(kind: &str, previous: &str, current: &str) -> bool {
    match previous {
        "captured" => ["captured", "publication_pending", "cancelled"].contains(&current),
        "publication_pending" => {
            ["publication_pending", "native_ack_observed", "cancelled"].contains(&current)
        }
        "native_ack_observed" => ["native_ack_observed", "config_committed"].contains(&current),
        "config_committed" => {
            ["config_committed", "settled"].contains(&current)
                || ["pad_copy_overwrite", "pad_bank"].contains(&kind)
                    && current == "native_ack_observed"
        }
        "settled" => current == "settled",
        "cancelled" => current == "cancelled",
        _ => false,
    }
}

fn arrangement(samples: &Path, record: &Value, kind: &str) -> io::Result<()> {
    exact_keys(
        record,
        match kind {
            "pad_pair" => PAIR_KEYS,
            "pad_bank" => bank::KEYS,
            _ => COPY_KEYS,
        },
    )?;
    let captured = record["captured_revision"].as_u64();
    let intent = record["intent_revision"].as_u64();
    let phase = record["phase"].as_str().unwrap_or("");
    if !validators::id(&record["transaction_id"])
        || !validators::optional_id(&record["resume_of"])
        || record["resume_of"] == record["transaction_id"]
        || !record["config_reference"].as_str().is_some_and(|value| {
            !value.is_empty() && value.len() <= PATH_LIMIT && Path::new(value).is_absolute()
        })
        || captured.is_none()
        || intent.is_none()
        || intent < captured
        || !validators::optional_digest(&record["captured_config_sha256"])
        || !record["error"].is_null()
            && !record["error"]
                .as_str()
                .is_some_and(|text| text.chars().count() <= 2048)
        || ![
            "captured",
            "publication_pending",
            "native_ack_observed",
            "config_committed",
            "settled",
            "cancelled",
        ]
        .contains(&phase)
    {
        return Err(invalid(
            "portable arrangement binding, revision or phase invalid; preserved",
        ));
    }
    let text = record["project_image_json"]
        .as_str()
        .ok_or_else(|| invalid("portable project image missing"))?;
    let project = validators::project(text)?;
    let committed = ["config_committed", "settled"].contains(&phase);
    if committed {
        if record["image_role"] != "after"
            || record["committed_revision"].as_u64() != intent
            || project["config_revision"].as_u64() != intent
            || record["committed_config_sha256"].as_str()
                != Some(format!("{:x}", Sha256::digest(text.as_bytes())).as_str())
        {
            return Err(invalid(
                "portable committed image lacks exact written digest/revision; preserved",
            ));
        }
    } else if record["image_role"] != "before"
        || !record["committed_revision"].is_null()
        || !record["committed_config_sha256"].is_null()
        || project["config_revision"].as_u64() > intent
    {
        return Err(invalid(
            "portable uncommitted image claims a receipt or newer revision; preserved",
        ));
    }
    match kind {
        "pad_pair" => pair(samples, record, &project, committed),
        "pad_bank" => bank::validate(samples, record, &project, committed),
        _ => copy(samples, record, &project, committed),
    }
}

fn pair(samples: &Path, record: &Value, project: &Value, committed: bool) -> io::Result<()> {
    let before = validators::seal(&record["before"])?;
    let after = validators::seal(&record["after"])?;
    exact_keys(
        &before,
        &["operation", "source_id", "target_id", "source", "target"],
    )?;
    exact_keys(&after, &["source_id", "target_id", "source", "target"])?;
    let (source, target) = validators::slots(&before)?;
    validators::slot(&before["source"])?;
    validators::slot(&before["target"])?;
    validators::slot(&after["source"])?;
    validators::slot(&after["target"])?;
    let moved = before["operation"] == "move";
    if !["move", "swap"].contains(&before["operation"].as_str().unwrap_or(""))
        || before["source"]["source_identity"].is_null()
        || moved != before["target"]["source_identity"].is_null()
        || !moved
            && before["source"]["source_identity"]["instance_id"]
                == before["target"]["source_identity"]["instance_id"]
        || after["source_id"] != before["source_id"]
        || after["target_id"] != before["target_id"]
        || after["target"] != before["source"]
        || after["source"]
            != if moved {
                validators::empty_slot()
            } else {
                before["target"].clone()
            }
    {
        return Err(invalid(
            "portable Pair original/after seals differ; preserved",
        ));
    }
    let image = if committed { &after } else { &before };
    for (slot, name) in [(source, "source"), (target, "target")] {
        let actual = validators::project_slot(project, slot)?;
        if if committed {
            !validators::slot_matches(samples, record, &actual, &image[name])?
        } else {
            actual != image[name]
        } {
            return Err(invalid(
                "portable Pair project differs from complete participant seals; preserved",
            ));
        }
    }
    Ok(())
}

fn copy(samples: &Path, record: &Value, project: &Value, committed: bool) -> io::Result<()> {
    let (source_id, target_id) = validators::slots(record)?;
    let source = validators::seal(&record["source"])?;
    let predecessor = validators::seal(&record["predecessor"])?;
    let after = validators::seal(&record["after"])?;
    for image in [&source, &predecessor, &after] {
        validators::slot(image)?;
        if image["source_identity"].is_null() {
            return Err(invalid("portable Copy requires occupied seals"));
        }
    }
    let captured = validators::project(
        record["captured_project_image_json"]
            .as_str()
            .ok_or_else(|| invalid("portable Copy captured project missing"))?,
    )?;
    let mut expected = source.clone();
    expected["source_identity"] = after["source_identity"].clone();
    let new_id = &after["source_identity"]["instance_id"];
    if source["source_identity"]["instance_id"] == predecessor["source_identity"]["instance_id"]
        || after != expected
        || after["source_identity"]["material_id"] != source["source_identity"]["material_id"]
        || after["source_identity"]["material_creation"]
            != source["source_identity"]["material_creation"]
        || after["source_identity"]["copied_from"] != source["source_identity"]["instance_id"]
        || captured["pad_content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["instance_id"] == *new_id)
        || captured["config_revision"].as_u64() > record["captured_revision"].as_u64()
        || record["phase"] == "captured"
            && record["resume_of"].is_null()
            && record["project_image_json"] != record["captured_project_image_json"]
        || validators::project_slot(&captured, source_id)? != source
        || validators::project_slot(&captured, target_id)? != predecessor
        || record["resolved_key_lock"].as_bool().is_none()
        || record["resolved_key_lock"]
            != if source["key_lock"].is_null() {
                captured["key_lock"].clone()
            } else {
                source["key_lock"].clone()
            }
        || record["phase"] != "cancelled"
            && record["resolved_key_lock"]
                != if after["key_lock"].is_null() {
                    project["key_lock"].clone()
                } else {
                    after["key_lock"].clone()
                }
    {
        return Err(invalid(
            "portable Copy captured A/B/fresh C seals differ; preserved",
        ));
    }
    let target = if committed { &after } else { &predecessor };
    if !committed
        && project["pad_content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["instance_id"] == *new_id)
        || !validators::slot_matches(
            samples,
            record,
            &validators::project_slot(project, target_id)?,
            target,
        )?
        || !committed
            && record["phase"] == "captured"
            && !validators::slot_matches(
                samples,
                record,
                &validators::project_slot(project, source_id)?,
                &source,
            )?
    {
        return Err(invalid(
            "portable Copy project changes its guarded original/after target; preserved",
        ));
    }
    Ok(())
}

fn changed_commit(samples: &Path, previous: &Value, current: &Value) -> io::Result<bool> {
    if current["kind"] == "pad_bank" {
        return bank::changed_commit(samples, previous, current);
    }
    if previous["committed_revision"].is_null() {
        return Ok(false);
    }
    if current["kind"] == "pad_copy_overwrite" && current["phase"] == "native_ack_observed" {
        return Ok(
            current["intent_revision"].as_u64() <= previous["intent_revision"].as_u64()
                || current["image_role"] != "before"
                || !current["committed_revision"].is_null()
                || !current["committed_config_sha256"].is_null(),
        );
    }
    if current["phase"] == "config_committed" {
        return Ok(current["kind"] == "pad_copy_overwrite"
            && current["intent_revision"] != previous["intent_revision"]
            || current["intent_revision"] == previous["intent_revision"]
                && validators::project_intent(samples, current)?
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

pub(super) fn validate_successors(
    samples: &Path,
    histories: &BTreeMap<String, History>,
) -> io::Result<()> {
    let mut children = BTreeMap::new();
    for (id, child) in histories {
        if child.kind.is_none() {
            continue;
        }
        if let Some(parent_id) = child.first["resume_of"].as_str() {
            if children.insert(parent_id, id).is_some() {
                return Err(invalid("forked portable arrangement lineage; preserved"));
            }
            if let Some(parent) = histories.get(parent_id) {
                if child.kind != parent.kind
                    || ["settled", "cancelled"]
                        .contains(&parent.last["phase"].as_str().unwrap_or(""))
                    || !super::super::config_location::matches(
                        samples,
                        Path::new(&child.config_reference),
                        &parent.config_reference,
                    )?
                    || child.first["captured_config_sha256"]
                        != parent.first["captured_config_sha256"]
                    || child.first["captured_revision"].as_u64()
                        < parent.last["intent_revision"].as_u64()
                {
                    return Err(invalid(
                        "portable successor changes original kind, Config or revision; preserved",
                    ));
                }
                if child.kind.as_deref() == Some("pad_bank") {
                    if bank::SEALS
                        .iter()
                        .any(|key| child.first[*key] != parent.first[*key])
                    {
                        return Err(invalid(
                            "portable Bank successor changes complete original seals; preserved",
                        ));
                    }
                } else if child.kind.as_deref() == Some("pad_copy_overwrite") {
                    if COPY_SEALS
                        .iter()
                        .any(|key| child.first[*key] != parent.first[*key])
                    {
                        return Err(invalid(
                            "portable Copy successor changes captured seals; preserved",
                        ));
                    }
                } else {
                    let before = validators::seal(&parent.first["before"])?;
                    let resumed = validators::seal(&child.first["before"])?;
                    let after = validators::seal(&parent.first["after"])?;
                    let next_after = validators::seal(&child.first["after"])?;
                    if ["operation", "source_id", "target_id"]
                        .iter()
                        .any(|key| before[*key] != resumed[*key])
                        || ["source", "target"].iter().any(|key| {
                            !validators::slot_matches(
                                samples,
                                &child.first,
                                &resumed[*key],
                                &before[*key],
                            )
                            .unwrap_or(false)
                                || !validators::slot_matches(
                                    samples,
                                    &child.first,
                                    &next_after[*key],
                                    &after[*key],
                                )
                                .unwrap_or(false)
                        })
                    {
                        return Err(invalid(
                            "portable Pair successor changes original musical intent; preserved",
                        ));
                    }
                }
            } else {
                // Exact ancestor compaction intentionally leaves a self-contained tail.
                // Copy compacts only after terminal settlement; Pair also supports
                // its existing actual Config-committed ownership transfer.
                let allowed = if child.kind.as_deref() == Some("pad_pair") {
                    &["config_committed", "settled", "cancelled"][..]
                } else {
                    &["settled", "cancelled"][..]
                };
                if !allowed.contains(&child.last["phase"].as_str().unwrap_or("")) {
                    return Err(invalid(
                        "portable arrangement predecessor missing; preserved",
                    ));
                }
            }
        }
        let mut seen = BTreeSet::new();
        let mut current = id.as_str();
        while let Some(history) = histories.get(current) {
            if !seen.insert(current) {
                return Err(invalid("cyclic portable arrangement lineage; preserved"));
            }
            match history.first["resume_of"].as_str() {
                Some(parent) => current = parent,
                None => break,
            }
        }
    }
    Ok(())
}

// Keep the previously accepted Legacy schema and per-record interpretation intact.
fn legacy(record: &Value) -> io::Result<()> {
    exact_keys(
        record,
        &[
            "schema_version",
            "transaction_id",
            "phase",
            "config_reference",
            "captured_revision",
            "intent_revision",
            "config_sha256",
            "snapshot_json",
            "assignments",
            "alias",
            "committed_revision",
            "committed_config_sha256",
            "artifacts",
            "resume_of",
            "cleanup_complete",
            "error",
        ],
    )?;
    if record["captured_revision"].as_u64().is_none()
        || record["intent_revision"].as_u64().is_none()
        || record["cleanup_complete"].as_bool().is_none()
        || !record["snapshot_json"].as_str().is_some_and(|text| {
            text.len() <= 8 * 1024 * 1024
                && serde_json::from_str::<Value>(text).is_ok_and(|value| value.is_object())
        })
        || !record["assignments"].as_array().is_some_and(|items| {
            !items.is_empty()
                && items.len() <= 216
                && items.iter().all(|item| {
                    item["sample_id"].as_u64().is_some_and(|id| id < 216)
                        && item["instance_id"]
                            .as_str()
                            .is_some_and(material_paths::valid_id)
                        && item["old_reference"]
                            .as_str()
                            .is_some_and(|value| !value.is_empty())
                })
        })
        || !record["artifacts"].as_array().is_some_and(|items| {
            items.len() <= 1024
                && items.iter().all(|item| {
                    ["target", "rollback"].contains(&item["role"].as_str().unwrap_or(""))
                        && item["created"].as_bool().is_some()
                        && serde_json::from_value::<Receipt>(item["evidence"].clone()).is_ok()
                })
        })
        || record["phase"] == "config_committed"
            && (record["committed_revision"].as_u64().is_none()
                || !record["committed_config_sha256"]
                    .as_str()
                    .is_some_and(digest_valid))
        || !record["phase"].as_str().is_some_and(|phase| {
            [
                "captured",
                "material_verified",
                "references_prepared",
                "adoption_pending",
                "ack_confirmed",
                "config_committed",
                "failed",
                "unresolved",
            ]
            .contains(&phase)
        })
    {
        return Err(invalid(
            "unrecognized journal history blocks portable relocation; preserved",
        ));
    }
    Ok(())
}
