//! Actual bounded JournalStore/Portable recognition; no audio/runtime authority.
#![cfg(windows)]

use super::*;
use crate::audio_engine::material_migration_journal::MaterialMigrationJournalStore;
use crate::audio_engine::material_migration_recovery::portable_relocation::{
    capture_stream, identity, run, verified_projection,
};
use pyo3::Python;
use std::io::Write;
use std::os::windows::fs::MetadataExt;

const TRANSACTION: &str = "11111111111111111111111111111111";
const CHILD: &str = "22222222222222222222222222222222";
const SECOND_CHILD: &str = "33333333333333333333333333333333";
const CONFIG: &str = "projects/bank.json";

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
    record: PathBuf,
    birth: Value,
}

impl Fixture {
    fn new() -> Self {
        Python::initialize();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let artifact_root = workspace.join("test-artifacts");
        fs::create_dir_all(&artifact_root).unwrap();
        assert_eq!(
            fs::symlink_metadata(&artifact_root)
                .unwrap()
                .file_attributes()
                & 0x400,
            0
        );
        let temporary = tempfile::Builder::new()
            .prefix("native-portable-bank-journal-")
            .tempdir_in(artifact_root)
            .unwrap();
        // A unique owned directory needs no shared marker publication protocol.
        fs::OpenOptions::new().write(true).create_new(true)
            .open(temporary.path().join("OWNER.md")).unwrap()
            .write_all(b"Owned reproducible Native Portable Bank journal unit-test fixture.\nNo Human audio, settings or installations.\n").unwrap();
        let root = temporary.path().join("PortableRoot");
        let source = root.join("data/samples");
        let target = root.join("flitzis-looper/samples");
        fs::create_dir_all(source.join("projects")).unwrap();
        fs::create_dir_all(source.join("BankSource")).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::create_dir(root.join("cache")).unwrap();
        fs::write(
            root.join("portable-root.json"),
            br#"{"schema_version":1,"app":"flitzis-looper"}"#,
        )
        .unwrap();
        let original = b"owned deterministic Portable bank fixture source";
        fs::write(source.join("BankSource/original.wav"), original).unwrap();
        let marker = json!({"schema_version":1,"sha256":format!("{:x}",Sha256::digest(original)),
            "bytes":original.len(),"original_leaf":"original.wav"})
        .to_string();
        let marker_path = source.join("BankSource/.material.json");
        fs::write(&marker_path, &marker).unwrap();
        let birth = json!({"schema_version":1,"root_reference":"samples/BankSource",
            "root_identity":identity(&source.join("BankSource")).unwrap(),
            "binding":{"name":".material.json","identity":identity(&marker_path).unwrap(),
                "bytes":marker.len(),"sha256":format!("{:x}",Sha256::digest(marker.as_bytes()))}});
        let record = root.join("cache/relocation-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json");
        Self {
            _temporary: temporary,
            root,
            source,
            target,
            record,
            birth,
        }
    }

    fn initial(&self, case: &str) -> Value {
        let empty = current_empty();
        let mut project = project(&empty);
        for id in 0..36 {
            if case == "full" || [0, 35].contains(&id) {
                place(&mut project, id, &self.occupied(id));
            }
        }
        for id in 36..72 {
            if case != "mixed" || id != 70 {
                place(&mut project, id, &self.occupied(id));
            }
        }
        place(&mut project, 108, &self.occupied(108));
        project["pad_gain_db"][1] = json!(-3.0); // Complete Empty source witness.
        project["pad_gain_db"][70] = json!(-19.0);
        project["pad_key_lock"][35] = json!(false);
        let rows: Vec<Value> = (0..36)
            .map(|offset| {
                let source =
                    (case != "clear").then(|| validators::project_slot(&project, offset).unwrap());
                let before = validators::project_slot(&project, 36 + offset).unwrap();
                let mut after = empty.clone();
                let mut resolved = Value::Null;
                if let Some(source) = source
                    .as_ref()
                    .filter(|image| !image["source_identity"].is_null())
                {
                    after = source.clone();
                    after["source_identity"]["instance_id"] =
                        json!(format!("{:032x}", 2_000 + offset));
                    after["source_identity"]["copied_from"] =
                        source["source_identity"]["instance_id"].clone();
                    resolved = if source["key_lock"].is_null() {
                        project["key_lock"].clone()
                    } else {
                        source["key_lock"].clone()
                    };
                }
                json!({"source_id":(case != "clear").then_some(offset),"target_id":36 + offset,
                "source":source,"before":before,"after":after,"resolved_key_lock":resolved})
            })
            .collect();
        let text = project.to_string();
        json!({"kind":"pad_bank","schema_version":1,"transaction_id":TRANSACTION,"resume_of":null,
            "config_reference":plain_path(&self.source.join(CONFIG)),"captured_revision":1,"intent_revision":1,
            "captured_config_sha256":format!("{:x}",Sha256::digest(text.as_bytes())),
            "bank":sealed(&json!({"source_bank":(case != "clear").then_some(0),"target_bank":1,"slots":rows})),
            "captured_project_image_json":text,"project_image_json":text,"image_role":"before","phase":"captured",
            "committed_revision":null,"committed_config_sha256":null,"error":null})
    }

    fn occupied(&self, id: usize) -> Value {
        let mut image = current_empty();
        image["source_path"] = json!("samples/BankSource/original.wav");
        image["source_identity"] = json!({"instance_id":format!("{:032x}",100 + id),
            "material_id":"55555555555555555555555555555555","material_creation":self.birth,"copied_from":null});
        image["duration"] = json!(3.0);
        image["gain_db"] = json!(-6.0 - (id % 7) as f64);
        image
    }

    fn append(&self, id: &str, records: &[Value]) {
        fs::write(
            self.source.join(CONFIG),
            records.last().unwrap()["project_image_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let store =
            MaterialMigrationJournalStore::new(plain_path(&self.source), id.to_owned()).unwrap();
        for record in records {
            store.append(record.to_string()).unwrap();
        }
        drop(store);
    }

    fn act(&self, action: &str) -> io::Result<String> {
        run(&self.root, &self.source, &self.target, &self.record, action)
    }

    fn reject(&self, records: &[Value], context: &str) {
        assert!(
            validate(&self.source, TRANSACTION, records).is_err(),
            "{context}: history accepted"
        );
        self.append(TRANSACTION, records);
        let before = snapshot(&self.source);
        let cache = snapshot(&self.root.join("cache"));
        assert!(
            self.act("capture").is_err(),
            "{context}: actual Portable capture accepted"
        );
        assert_eq!(snapshot(&self.source), before);
        assert_eq!(snapshot(&self.root.join("cache")), cache);
        assert!(!self.target.exists());
        assert!(!self.record.exists());
        assert!(!capture_stream(&self.source).exists());
    }
}

fn current_empty() -> Value {
    let mut image = validators::empty_slot();
    image["key_intent"]["extra_retrigger"] = json!(false);
    image
}

fn project(empty: &Value) -> Value {
    let mut project = json!({"config_revision":1,"material_migrations":{},"key_lock_intent_version":2,
        "multi_loop":false,"key_lock":true,"key_lock_start_behavior":"attack_at_grid","bpm_lock":false,
        "trigger_quantization_enabled":false,"trigger_quantization_step":"1_32","input_mapping_enabled":true,
        "stem_separator":"demucs:htdemucs","preload_stem_loops_on_startup":false,"resident_pcm_budget_mib":512,
        "demucs_shifts":1,"demucs_overlap":0.5,"volume":1.0,"speed":1.0,"selected_pad":108,"selected_bank":3,
        "sidebar_left_expanded":true,"sidebar_right_expanded":false});
    for (name, field) in validators::CONTENT {
        project[*field] = json!(vec![empty[*name].clone(); 216]);
    }
    project
}

fn place(project: &mut Value, id: usize, image: &Value) {
    for (name, field) in validators::CONTENT {
        project[*field][id] = image[*name].clone();
    }
    if project.get("pad_source_beatmap_intent").is_some()
        || image.get("source_beatmap_intent").is_some()
    {
        if project.get("pad_source_beatmap_intent").is_none() {
            project["pad_source_beatmap_intent"] = json!(vec![Value::Null; 216]);
        }
        project["pad_source_beatmap_intent"][id] = image
            .get("source_beatmap_intent")
            .cloned()
            .unwrap_or(Value::Null);
    }
}

fn sealed(value: &Value) -> Value {
    json!({"schema_version":1,"payload_json":value.to_string()})
}
fn bank(record: &Value) -> Value {
    validators::seal(&record["bank"]).unwrap()
}
fn image(record: &Value) -> Value {
    serde_json::from_str(record["project_image_json"].as_str().unwrap()).unwrap()
}
fn set_image(record: &mut Value, project: Value) {
    let text = project.to_string();
    record["project_image_json"] = json!(text);
    if !record["committed_revision"].is_null() {
        record["committed_config_sha256"] = json!(format!("{:x}", Sha256::digest(text.as_bytes())));
    }
}
fn receipt(ack: &Value) -> Value {
    let mut record = ack.clone();
    let mut project = image(ack);
    for row in bank(ack)["slots"].as_array().unwrap() {
        place(
            &mut project,
            row["target_id"].as_u64().unwrap() as usize,
            &row["after"],
        );
    }
    project["config_revision"] = ack["intent_revision"].clone();
    record["phase"] = json!("config_committed");
    record["image_role"] = json!("after");
    record["committed_revision"] = ack["intent_revision"].clone();
    set_image(&mut record, project);
    record
}
fn history(initial: &Value, terminal: &str) -> Vec<Value> {
    let mut records = vec![initial.clone()];
    for phase in [
        "publication_pending",
        "native_ack_observed",
        "config_committed",
        "settled",
    ] {
        let mut record = records.last().unwrap().clone();
        if phase == "config_committed" {
            record = receipt(&record);
        } else {
            record["phase"] = json!(phase);
        }
        records.push(record);
        if phase == terminal {
            break;
        }
    }
    records
}
fn validate(samples: &Path, id: &str, records: &[Value]) -> io::Result<History> {
    History::validate(
        samples,
        id,
        &records.iter().map(Value::to_string).collect::<Vec<_>>(),
    )
}
type Snapshot = BTreeMap<String, (FileIdentity, Option<Vec<u8>>)>;
fn snapshot(root: &Path) -> Snapshot {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    result.insert(String::new(), (identity(root).unwrap(), None));
    while let Some(directory) = pending.pop() {
        for item in fs::read_dir(directory).unwrap() {
            let path = item.unwrap().path();
            let bytes = if path.is_dir() {
                pending.push(path.clone());
                None
            } else {
                Some(fs::read(&path).unwrap())
            };
            result.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                (identity(&path).unwrap(), bytes),
            );
        }
    }
    result
}

#[test]
fn native_portable_bank_carries_compact_map_selection_and_keeps_neutral_empty_legacy_shape() {
    let fixture = Fixture::new();
    let mut initial = fixture.initial("mixed");
    let intent = json!({"schema_version":1,"revision":9,"selected":{
        "schema_version":1,"kind":"source-beatmap-v1",
        "reference":"samples/BankSource/.pcm-cache/source-beatmaps/v1/.ready-0123456789abcdef0123456789abcdef/map.json",
        "bytes":1024,"sha256":"a".repeat(64),"map_revision":"b".repeat(64),"decoder_identity":"c".repeat(64)}});
    let mut bank = bank(&initial);
    bank["slots"][0]["source"]["source_beatmap_intent"] = intent.clone();
    bank["slots"][0]["after"]["source_beatmap_intent"] = intent.clone();
    initial["bank"] = sealed(&bank);
    let mut captured = image(&initial);
    captured["pad_source_beatmap_intent"] = json!(vec![Value::Null; 216]);
    captured["pad_source_beatmap_intent"][0] = intent;
    set_image(&mut initial, captured);
    initial["captured_project_image_json"] = initial["project_image_json"].clone();
    initial["captured_config_sha256"] = json!(format!(
        "{:x}",
        Sha256::digest(initial["project_image_json"].as_str().unwrap().as_bytes())
    ));
    let records = history(&initial, "settled");
    assert!(validate(&fixture.source, TRANSACTION, &records).is_ok());
    fixture.append(TRANSACTION, &records);
    let before = snapshot(&fixture.source);
    fixture.act("capture").unwrap();
    fixture.act("move").unwrap();
    let mut moved = snapshot(&fixture.target);
    moved.remove(".portable-location-v1.json");
    assert_eq!(moved, before);
    fixture.act("recover").unwrap();
    fixture.act("rollback").unwrap();
    assert_eq!(snapshot(&fixture.source), before);
}

#[test]
fn native_portable_bank_mixed_full_clear_keep_complete_histories_bytes_and_fileids() {
    for case in ["mixed", "full", "clear"] {
        for policy in ["first_sound_at_grid", "attack_at_grid"] {
            let fixture = Fixture::new();
            let mut initial = fixture.initial(case);
            let mut project = image(&initial);
            project["key_lock_start_behavior"] = json!(policy);
            set_image(&mut initial, project);
            initial["captured_project_image_json"] = initial["project_image_json"].clone();
            initial["captured_config_sha256"] = json!(format!(
                "{:x}",
                Sha256::digest(initial["project_image_json"].as_str().unwrap().as_bytes())
            ));
            let records = history(&initial, "settled");
            assert!(validate(&fixture.source, TRANSACTION, &records).is_ok());
            fixture.append(TRANSACTION, &records);
            let before = snapshot(&fixture.source);
            fixture.act("capture").unwrap();
            fixture.act("move").unwrap();
            let mut after = snapshot(&fixture.target);
            after.remove(".portable-location-v1.json");
            assert_eq!(after, before);
            assert!(verified_projection(&fixture.target).unwrap().is_some());
            assert!(!fixture.source.exists());
            fixture.act("recover").unwrap();
            fixture.act("rollback").unwrap();
            assert_eq!(snapshot(&fixture.source), before);
        }
    }
}

#[test]
fn native_portable_bank_rejects_malformed_final_and_earlier_ordinals_without_touching_files() {
    for fault in [
        "last_target",
        "partial_bank",
        "row_order",
        "source_none",
        "clear_source",
        "lineage",
        "birth",
        "duplicate_after",
        "outside_collision",
        "current_before_collision",
        "committed_outside_collision",
        "empty_after",
        "inheritance",
        "unknown_record",
        "unknown_row",
        "unknown_project",
        "unknown_kind",
        "missing_policy",
        "invalid_policy",
        "missing_slot_extra",
        "missing_project_extra",
        "first_phase",
        "phase_jump",
        "receipt_digest",
        "receipt_revision",
        "partial_after",
        "unrecorded_global",
        "unrecorded_source",
        "earlier_ordinal",
    ] {
        let fixture = Fixture::new();
        let initial = fixture.initial("mixed");
        let mut records = history(&initial, "settled");
        match fault {
            "receipt_digest" => records[3]["committed_config_sha256"] = json!("0".repeat(64)),
            "receipt_revision" => records[3]["committed_revision"] = json!(2),
            "first_phase" => records[0]["phase"] = json!("publication_pending"),
            "phase_jump" => {
                records.drain(1..3);
            }
            "earlier_ordinal" => records[1]["unrecognized_earlier_ordinal"] = json!(true),
            "current_before_collision" => {
                records.truncate(3);
                let mut project = image(&records[2]);
                project["pad_content"][108] =
                    bank(&initial)["slots"][0]["after"]["source_identity"].clone();
                set_image(&mut records[2], project);
            }
            "partial_after"
            | "unrecorded_global"
            | "unrecorded_source"
            | "unknown_project"
            | "invalid_policy"
            | "committed_outside_collision" => {
                let mut project = image(&records[3]);
                match fault {
                    "partial_after" => {
                        place(&mut project, 37, &bank(&initial)["slots"][1]["before"])
                    }
                    "unrecorded_global" => project["volume"] = json!(0.4),
                    "unrecorded_source" => project["pad_gain_db"][0] = json!(-21.0),
                    "unknown_project" => project["unrecognized_project_field"] = json!(true),
                    "committed_outside_collision" => {
                        project["pad_content"][108] = project["pad_content"][36].clone()
                    }
                    _ => project["key_lock_start_behavior"] = json!("unknown_policy"),
                }
                set_image(&mut records[3], project);
                records[4] = records[3].clone();
                records[4]["phase"] = json!("settled");
            }
            _ => {
                records.truncate(3);
                let mut altered = bank(&initial);
                match fault {
                    "last_target" => altered["slots"][35]["target_id"] = json!(70),
                    "partial_bank" => {
                        altered["slots"].as_array_mut().unwrap().pop();
                    }
                    "row_order" => altered["slots"].as_array_mut().unwrap().swap(0, 35),
                    "source_none" => altered["slots"][35]["source"] = Value::Null,
                    "clear_source" => altered["source_bank"] = Value::Null,
                    "lineage" => {
                        altered["slots"][35]["after"]["source_identity"]["copied_from"] =
                            json!(SECOND_CHILD)
                    }
                    "birth" => {
                        altered["slots"][35]["after"]["source_identity"]["material_creation"]["binding"]
                            ["sha256"] = json!("0".repeat(64))
                    }
                    "duplicate_after" => {
                        altered["slots"][35]["after"]["source_identity"]["instance_id"] =
                            altered["slots"][0]["after"]["source_identity"]["instance_id"].clone()
                    }
                    "empty_after" => altered["slots"][1]["after"]["gain_db"] = json!(3.0),
                    "inheritance" => altered["slots"][0]["resolved_key_lock"] = json!(false),
                    "unknown_row" => {
                        altered["slots"][35]["before"]["unrecognized_musical_field"] = json!(true)
                    }
                    "missing_slot_extra" => {
                        altered["slots"][35]["source"]["key_intent"]
                            .as_object_mut()
                            .unwrap()
                            .remove("extra_retrigger");
                    }
                    "unknown_record" => records[2]["unrecognized_record_field"] = json!(true),
                    "unknown_kind" => records[2]["kind"] = json!("pad_bank_unknown"),
                    "outside_collision" | "missing_policy" | "missing_project_extra" => {
                        let mut project = image(&initial);
                        if fault == "outside_collision" {
                            project["pad_content"][108] =
                                altered["slots"][0]["after"]["source_identity"].clone();
                        } else if fault == "missing_policy" {
                            project
                                .as_object_mut()
                                .unwrap()
                                .remove("key_lock_start_behavior");
                        } else {
                            project["pad_key_intent"][108]
                                .as_object_mut()
                                .unwrap()
                                .remove("extra_retrigger");
                        }
                        for record in &mut records {
                            set_image(record, project.clone());
                            record["captured_project_image_json"] =
                                record["project_image_json"].clone();
                        }
                    }
                    _ => unreachable!("{fault}"),
                }
                if !["unknown_record", "unknown_kind"].contains(&fault) {
                    for record in &mut records {
                        record["bank"] = sealed(&altered);
                    }
                }
            }
        }
        fixture.reject(&records, fault);
    }
}

#[test]
fn native_portable_bank_receipts_require_recorded_full_intent_and_new_revision_after_commit() {
    let fixture = Fixture::new();
    let initial = fixture.initial("mixed");
    let mut records = history(&initial, "config_committed");
    let mut ack = records[2].clone();
    ack["intent_revision"] = json!(2);
    let mut project = image(&ack);
    project["pad_gain_db"][0] = json!(-17.0);
    project["pad_gain_db"][108] = json!(-19.0);
    project["volume"] = json!(0.4);
    project["key_lock_start_behavior"] = json!("first_sound_at_grid");
    set_image(&mut ack, project);
    records.push(ack.clone());
    records.push(receipt(&ack));
    let mut settled = records.last().unwrap().clone();
    settled["phase"] = json!("settled");
    records.push(settled);
    assert!(validate(&fixture.source, TRANSACTION, &records).is_ok());
    fixture.append(TRANSACTION, &records);
    fixture.act("capture").unwrap();
    for fault in [
        "same_revision_ack",
        "new_revision_without_ack",
        "unrecorded_second_receipt",
        "changed_repeat_commit",
    ] {
        let fixture = Fixture::new();
        let initial = fixture.initial("mixed");
        let mut rejected = history(&initial, "config_committed");
        match fault {
            "same_revision_ack" => rejected.push(rejected[2].clone()),
            "new_revision_without_ack" => {
                let mut next = rejected[2].clone();
                next["intent_revision"] = json!(2);
                rejected.push(receipt(&next));
            }
            "unrecorded_second_receipt" => {
                let mut next = rejected[2].clone();
                next["intent_revision"] = json!(2);
                rejected.push(next.clone());
                let mut committed = receipt(&next);
                let mut changed = image(&committed);
                changed["volume"] = json!(0.4);
                set_image(&mut committed, changed);
                rejected.push(committed);
            }
            _ => {
                let mut next = rejected[3].clone();
                let mut changed = image(&next);
                changed["volume"] = json!(0.4);
                set_image(&mut next, changed);
                rejected.push(next);
            }
        }
        fixture.reject(&rejected, fault);
    }
}

#[test]
fn native_portable_bank_successors_retain_whole_seals_and_reject_forks_cycles_and_missing_parents()
{
    for fault in [
        "valid",
        "fork",
        "cycle",
        "terminal_parent",
        "changed_capture",
        "changed_bank",
        "old_revision",
        "missing_parent",
    ] {
        let fixture = Fixture::new();
        let initial = fixture.initial("mixed");
        let mut parent = history(
            &initial,
            if fault == "terminal_parent" {
                "settled"
            } else {
                "native_ack_observed"
            },
        );
        let mut child = initial.clone();
        child["transaction_id"] = json!(CHILD);
        child["resume_of"] = json!(TRANSACTION);
        if fault == "changed_capture" || fault == "changed_bank" {
            let mut project = image(&child);
            if fault == "changed_capture" {
                project["pad_gain_db"][108] = json!(-20.0);
            } else {
                project["pad_gain_db"][0] = json!(-20.0);
                let mut changed = bank(&child);
                changed["slots"][0]["source"]["gain_db"] = json!(-20.0);
                changed["slots"][0]["after"]["gain_db"] = json!(-20.0);
                child["bank"] = sealed(&changed);
            }
            set_image(&mut child, project);
            child["captured_project_image_json"] = child["project_image_json"].clone();
        }
        if fault == "old_revision" {
            parent.last_mut().unwrap()["intent_revision"] = json!(2);
        }
        if fault == "cycle" {
            for record in &mut parent {
                record["resume_of"] = json!(CHILD);
            }
        }
        let child_records = history(&child, "native_ack_observed");
        let mut histories = BTreeMap::new();
        if fault != "missing_parent" {
            histories.insert(
                TRANSACTION.to_owned(),
                validate(&fixture.source, TRANSACTION, &parent).unwrap(),
            );
            fixture.append(TRANSACTION, &parent);
        }
        histories.insert(
            CHILD.to_owned(),
            validate(&fixture.source, CHILD, &child_records).unwrap(),
        );
        fixture.append(CHILD, &child_records);
        if fault == "fork" {
            let mut second = child.clone();
            second["transaction_id"] = json!(SECOND_CHILD);
            let records = history(&second, "native_ack_observed");
            histories.insert(
                SECOND_CHILD.to_owned(),
                validate(&fixture.source, SECOND_CHILD, &records).unwrap(),
            );
            fixture.append(SECOND_CHILD, &records);
        }
        let before = snapshot(&fixture.source);
        if fault == "valid" {
            assert!(validate_successors(&fixture.source, &histories).is_ok());
            fixture.act("capture").unwrap();
        } else {
            assert!(
                validate_successors(&fixture.source, &histories).is_err(),
                "{fault}"
            );
            assert!(fixture.act("capture").is_err(), "{fault}");
            assert_eq!(snapshot(&fixture.source), before);
            assert!(!fixture.target.exists());
        }
    }
}

fn two_slot(initial: &Value, kind: &str) -> Value {
    let payload = bank(initial);
    let row = &payload["slots"][0];
    let mut record = initial.clone();
    record["kind"] = json!(kind);
    record.as_object_mut().unwrap().remove("bank");
    if kind == "pad_pair" {
        record
            .as_object_mut()
            .unwrap()
            .remove("captured_project_image_json");
        record["before"] = sealed(
            &json!({"operation":"swap","source_id":0,"target_id":36,"source":row["source"],"target":row["before"]}),
        );
        record["after"] = sealed(
            &json!({"source_id":0,"target_id":36,"source":row["before"],"target":row["source"]}),
        );
    } else {
        record["source_id"] = json!(0);
        record["target_id"] = json!(36);
        record["source"] = sealed(&row["source"]);
        record["predecessor"] = sealed(&row["before"]);
        record["after"] = sealed(&row["after"]);
        record["resolved_key_lock"] = row["resolved_key_lock"].clone();
    }
    record
}

#[test]
fn native_portable_current_pair_copy_policy_and_historical_omission_remain_distinct_from_bank() {
    for kind in ["pad_pair", "pad_copy_overwrite"] {
        for policy in [None, Some("first_sound_at_grid"), Some("attack_at_grid")] {
            let fixture = Fixture::new();
            let mut record = two_slot(&fixture.initial("mixed"), kind);
            let mut project = image(&record);
            if let Some(policy) = policy {
                project["key_lock_start_behavior"] = json!(policy);
            } else {
                project
                    .as_object_mut()
                    .unwrap()
                    .remove("key_lock_start_behavior");
                for intent in project["pad_key_intent"].as_array_mut().unwrap() {
                    intent.as_object_mut().unwrap().remove("extra_retrigger");
                }
                for name in if kind == "pad_pair" {
                    &["before", "after"][..]
                } else {
                    &["source", "predecessor", "after"][..]
                } {
                    let mut payload = validators::seal(&record[*name]).unwrap();
                    if kind == "pad_pair" {
                        for slot in ["source", "target"] {
                            payload[slot]["key_intent"]
                                .as_object_mut()
                                .unwrap()
                                .remove("extra_retrigger");
                        }
                    } else {
                        payload["key_intent"]
                            .as_object_mut()
                            .unwrap()
                            .remove("extra_retrigger");
                    }
                    record[*name] = sealed(&payload);
                }
            }
            set_image(&mut record, project);
            if kind == "pad_copy_overwrite" {
                record["captured_project_image_json"] = record["project_image_json"].clone();
            }
            record["captured_config_sha256"] = json!(format!(
                "{:x}",
                Sha256::digest(record["project_image_json"].as_str().unwrap().as_bytes())
            ));
            assert!(
                validate(&fixture.source, TRANSACTION, &[record.clone()]).is_ok(),
                "{kind} {policy:?}"
            );
            fixture.append(TRANSACTION, &[record]);
            fixture.act("capture").unwrap();
        }
        for field in ["key_lock_start_behavior", "unknown_project_field"] {
            for value in [json!("bad_policy"), Value::Null, json!(false)] {
                let fixture = Fixture::new();
                let mut record = two_slot(&fixture.initial("mixed"), kind);
                let mut project = image(&record);
                project[field] = value;
                set_image(&mut record, project);
                if kind == "pad_copy_overwrite" {
                    record["captured_project_image_json"] = record["project_image_json"].clone();
                }
                fixture.reject(&[record], &format!("{kind}: invalid {field}"));
            }
        }
    }
}

#[test]
fn native_portable_bank_cancel_and_receipt_normalization_do_not_claim_stem_availability() {
    let fixture = Fixture::new();
    let mut initial = fixture.initial("mixed");
    let cache = json!({"source_version":"fixture-version","cache_dir":"samples/BankSource/stems",
        "stems":{"vocals":null,"melody":null,"bass":null,"drums":null,"instrumental":null},
        "available":false,"pair":null,"fingerprint_schema":null,"generation_fingerprint":null,"producer_identity":null});
    let analysis = json!({"bpm":0.0,"key":"","beat_grid":{"beats":[],"downbeats":[],"bars":[]},"accepted_timing":null});
    let mut captured = image(&initial);
    captured["stem_cache"][0] = cache.clone();
    captured["sample_analysis"][0] = analysis.clone();
    let mut payload = bank(&initial);
    for role in ["source", "after"] {
        payload["slots"][0][role]["stem_cache"] = cache.clone();
        payload["slots"][0][role]["analysis"] = analysis.clone();
    }
    initial["bank"] = sealed(&payload);
    set_image(&mut initial, captured);
    initial["captured_project_image_json"] = initial["project_image_json"].clone();
    initial["captured_config_sha256"] = json!(format!(
        "{:x}",
        Sha256::digest(initial["project_image_json"].as_str().unwrap().as_bytes())
    ));
    let mut cancelled = initial.clone();
    cancelled["phase"] = json!("cancelled");
    let mut cancelled_project = image(&cancelled);
    cancelled_project["key_lock"] = json!(false);
    set_image(&mut cancelled, cancelled_project);
    assert!(validate(&fixture.source, TRANSACTION, &[initial.clone(), cancelled]).is_ok());
    let mut records = history(&initial, "config_committed");
    let mut written = image(&records[3]);
    written["sample_analysis"][0] = Value::Null;
    written["sample_analysis"][36] = Value::Null;
    written["stem_cache"][36]["available"] = json!(true);
    written["sample_paths"][36] =
        json!(plain_path(&fixture.source.join("BankSource/original.wav")));
    set_image(&mut records[3], written);
    assert!(validate(&fixture.source, TRANSACTION, &records).is_ok());
    let mut wrong_outside_eligibility = records.clone();
    let mut written = image(&wrong_outside_eligibility[3]);
    written["stem_cache"][0]["available"] = json!(true);
    set_image(&mut wrong_outside_eligibility[3], written);
    fixture.reject(
        &wrong_outside_eligibility,
        "unrecorded outside availability",
    );
}

#[test]
fn native_portable_bank_duplicate_json_and_history_bounds_fail_closed() {
    let fixture = Fixture::new();
    let initial = fixture.initial("mixed");
    assert!(History::validate(&fixture.source, TRANSACTION, &[]).is_err());
    assert!(validate(&fixture.source, TRANSACTION, &vec![initial.clone(); 17]).is_err());
    let mut duplicate = initial.clone();
    duplicate["bank"]["payload_json"] = json!(
        initial["bank"]["payload_json"]
            .as_str()
            .unwrap()
            .replacen('{', "{\"target_bank\":1,", 1)
    );
    fixture.reject(&[duplicate], "duplicate Bank JSON key");
}

#[test]
fn native_portable_bank_rejects_raw_project_availability_before_normalization() {
    for later in [false, true] {
        for invalid in [None, Some(Value::Null), Some(json!("false"))] {
            let fixture = Fixture::new();
            let mut initial = fixture.initial("mixed");
            let mut original = image(&initial);
            original["stem_cache"][108] = json!({"source_version":"fixture-version", "cache_dir":"samples/BankSource/stems",
                "stems":{"vocals":null,"melody":null,"bass":null,"drums":null,"instrumental":null},
                "available":false,"pair":null,"fingerprint_schema":null,"generation_fingerprint":null,"producer_identity":null});
            set_image(&mut initial, original);
            initial["captured_project_image_json"] = initial["project_image_json"].clone();
            initial["captured_config_sha256"] = json!(format!(
                "{:x}",
                Sha256::digest(initial["project_image_json"].as_str().unwrap().as_bytes())
            ));
            let mut records = history(&initial, "config_committed");
            for record in records.iter_mut().skip(usize::from(later)) {
                let mut project = image(record);
                let cache = project["stem_cache"][108].as_object_mut().unwrap();
                if let Some(value) = &invalid {
                    cache.insert("available".to_owned(), value.clone());
                } else {
                    cache.remove("available");
                }
                set_image(record, project);
            }
            if !later {
                let captured = records[0]["project_image_json"].clone();
                let digest = json!(format!(
                    "{:x}",
                    Sha256::digest(captured.as_str().unwrap().as_bytes())
                ));
                for record in &mut records {
                    record["captured_project_image_json"] = captured.clone();
                    record["captured_config_sha256"] = digest.clone();
                }
            }
            fixture.reject(
                &records,
                &format!("raw availability {invalid:?}, later={later}"),
            );
        }
    }
}
