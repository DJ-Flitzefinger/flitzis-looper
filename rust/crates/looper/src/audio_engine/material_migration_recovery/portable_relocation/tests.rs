//! Actual installer custody and location tests; these do not restore audio authority.
#![cfg(windows)]

use super::super::{MigrationProjectGuard, config_location, inventory_gate};
use super::*;
use crate::audio_engine::material_migration_journal::MaterialMigrationJournalStore;
use pyo3::Python;
use std::os::windows::process::CommandExt;
use tempfile::TempDir;

const TRANSACTION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OPERATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SECOND_OPERATION: &str = "cccccccccccccccccccccccccccccccc";
const CONFIG: &str = "projects/session.json";
const ORIGINAL: &str = "original.wav";

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
    record: PathBuf,
    instance: String,
}

impl Fixture {
    fn new(legacy: bool) -> Self {
        Python::initialize();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .expect("manifest is inside the authorized workspace");
        let scratch = workspace.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        let temporary = tempfile::Builder::new()
            .prefix("portable-native-")
            .tempdir_in(scratch)
            .unwrap();
        let root = temporary.path().join("PortableRoot");
        let target = root.join("flitzis-looper/samples");
        let source = if legacy {
            root.join("data/samples")
        } else {
            target.clone()
        };
        fs::create_dir_all(source.join("projects")).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::create_dir(root.join("cache")).unwrap();
        fs::write(
            root.join("portable-root.json"),
            br#"{"schema_version":1,"app":"flitzis-looper"}"#,
        )
        .unwrap();
        let image = json!({
            "config_revision":1,
            "sample_paths":["samples/original.wav"],
            "stem_cache":[]
        });
        let config_bytes = serde_json::to_vec(&image).unwrap();
        fs::write(source.join(CONFIG), &config_bytes).unwrap();
        fs::write(source.join("projects/same.json"), &config_bytes).unwrap();
        fs::write(source.join(ORIGINAL), b"one original").unwrap();
        let mut guard =
            MigrationProjectGuard::create_native(&source, &source.join(CONFIG)).unwrap();
        let instance = guard.instance_id();
        guard.release();
        drop(guard);
        let journal =
            MaterialMigrationJournalStore::new(plain_path(&source), TRANSACTION.to_owned())
                .unwrap();
        for phase in ["captured", "unresolved"] {
            // All 16 protocol fields are present. An unresolved journal is retained
            // intent, with no invented Source ACK, material birth or adoption proof.
            journal
                .append(
                    json!({
                        "schema_version":1,
                        "transaction_id":TRANSACTION,
                        "phase":phase,
                        "config_reference":plain_path(&source.join(CONFIG)),
                        "captured_revision":1,
                        "intent_revision":1,
                        "config_sha256":format!("{:x}", Sha256::digest(&config_bytes)),
                        "snapshot_json":image.to_string(),
                        "assignments":[{
                            "sample_id":0,
                            "instance_id":instance,
                            "old_reference":"samples/original.wav"
                        }],
                        "alias":null,
                        "committed_revision":null,
                        "committed_config_sha256":null,
                        "artifacts":[],
                        "resume_of":null,
                        "cleanup_complete":false,
                        "error":null
                    })
                    .to_string(),
                )
                .unwrap();
        }
        drop(journal);
        let record = root
            .join("cache")
            .join(format!("relocation-{OPERATION}.json"));
        Self {
            _temporary: temporary,
            root,
            source,
            target,
            record,
            instance,
        }
    }

    fn act(&self, action: &str) -> io::Result<String> {
        run(&self.root, &self.source, &self.target, &self.record, action)
    }

    fn journal(&self, samples: &Path, index: usize) -> PathBuf {
        samples
            .join(".material-migrations")
            .join(TRANSACTION)
            .join(format!("{index:02}.json"))
    }

    fn capture(&self) {
        assert_eq!(phase(self.act("capture").unwrap()), "captured");
    }

    fn move_samples(&self) {
        self.capture();
        assert_eq!(phase(self.act("move").unwrap()), "moved");
    }
}

fn phase(receipt: String) -> String {
    serde_json::from_str::<Value>(&receipt).unwrap()["phase"]
        .as_str()
        .unwrap()
        .to_owned()
}

type Snapshot = BTreeMap<String, (FileIdentity, Option<Vec<u8>>)>;

fn snapshot(samples: &Path, include_projection: bool) -> Snapshot {
    let mut files = BTreeMap::new();
    let mut directories = vec![samples.to_owned()];
    files.insert(String::new(), (identity(samples).unwrap(), None));
    while let Some(directory) = directories.pop() {
        for child in fs::read_dir(directory).unwrap() {
            let path = child.unwrap().path();
            let relative = path
                .strip_prefix(samples)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if !include_projection && relative == PROJECTION {
                continue;
            }
            let bytes = if path.is_dir() {
                directories.push(path.clone());
                None
            } else {
                Some(fs::read(&path).unwrap())
            };
            files.insert(relative, (identity(&path).unwrap(), bytes));
        }
    }
    files
}

fn inventory(samples: &Path) -> Vec<Value> {
    let lease = inventory_gate(samples).unwrap();
    lease
        .records_native()
        .unwrap()
        .iter()
        .map(|record| serde_json::from_str(record).unwrap())
        .collect()
}

fn assert_failed_move_preserves(fixture: &Fixture) {
    let before = snapshot(&fixture.source, true);
    let record = fs::read(&fixture.record).unwrap();
    let seal = fs::read(capture_stream(&fixture.source)).unwrap();
    assert!(fixture.act("move").is_err());
    assert_eq!(snapshot(&fixture.source, true), before);
    assert_eq!(fs::read(&fixture.record).unwrap(), record);
    assert_eq!(fs::read(capture_stream(&fixture.source)).unwrap(), seal);
    assert!(!fixture.target.exists());
}

#[test]
fn legacy_move_recover_and_rollback_preserve_real_ids_bytes_and_pending_history() {
    let fixture = Fixture::new(true);
    let original_reference = inventory(&fixture.source)[0]["config_reference"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = snapshot(&fixture.source, false);
    let root_identity = identity(&fixture.root).unwrap();
    fixture.move_samples();
    assert!(!fixture.source.exists());
    assert_eq!(identity(&fixture.root).unwrap(), root_identity);
    assert_eq!(snapshot(&fixture.target, false), before);
    assert!(verified_projection(&fixture.target).unwrap().is_some());
    let records = inventory(&fixture.target);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["instance_id"], fixture.instance);
    assert_eq!(records[0]["live"], false);
    assert!(records[0]["error"].is_null());
    assert_eq!(records[0]["config_reference"], original_reference);
    let moved = snapshot(&fixture.target, true);
    for _ in 0..2 {
        assert_eq!(phase(fixture.act("recover").unwrap()), "moved");
        assert_eq!(snapshot(&fixture.target, true), moved);
    }
    assert_eq!(phase(fixture.act("rollback").unwrap()), "rolled_back");
    assert_eq!(snapshot(&fixture.source, true), before);
    assert!(!fixture.target.exists());
    assert!(verified_projection(&fixture.source).unwrap().is_none());
    assert!(!capture_stream(&fixture.source).exists());
    // A terminal rollback must be repeatable without recreating authority.
    assert_eq!(phase(fixture.act("rollback").unwrap()), "rolled_back");
    assert_eq!(snapshot(&fixture.source, true), before);
}

#[test]
fn rollback_before_any_move_is_idempotent_and_preserves_original_samples() {
    let fixture = Fixture::new(true);
    let before = snapshot(&fixture.source, false);
    fixture.capture();
    for _ in 0..2 {
        assert_eq!(phase(fixture.act("rollback").unwrap()), "rolled_back");
        assert_eq!(snapshot(&fixture.source, true), before);
        assert!(!fixture.target.exists());
    }
}

#[test]
fn recover_finishes_actual_moving_phase_before_and_after_physical_rename() {
    for renamed in [false, true] {
        let fixture = Fixture::new(true);
        let before = snapshot(&fixture.source, false);
        fixture.capture();
        // Reproduce the two durable crash boundaries using the actual Native
        // record write and directory-handle rename, without inventing a seal.
        let mut capture = load_record(&fixture.record).unwrap();
        capture.phase = "moving".into();
        atomic_record(&fixture.record, &capture, false).unwrap();
        if renamed {
            let handle = move_guard(&fixture.source).unwrap();
            rename_guard(&handle, &fixture.target).unwrap();
            drop(handle);
        }
        assert_eq!(phase(fixture.act("recover").unwrap()), "moved");
        assert!(!fixture.source.exists());
        assert_eq!(snapshot(&fixture.target, false), before);
        assert!(verified_projection(&fixture.target).unwrap().is_some());
        assert_eq!(phase(fixture.act("recover").unwrap()), "moved");
        assert_eq!(snapshot(&fixture.target, false), before);
    }
}

#[test]
fn moved_history_supports_new_native_registration_and_later_atomic_config_save() {
    let fixture = Fixture::new(true);
    fixture.move_samples();
    let current = fixture.target.join(CONFIG);
    let historical = plain_path(&fixture.source.join(CONFIG));
    assert!(config_location::matches(&fixture.target, &current, &historical).unwrap());
    assert!(
        !config_location::matches(
            &fixture.target,
            &fixture.target.join("projects/same.json"),
            &historical,
        )
        .unwrap()
    );
    let different_parent = fixture.target.join("another/session.json");
    fs::create_dir(different_parent.parent().unwrap()).unwrap();
    fs::write(&different_parent, fs::read(&current).unwrap()).unwrap();
    assert!(!config_location::matches(&fixture.target, &different_parent, &historical).unwrap());
    let old_identity = identity(&current).unwrap();
    let next = fixture.target.join("projects/atomic-save.json");
    let newer = br#"{"config_revision":2,"sample_paths":[],"stem_cache":[]}"#;
    fs::write(&next, newer).unwrap();
    fs::rename(&next, &current).unwrap();
    assert_ne!(identity(&current).unwrap(), old_identity);
    assert_eq!(fs::read(&current).unwrap(), newer);
    assert_eq!(
        config_location::resolve(&fixture.target, Path::new(&historical)).unwrap(),
        fs::canonicalize(&current).unwrap()
    );
    let mut guard = MigrationProjectGuard::create_native(&fixture.target, &current).unwrap();
    let fresh = guard.instance_id();
    assert_ne!(fresh, fixture.instance);
    let records = inventory(&fixture.target);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["instance_id"], fresh);
    assert_eq!(records[0]["live"], true);
    assert!(records[0]["error"].is_null());
    assert_eq!(
        records[0]["config_reference"],
        plain_path(&fs::canonicalize(&current).unwrap())
    );
    guard.release();
    assert_eq!(inventory(&fixture.target)[0]["live"], false);
    let journal =
        MaterialMigrationJournalStore::open(plain_path(&fixture.target), TRANSACTION.into())
            .unwrap();
    let history = journal.history().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(&history[1]).unwrap()["config_reference"],
        historical
    );
    assert!(config_location::matches(&fixture.target, &current, &historical).unwrap());
}

#[test]
fn captured_current_samples_survive_checkout_backup_move_and_restore_previous_projection() {
    let fixture = Fixture::new(true);
    fixture.move_samples();
    let before = snapshot(&fixture.target, true);
    let previous_anchor = fs::read(stream_path(&fixture.target)).unwrap();
    let previous_seal = fs::read(capture_stream(&fixture.target)).unwrap();
    let second = fixture
        .root
        .join("cache")
        .join(format!("relocation-{SECOND_OPERATION}.json"));
    run(
        &fixture.root,
        &fixture.target,
        &fixture.target,
        &second,
        "capture",
    )
    .unwrap();
    let checkout = fixture.root.join("flitzis-looper");
    let backup = fixture.root.join("backup");
    fs::rename(&checkout, &backup).unwrap();
    fs::create_dir(&checkout).unwrap();
    let alias = backup.join("samples");
    assert_eq!(
        phase(run(&fixture.root, &alias, &fixture.target, &second, "move").unwrap()),
        "moved"
    );
    assert_eq!(snapshot(&fixture.target, true), before);
    assert!(
        config_location::matches(
            &fixture.target,
            &fixture.target.join(CONFIG),
            &plain_path(&fixture.source.join(CONFIG)),
        )
        .unwrap()
    );
    for _ in 0..2 {
        assert_eq!(
            phase(run(&fixture.root, &alias, &fixture.target, &second, "recover").unwrap()),
            "moved"
        );
    }
    assert_eq!(
        phase(run(&fixture.root, &alias, &fixture.target, &second, "rollback").unwrap()),
        "rolled_back"
    );
    assert_eq!(snapshot(&alias, true), before);
    assert_eq!(fs::read(stream_path(&alias)).unwrap(), previous_anchor);
    assert_eq!(fs::read(capture_stream(&alias)).unwrap(), previous_seal);
    assert!(verified_projection(&alias).unwrap().is_some());
    assert_eq!(
        phase(run(&fixture.root, &alias, &fixture.target, &second, "rollback").unwrap()),
        "rolled_back"
    );
    assert_eq!(snapshot(&alias, true), before);
}

#[test]
fn whole_portable_root_rename_preserves_historical_and_final_config_locations() {
    let fixture = Fixture::new(true);
    let legacy = inventory(&fixture.source)[0]["config_reference"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.move_samples();
    let before = snapshot(&fixture.target, true);
    let previous_final = plain_path(&fixture.target.join(CONFIG));
    let root_identity = identity(&fixture.root).unwrap();
    let renamed = fixture._temporary.path().join("RenamedPortableRoot");
    fs::rename(&fixture.root, &renamed).unwrap();
    let samples = renamed.join("flitzis-looper/samples");
    let config = samples.join(CONFIG);
    assert_eq!(identity(&renamed).unwrap(), root_identity);
    assert_eq!(snapshot(&samples, true), before);
    assert!(verified_projection(&samples).unwrap().is_some());
    for historical in [&legacy, &previous_final] {
        assert!(config_location::matches(&samples, &config, historical).unwrap());
        assert!(
            !config_location::matches(&samples, &samples.join("projects/same.json"), historical,)
                .unwrap()
        );
    }
    let records = inventory(&samples);
    assert_eq!(records.len(), 1);
    assert!(records[0]["error"].is_null());
    assert_eq!(records[0]["config_reference"], legacy);
    let mut guard = MigrationProjectGuard::create_native(&samples, &config).unwrap();
    assert_eq!(inventory(&samples)[0]["live"], true);
    guard.release();
    assert_eq!(inventory(&samples)[0]["live"], false);
}

#[test]
fn actual_native_directory_and_inventory_guards_block_capture_and_checkout_rename() {
    for kind in ["samples", "config", "inventory", "project"] {
        let fixture = Fixture::new(false);
        let mut project = None;
        let mut lease = None;
        let guards = match kind {
            "samples" => project_assets::directory_guards(&fixture.source).unwrap(),
            "config" => project_assets::directory_guards(&fixture.source.join("projects")).unwrap(),
            "inventory" => {
                lease = Some(inventory_gate(&fixture.source).unwrap());
                Vec::new()
            }
            "project" => {
                project = Some(
                    MigrationProjectGuard::create_native(
                        &fixture.source,
                        &fixture.source.join(CONFIG),
                    )
                    .unwrap(),
                );
                Vec::new()
            }
            _ => unreachable!(),
        };
        assert!(
            fixture.act("capture").is_err(),
            "held {kind} guard admitted capture"
        );
        assert!(!fixture.record.exists());
        assert!(!capture_stream(&fixture.source).exists());
        let backup = fixture.root.join("backup");
        assert!(
            fs::rename(fixture.target.parent().unwrap(), &backup).is_err(),
            "held {kind} guard admitted checkout rename"
        );
        assert!(!backup.exists());
        assert!(fixture.source.join(CONFIG).is_file());
        assert!(fixture.journal(&fixture.source, 1).is_file());
        drop(project.take());
        drop(lease.take());
        drop(guards);
        fixture.capture();
    }
}

#[test]
fn a_real_fresh_project_guard_after_capture_prevents_the_move() {
    let fixture = Fixture::new(true);
    fixture.capture();
    let mut guard =
        MigrationProjectGuard::create_native(&fixture.source, &fixture.source.join(CONFIG))
            .unwrap();
    assert_failed_move_preserves(&fixture);
    assert_eq!(inventory(&fixture.source)[0]["live"], true);
    guard.release();
    // Re-registration changed the captured inventory. Releasing the live guard
    // cannot make that old capture authorize the new tree.
    assert_failed_move_preserves(&fixture);
}

#[test]
fn root_delete_custody_blocks_new_native_guards_after_own_inventory_is_dropped() {
    let fixture = Fixture::new(true);
    let before = snapshot(&fixture.source, true);
    let root_guard = move_guard(&fixture.source).unwrap();
    let own_inventory = inventory_lock(&fixture.source).unwrap().unwrap();
    drop(own_inventory);
    assert!(inventory_gate(&fixture.source).is_err());
    assert!(
        MigrationProjectGuard::create_native(&fixture.source, &fixture.source.join(CONFIG))
            .is_err()
    );
    assert_eq!(snapshot(&fixture.source, true), before);
    assert!(!fixture.target.exists());
    drop(root_guard);
    let mut ordinary_inventory = inventory_gate(&fixture.source).unwrap();
    let records = ordinary_inventory.records_native().unwrap();
    assert_eq!(records.len(), 1);
    let record: Value = serde_json::from_str(&records[0]).unwrap();
    assert_eq!(record["instance_id"], fixture.instance);
    assert_eq!(record["live"], false);
    assert!(record["error"].is_null());
    ordinary_inventory.release();
    drop(ordinary_inventory);
    assert_eq!(snapshot(&fixture.source, true), before);
}

#[test]
fn changed_same_size_bytes_and_equal_bytes_with_new_file_id_reject_move() {
    for replace in [false, true] {
        let fixture = Fixture::new(true);
        fixture.capture();
        let path = fixture.source.join(ORIGINAL);
        let previous = identity(&path).unwrap();
        if replace {
            let next = fixture.source.join("replacement.wav");
            fs::write(&next, fs::read(&path).unwrap()).unwrap();
            fs::rename(&next, &path).unwrap();
            assert_ne!(identity(&path).unwrap(), previous);
        } else {
            fs::write(&path, b"two original").unwrap();
            assert_eq!(identity(&path).unwrap(), previous);
        }
        assert_failed_move_preserves(&fixture);
    }
}

#[test]
fn forged_external_capture_binding_is_rejected_by_actual_samples_ads_seal() {
    let fixture = Fixture::new(true);
    fixture.capture();
    let external = fixture.root.join("external.json");
    fs::write(&external, fs::read(fixture.source.join(CONFIG)).unwrap()).unwrap();
    let mut capture: Value = serde_json::from_slice(&fs::read(&fixture.record).unwrap()).unwrap();
    capture["bindings"][0]["config_reference"] = json!(plain_path(&external));
    capture["bindings"][0]["config_path"] = json!("../../external.json");
    capture["bindings"][0]["config_identity"] =
        serde_json::to_value(identity(&external).unwrap()).unwrap();
    fs::write(&fixture.record, capture.to_string()).unwrap();
    assert_failed_move_preserves(&fixture);
    assert_eq!(
        fs::read(&external).unwrap(),
        fs::read(fixture.source.join(CONFIG)).unwrap()
    );
}

#[test]
fn projection_tamper_replacement_and_copied_custody_fail_closed() {
    let fixture = Fixture::new(true);
    fixture.move_samples();
    let projection = fixture.target.join(PROJECTION);
    let bytes = fs::read(&projection).unwrap();
    let anchor = fs::read(stream_path(&fixture.target)).unwrap();
    let historical = plain_path(&fixture.source.join(CONFIG));
    let mut tampered: Value = serde_json::from_slice(&bytes).unwrap();
    tampered["bindings"][0]["config_path"] = json!("projects/same.json");
    fs::write(&projection, tampered.to_string()).unwrap();
    assert!(verified_projection(&fixture.target).is_err());
    assert!(config_location::resolve(&fixture.target, Path::new(&historical)).is_err());
    fs::write(&projection, &bytes).unwrap();
    assert!(verified_projection(&fixture.target).unwrap().is_some());
    let copied = fixture.root.join("copied/samples");
    for (relative, (_, contents)) in snapshot(&fixture.target, true) {
        let destination = copied.join(relative);
        if let Some(contents) = contents {
            fs::write(destination, contents).unwrap();
        } else {
            fs::create_dir_all(destination).unwrap();
        }
    }
    // Raw JSON alone supplies no alias authority.
    assert!(verified_projection(&copied).is_err());
    fs::write(stream_path(&copied), &anchor).unwrap();
    fs::write(
        capture_stream(&copied),
        fs::read(capture_stream(&fixture.target)).unwrap(),
    )
    .unwrap();
    // Even exact copied anchor/projection bytes cannot assert the new directory's IDs.
    assert!(verified_projection(&copied).is_err());
    assert!(config_location::resolve(&copied, Path::new(&historical)).is_err());
    let next = fixture.target.join("replacement-projection.json");
    fs::write(&next, &bytes).unwrap();
    let old = identity(&projection).unwrap();
    fs::rename(&next, &projection).unwrap();
    assert_ne!(identity(&projection).unwrap(), old);
    assert!(verified_projection(&fixture.target).is_err());
    assert!(fixture.target.join(ORIGINAL).is_file());
    assert!(copied.join(ORIGINAL).is_file());
}

#[test]
fn replacing_bound_config_parent_with_equal_bytes_does_not_inherit_location() {
    let fixture = Fixture::new(true);
    fixture.move_samples();
    let parent = fixture.target.join("projects");
    let contents = fs::read(parent.join("session.json")).unwrap();
    let previous_identity = identity(&parent).unwrap();
    fs::rename(&parent, fixture.target.join("retained-projects")).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::write(parent.join("session.json"), &contents).unwrap();
    fs::write(parent.join("same.json"), &contents).unwrap();
    assert_ne!(identity(&parent).unwrap(), previous_identity);
    assert!(verified_projection(&fixture.target).unwrap().is_some());
    let before = snapshot(&fixture.target, true);
    assert!(config_location::resolve(&fixture.target, &fixture.source.join(CONFIG)).is_err());
    assert_eq!(snapshot(&fixture.target, true), before);
    assert_eq!(fs::read(parent.join("session.json")).unwrap(), contents);
}

#[test]
fn unknown_inventory_gapped_history_and_unknown_journal_fields_block_capture() {
    for kind in ["inventory", "gap", "fields"] {
        let fixture = Fixture::new(true);
        match kind {
            "inventory" => fs::write(
                fixture.source.join(".migration-projects/unknown.json"),
                b"{}",
            )
            .unwrap(),
            "gap" => fs::rename(
                fixture.journal(&fixture.source, 1),
                fixture.journal(&fixture.source, 2),
            )
            .unwrap(),
            "fields" => {
                let path = fixture.journal(&fixture.source, 1);
                let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                record["unknown"] = json!(true);
                fs::write(path, record.to_string()).unwrap();
            }
            _ => unreachable!(),
        }
        let before = snapshot(&fixture.source, true);
        assert!(
            fixture.act("capture").is_err(),
            "unrecognized {kind} was admitted"
        );
        assert_eq!(snapshot(&fixture.source, true), before);
        assert!(!fixture.record.exists());
        assert!(!capture_stream(&fixture.source).exists());
        assert!(!fixture.target.exists());
    }
}

#[test]
fn known_external_config_stops_before_capture_without_omitting_its_registration() {
    let fixture = Fixture::new(true);
    let external = fixture.source.parent().unwrap().join("external.json");
    fs::write(&external, b"{}").unwrap();
    let mut guard = MigrationProjectGuard::create_native(&fixture.source, &external).unwrap();
    guard.release();
    let records = inventory(&fixture.source);
    assert_eq!(records.len(), 2);
    assert!(
        records.iter().any(|record| record["config_reference"]
            == plain_path(&fs::canonicalize(&external).unwrap()))
    );
    let before = snapshot(&fixture.source, true);
    assert!(fixture.act("capture").is_err());
    assert_eq!(snapshot(&fixture.source, true), before);
    assert_eq!(inventory(&fixture.source), records);
    assert_eq!(fs::read(external).unwrap(), b"{}");
    assert!(!fixture.record.exists());
    assert!(!fixture.target.exists());
}

#[test]
fn real_reparse_child_and_occupied_destination_are_preserved() {
    let fixture = Fixture::new(true);
    let foreign = fixture.root.join("foreign");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("untouched.wav"), b"foreign").unwrap();
    let junction = fixture.source.join("linked");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&foreign)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "actual contained junction creation failed"
    );
    assert!(fixture.act("capture").is_err());
    assert_eq!(fs::read(foreign.join("untouched.wav")).unwrap(), b"foreign");
    assert!(!fixture.record.exists());
    fs::remove_dir(&junction).unwrap();
    fixture.capture();
    fs::create_dir(&fixture.target).unwrap();
    fs::write(fixture.target.join("collision.wav"), b"occupied").unwrap();
    let before = snapshot(&fixture.source, true);
    assert!(fixture.act("move").is_err());
    assert_eq!(snapshot(&fixture.source, true), before);
    assert_eq!(
        fs::read(fixture.target.join("collision.wav")).unwrap(),
        b"occupied"
    );
    assert_eq!(fs::read(foreign.join("untouched.wav")).unwrap(), b"foreign");
}

#[test]
fn occupied_rollback_destination_preserves_moved_samples_and_both_custody_streams() {
    let fixture = Fixture::new(true);
    fixture.move_samples();
    fs::create_dir(&fixture.source).unwrap();
    fs::write(fixture.source.join("collision.wav"), b"occupied").unwrap();
    let source = snapshot(&fixture.source, true);
    let target = snapshot(&fixture.target, true);
    let anchor = fs::read(stream_path(&fixture.target)).unwrap();
    let seal = fs::read(capture_stream(&fixture.target)).unwrap();
    let record = fs::read(&fixture.record).unwrap();
    assert!(fixture.act("rollback").is_err());
    assert_eq!(snapshot(&fixture.source, true), source);
    assert_eq!(snapshot(&fixture.target, true), target);
    assert_eq!(fs::read(stream_path(&fixture.target)).unwrap(), anchor);
    assert_eq!(fs::read(capture_stream(&fixture.target)).unwrap(), seal);
    assert_eq!(fs::read(&fixture.record).unwrap(), record);
    assert!(verified_projection(&fixture.target).unwrap().is_some());
}

fn unflushed_installer_flag(root: &Path, operation: &str) -> (PathBuf, Vec<u8>) {
    let path = root.join("portable-transaction.json");
    // The actual control fields at this interruption boundary. Native does not
    // consume this file; this is not a complete PowerShell recovery fixture.
    let bytes = serde_json::to_vec(&json!({
        "schema_version":1,
        "operation":operation,
        "git_commit":"0".repeat(40),
        "phase":"captured",
        "capture_completed":false
    }))
    .unwrap();
    fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

#[test]
fn sealed_unmoved_capture_can_be_superseded_before_installer_flag_flush() {
    for failed_record in [false, true] {
        let fixture = Fixture::new(true);
        let original_tree = snapshot(&fixture.source, false);
        let (flag, flag_bytes) = unflushed_installer_flag(&fixture.root, OPERATION);
        fixture.capture();
        let previous = load_record(&fixture.record).unwrap();
        let previous_seal: CaptureSeal =
            serde_json::from_slice(&fs::read(capture_stream(&fixture.source)).unwrap()).unwrap();
        assert!(!previous_seal.complete);
        assert_eq!(previous.phase, "captured");
        assert_eq!(
            previous.source,
            plain_path(&fs::canonicalize(&fixture.source).unwrap())
        );
        verify_capture_seal(&fixture.source, &fixture.record, &previous).unwrap();
        let preserved = if failed_record {
            let path = fixture
                .root
                .join("cache")
                .join(format!("failed-relocation-{OPERATION}.json"));
            // PowerShell's false-flag recovery preserves the external record,
            // while no Samples or checkout move has occurred.
            fs::rename(&fixture.record, &path).unwrap();
            path
        } else {
            fixture.record.clone()
        };
        let preserved_bytes = fs::read(&preserved).unwrap();
        let preserved_identity = identity(&preserved).unwrap();
        let fresh = fixture
            .root
            .join("cache")
            .join(format!("relocation-{SECOND_OPERATION}.json"));
        assert_eq!(
            phase(
                run(
                    &fixture.root,
                    &fixture.source,
                    &fixture.target,
                    &fresh,
                    "capture"
                )
                .unwrap()
            ),
            "captured"
        );
        assert_eq!(snapshot(&fixture.source, false), original_tree);
        assert_eq!(fs::read(&flag).unwrap(), flag_bytes);
        assert_eq!(fs::read(&preserved).unwrap(), preserved_bytes);
        assert_eq!(identity(&preserved).unwrap(), preserved_identity);
        let current = load_record(&fresh).unwrap();
        let current_seal: CaptureSeal =
            serde_json::from_slice(&fs::read(capture_stream(&fixture.source)).unwrap()).unwrap();
        assert!(!current_seal.complete);
        assert_eq!(
            current_seal.record_name,
            fresh.file_name().unwrap().to_string_lossy().into_owned()
        );
        assert_eq!(
            current_seal.payload_sha256,
            capture_digest(&current).unwrap()
        );
        assert_eq!(current.source, previous.source);
        assert!(current.previous_capture.is_none());
        let seal = fs::read(capture_stream(&fixture.source)).unwrap();
        assert!(fixture.act("move").is_err());
        assert_eq!(snapshot(&fixture.source, false), original_tree);
        assert_eq!(fs::read(capture_stream(&fixture.source)).unwrap(), seal);
        assert_eq!(
            phase(
                run(
                    &fixture.root,
                    &fixture.source,
                    &fixture.target,
                    &fresh,
                    "move"
                )
                .unwrap()
            ),
            "moved"
        );
        assert_eq!(snapshot(&fixture.target, false), original_tree);
        assert!(verified_projection(&fixture.target).unwrap().is_some());
        assert_eq!(fs::read(&preserved).unwrap(), preserved_bytes);
    }
}

#[test]
fn superseding_pending_capture_retains_the_earlier_completed_custody_for_rollback() {
    let fixture = Fixture::new(true);
    fixture.move_samples();
    let before = snapshot(&fixture.target, true);
    let anchor = fs::read(stream_path(&fixture.target)).unwrap();
    let complete_seal = fs::read_to_string(capture_stream(&fixture.target)).unwrap();
    let pending = fixture
        .root
        .join("cache")
        .join(format!("relocation-{SECOND_OPERATION}.json"));
    let (flag, flag_bytes) = unflushed_installer_flag(&fixture.root, SECOND_OPERATION);
    run(
        &fixture.root,
        &fixture.target,
        &fixture.target,
        &pending,
        "capture",
    )
    .unwrap();
    let seal: CaptureSeal =
        serde_json::from_slice(&fs::read(capture_stream(&fixture.target)).unwrap()).unwrap();
    assert!(!seal.complete);
    let preserved = fixture
        .root
        .join("cache")
        .join(format!("failed-relocation-{SECOND_OPERATION}.json"));
    fs::rename(&pending, &preserved).unwrap();
    let preserved_bytes = fs::read(&preserved).unwrap();
    let fresh = fixture
        .root
        .join("cache")
        .join(format!("relocation-{}.json", "d".repeat(32)));
    assert_eq!(
        phase(
            run(
                &fixture.root,
                &fixture.target,
                &fixture.target,
                &fresh,
                "capture"
            )
            .unwrap()
        ),
        "captured"
    );
    assert_eq!(snapshot(&fixture.target, true), before);
    assert_eq!(
        load_record(&fresh).unwrap().previous_capture,
        Some(complete_seal.clone())
    );
    for _ in 0..2 {
        assert_eq!(
            phase(
                run(
                    &fixture.root,
                    &fixture.target,
                    &fixture.target,
                    &fresh,
                    "rollback"
                )
                .unwrap()
            ),
            "rolled_back"
        );
        assert_eq!(snapshot(&fixture.target, true), before);
        assert_eq!(fs::read(stream_path(&fixture.target)).unwrap(), anchor);
        assert_eq!(
            fs::read_to_string(capture_stream(&fixture.target)).unwrap(),
            complete_seal
        );
        assert_eq!(fs::read(&preserved).unwrap(), preserved_bytes);
        assert_eq!(fs::read(&flag).unwrap(), flag_bytes);
    }
}

#[test]
fn false_capture_seal_rejects_missing_forged_tampered_moving_or_relocated_predecessor() {
    for kind in ["missing", "forged", "tampered", "moving", "relocated"] {
        let fixture = Fixture::new(true);
        let (flag, flag_bytes) = unflushed_installer_flag(&fixture.root, OPERATION);
        fixture.capture();
        let mut source = fixture.source.clone();
        match kind {
            "missing" => fs::remove_file(&fixture.record).unwrap(),
            "forged" => {
                let preserved = fixture.root.join("cache").join("unsealed-diagnostic.json");
                fs::rename(&fixture.record, &preserved).unwrap();
                let mut claimed: Value =
                    serde_json::from_slice(&fs::read(&preserved).unwrap()).unwrap();
                claimed["entries"] = json!([]);
                claimed["bindings"] = json!([]);
                let fallback = fixture
                    .root
                    .join("cache")
                    .join(format!("failed-relocation-{OPERATION}.json"));
                fs::write(fallback, claimed.to_string()).unwrap();
            }
            "tampered" => {
                let mut previous: Value =
                    serde_json::from_slice(&fs::read(&fixture.record).unwrap()).unwrap();
                previous["bindings"][0]["config_path"] = json!("projects/same.json");
                fs::write(&fixture.record, previous.to_string()).unwrap();
            }
            "moving" => {
                let mut previous = load_record(&fixture.record).unwrap();
                previous.phase = "moving".into();
                atomic_record(&fixture.record, &previous, false).unwrap();
            }
            "relocated" => {
                let backup = fixture.root.join("backup");
                fs::rename(fixture.source.parent().unwrap(), &backup).unwrap();
                source = backup.join("samples");
            }
            _ => unreachable!(),
        }
        let tree = snapshot(&source, true);
        let records = snapshot(&fixture.root.join("cache"), true);
        let seal = fs::read(capture_stream(&source)).unwrap();
        let fresh = fixture
            .root
            .join("cache")
            .join(format!("relocation-{SECOND_OPERATION}.json"));
        assert!(
            run(&fixture.root, &source, &fixture.target, &fresh, "capture").is_err(),
            "{kind} predecessor granted recapture"
        );
        assert_eq!(snapshot(&source, true), tree);
        assert_eq!(snapshot(&fixture.root.join("cache"), true), records);
        assert_eq!(fs::read(capture_stream(&source)).unwrap(), seal);
        assert_eq!(fs::read(&flag).unwrap(), flag_bytes);
        assert!(!fresh.exists());
        assert!(!fixture.target.exists());
    }
}

#[test]
fn projection_capacity_kernel_rejects_oversized_metadata_and_excess_final_aliases() {
    let fixture = Fixture::new(true);
    fixture.capture();
    let mut capture = load_record(&fixture.record).unwrap();
    let binding = capture.bindings[0].clone();
    assert!(projection_bytes(&fixture.source, &capture).is_ok());
    let tree = snapshot(&fixture.source, true);
    let record = fs::read(&fixture.record).unwrap();
    let seal = fs::read(capture_stream(&fixture.source)).unwrap();
    // This is only the bounded serialization kernel. These synthetic references
    // are not authenticated bindings or an actual config admission fixture.
    capture.bindings = (0..512)
        .map(|index| {
            let mut candidate = binding.clone();
            candidate.config_reference = format!("C:/history/{}/{index}.json", "h".repeat(768));
            candidate
        })
        .collect();
    assert!(
        projection_bytes(&fixture.source, &capture)
            .unwrap_err()
            .to_string()
            .contains("metadata bound")
    );
    capture.bindings = (0..1024)
        .map(|index| {
            let mut candidate = binding.clone();
            candidate.config_reference = format!("C:/history/{index}.json");
            candidate
        })
        .collect();
    assert!(
        projection_bytes(&fixture.source, &capture)
            .unwrap_err()
            .to_string()
            .contains("binding capacity")
    );
    assert_eq!(snapshot(&fixture.source, true), tree);
    assert_eq!(fs::read(&fixture.record).unwrap(), record);
    assert_eq!(fs::read(capture_stream(&fixture.source)).unwrap(), seal);
}

fn flush_pending_update(record: &Path, capture: &Capture) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(record.with_extension("pending"))
        .unwrap();
    file.write_all(&serde_json::to_vec(capture).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}

#[test]
fn recover_commits_flushed_moving_update_before_actual_native_move() {
    let fixture = Fixture::new(true);
    let original = snapshot(&fixture.source, false);
    fixture.capture();
    let record = fs::read(&fixture.record).unwrap();
    let mut pending = load_record(&fixture.record).unwrap();
    pending.phase = "moving".into();
    flush_pending_update(&fixture.record, &pending);
    assert_eq!(fs::read(&fixture.record).unwrap(), record);
    assert_eq!(phase(fixture.act("recover").unwrap()), "moved");
    assert!(!fixture.record.with_extension("pending").exists());
    assert_eq!(load_record(&fixture.record).unwrap().phase, "moved");
    assert!(!fixture.source.exists());
    assert_eq!(snapshot(&fixture.target, false), original);
    assert!(verified_projection(&fixture.target).unwrap().is_some());
    assert_eq!(phase(fixture.act("recover").unwrap()), "moved");
    assert_eq!(snapshot(&fixture.target, false), original);
}

#[test]
fn recover_commits_flushed_moved_update_after_real_rename_and_own_projection() {
    let fixture = Fixture::new(true);
    let original = snapshot(&fixture.source, false);
    fixture.capture();
    let mut pending = load_record(&fixture.record).unwrap();
    pending.phase = "moving".into();
    atomic_record(&fixture.record, &pending, false).unwrap();
    let guard = move_guard(&fixture.source).unwrap();
    rename_guard(&guard, &fixture.target).unwrap();
    write_projection(&fixture.target, &pending).unwrap();
    drop(guard);
    assert!(verified_projection(&fixture.target).unwrap().is_some());
    let moved = snapshot(&fixture.target, true);
    pending.phase = "moved".into();
    flush_pending_update(&fixture.record, &pending);
    assert_eq!(load_record(&fixture.record).unwrap().phase, "moving");
    assert_eq!(phase(fixture.act("recover").unwrap()), "moved");
    assert!(!fixture.record.with_extension("pending").exists());
    assert_eq!(load_record(&fixture.record).unwrap().phase, "moved");
    assert_eq!(snapshot(&fixture.target, false), original);
    assert_eq!(snapshot(&fixture.target, true), moved);
    let seal: CaptureSeal =
        serde_json::from_slice(&fs::read(capture_stream(&fixture.target)).unwrap()).unwrap();
    assert!(seal.complete);
}

#[test]
fn recover_commits_flushed_rollback_update_and_finishes_terminal_seal_cleanup() {
    let fixture = Fixture::new(true);
    let original = snapshot(&fixture.source, false);
    fixture.move_samples();
    let mut pending = load_record(&fixture.record).unwrap();
    let guard = move_guard(&fixture.target).unwrap();
    rename_guard(&guard, &fixture.source).unwrap();
    restore_projection(&fixture.source, &pending).unwrap();
    drop(guard);
    pending.phase = "rolled_back".into();
    flush_pending_update(&fixture.record, &pending);
    assert_eq!(load_record(&fixture.record).unwrap().phase, "moved");
    assert_eq!(phase(fixture.act("recover").unwrap()), "rolled_back");
    assert!(!fixture.record.with_extension("pending").exists());
    assert_eq!(load_record(&fixture.record).unwrap().phase, "rolled_back");
    assert_eq!(snapshot(&fixture.source, true), original);
    assert!(!fixture.target.exists());
    assert!(!capture_stream(&fixture.source).exists());
    assert!(verified_projection(&fixture.source).unwrap().is_none());
    assert_eq!(phase(fixture.act("recover").unwrap()), "rolled_back");
    assert_eq!(phase(fixture.act("rollback").unwrap()), "rolled_back");
    assert_eq!(snapshot(&fixture.source, true), original);
}

#[test]
fn unknown_pending_updates_and_changed_native_custody_preserve_every_file() {
    for kind in ["malformed", "payload", "phase", "tree", "seal"] {
        let fixture = Fixture::new(true);
        fixture.capture();
        let mut pending = load_record(&fixture.record).unwrap();
        pending.phase = "moving".into();
        match kind {
            "malformed" => fs::write(
                fixture.record.with_extension("pending"),
                b"unknown unfinished JSON",
            )
            .unwrap(),
            "payload" => {
                pending.bindings[0].config_path = "projects/same.json".into();
                flush_pending_update(&fixture.record, &pending);
            }
            "phase" => {
                pending.phase = "moved".into();
                flush_pending_update(&fixture.record, &pending);
            }
            "tree" => {
                flush_pending_update(&fixture.record, &pending);
                fs::write(fixture.source.join(ORIGINAL), b"two original").unwrap();
            }
            "seal" => {
                flush_pending_update(&fixture.record, &pending);
                let path = capture_stream(&fixture.source);
                let mut seal: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                seal["payload_sha256"] = json!("0".repeat(64));
                fs::write(path, seal.to_string()).unwrap();
            }
            _ => unreachable!(),
        }
        let original = snapshot(&fixture.source, true);
        let metadata = snapshot(&fixture.root.join("cache"), true);
        let seal = fs::read(capture_stream(&fixture.source)).unwrap();
        assert!(
            fixture.act("recover").is_err(),
            "{kind} pending update was admitted"
        );
        assert_eq!(snapshot(&fixture.source, true), original);
        assert_eq!(snapshot(&fixture.root.join("cache"), true), metadata);
        assert_eq!(fs::read(capture_stream(&fixture.source)).unwrap(), seal);
        assert!(!fixture.target.exists());
        assert!(fixture.record.with_extension("pending").is_file());
    }
}
