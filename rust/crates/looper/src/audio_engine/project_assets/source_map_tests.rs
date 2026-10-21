//! Exact native map cleanup and real registered-Config protection; no DSP claims.
#![cfg(windows)]

use super::*;
use serde_json::{Value, json};

const GENERATION: &str = ".ready-0123456789abcdef0123456789abcdef";

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    directory: PathBuf,
    leaf: PathBuf,
    receipt: Value,
    assets: Arc<ProjectAssets>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        let directory = root
            .join("Owned/.pcm-cache/source-beatmaps/v1")
            .join(GENERATION);
        fs::create_dir_all(&directory).unwrap();
        // Readable material resolution requires the same canonical immutable
        // source marker as production; a bare named folder is not a material.
        let original = b"owned exact map retirement original";
        fs::write(root.join("Owned/original.wav"), original).unwrap();
        fs::write(
            root.join("Owned/.material.json"),
            json!({"schema_version":1,
            "sha256":format!("{:x}",Sha256::digest(original)),"bytes":original.len(),
            "original_leaf":"original.wav"})
            .to_string(),
        )
        .unwrap();
        let leaf = directory.join("map.json");
        let bytes = b"immutable exact map retirement fixture";
        fs::write(&leaf, bytes).unwrap();
        let receipt = json!({"schema_version":1,"kind":"source-beatmap-v1",
            "reference":format!("samples/Owned/.pcm-cache/source-beatmaps/v1/{GENERATION}/map.json"),
            "bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes)),
            "map_revision":"a".repeat(64),"decoder_identity":"b".repeat(64)});
        Self {
            _temp: temp,
            root,
            directory,
            leaf,
            receipt,
            assets: ProjectAssets::isolated(),
        }
    }

    fn pin(&self) -> ProjectAssetLease {
        let pin = self.assets.acquire(&self.root, &self.leaf).unwrap();
        pin.bind_source_beatmap(self.receipt.to_string()).unwrap();
        pin
    }

    fn retire(&self, pin: &ProjectAssetLease) {
        self.assets
            .retire_bound_source_map(pin.owner.as_ref().unwrap())
            .unwrap();
        assert_eq!(self.assets.state.lock().unwrap().retiring.len(), 1);
        assert!(
            self.assets.map_retirement_pending_for_test(&self.leaf),
            "raw and canonical Windows spellings must observe the same actual queued proof"
        );
    }
}

#[test]
fn bound_map_receipt_is_idempotent_and_never_rebinds_after_metadata_aba() {
    let fixture = Fixture::new();
    let pin = fixture.pin();
    pin.bind_source_beatmap(fixture.receipt.to_string())
        .unwrap();
    for key in ["sha256", "map_revision", "decoder_identity"] {
        let mut changed = fixture.receipt.clone();
        changed[key] = json!("c".repeat(64));
        assert!(pin.bind_source_beatmap(changed.to_string()).is_err());
        assert_eq!(
            pin.owner
                .as_ref()
                .unwrap()
                .source_map
                .get()
                .unwrap()
                .receipt,
            super::super::source_beatmap_artifact::receipt_evidence(&fixture.receipt.to_string())
                .unwrap()
                .3
        );
    }
    let mut changed = fixture.receipt.clone();
    changed["bytes"] = json!(1);
    assert!(pin.bind_source_beatmap(changed.to_string()).is_err());
}

#[test]
fn missing_sidecar_logical_pin_does_not_claim_bound_evidence_or_change_existing_binding() {
    let fixture = Fixture::new();
    let held = fixture.pin();
    assert!(held.source_beatmap_bound());
    fs::remove_file(&fixture.leaf).unwrap();
    assert!(
        held.source_beatmap_bound(),
        "a cached binding is historical evidence, not a live existence claim"
    );
    let mut historical = fixture
        .assets
        .acquire(&fixture.root, &fixture.leaf)
        .unwrap();
    assert!(!historical.source_beatmap_bound());
    assert!(
        historical
            .bind_source_beatmap(fixture.receipt.to_string())
            .is_err()
    );
    assert!(!historical.source_beatmap_bound());
    fs::remove_dir(&fixture.directory).unwrap();
    let missing_generation = fixture
        .assets
        .acquire(&fixture.root, &fixture.leaf)
        .unwrap();
    assert!(!missing_generation.source_beatmap_bound());
    historical.release();
    assert!(!historical.source_beatmap_bound());
}

#[test]
fn transient_map_acquisition_keeps_exact_retirement_and_saved_ack_alone_cancels_it() {
    let fixture = Fixture::new();
    let mut first = fixture.pin();
    fixture.retire(&first);
    let mut history = fixture.pin();
    first.release();
    fixture.assets.collect();
    assert!(fixture.leaf.exists());
    assert!(
        fixture
            .assets
            .map_retirement_pending_for_test(&fixture.leaf)
    );
    fixture
        .assets
        .acknowledge_source_map(history.owner.as_ref().unwrap())
        .unwrap();
    fixture.assets.collect();
    assert!(
        !fixture
            .assets
            .map_retirement_pending_for_test(&fixture.leaf)
    );
    assert!(fixture.leaf.exists());
    fixture.retire(&history);
    history.release();
    fixture.assets.collect();
    assert!(!fixture.leaf.exists());
}

#[test]
fn history_directory_fallback_retains_queued_map_after_actual_leaf_admission_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let mut assigned = fixture.pin();
    fixture.retire(&assigned);
    let reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&fixture.leaf)
        .unwrap();
    assert!(
        fixture
            .assets
            .acquire(&fixture.root, &fixture.leaf)
            .is_err(),
        "actual exclusive handle prevents leaf pin admission"
    );
    let mut history = fixture
        .assets
        .acquire(&fixture.root, &fixture.directory)
        .unwrap();
    assigned.release();
    drop(reader);
    fixture.assets.collect();
    assert!(
        fixture.leaf.exists(),
        "transient generation pin protects real queued proof without a fabricated leaf receipt"
    );
    assert!(
        fixture
            .assets
            .map_retirement_pending_for_test(&fixture.leaf)
    );
    assert!(
        history
            .bind_source_beatmap(fixture.receipt.to_string())
            .is_err(),
        "directory retention never grants leaf/hash authority"
    );
    history.release();
    fixture.assets.collect();
    assert!(!fixture.directory.exists());
}

#[test]
fn saved_map_cleanup_waits_for_actual_windows_sharing_reader_and_retries_same_proof() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    let mut pin = fixture.pin();
    fixture.retire(&pin);
    let reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&fixture.leaf)
        .unwrap();
    pin.release();
    fixture.assets.collect();
    assert!(fixture.leaf.exists());
    assert!(
        fixture
            .assets
            .map_retirement_pending_for_test(&fixture.leaf)
    );
    drop(reader);
    {
        let mut state = fixture.assets.state.lock().unwrap();
        state
            .retiring
            .get_mut(&fs::canonicalize(&fixture.directory).unwrap())
            .unwrap()
            .map
            .as_mut()
            .unwrap()
            .next_attempt = Instant::now();
    }
    fixture.assets.collect();
    assert!(!fixture.directory.exists());
}

#[test]
fn changed_map_bytes_leaf_directory_and_unknown_children_are_preserved() {
    for change in ["bytes", "leaf", "directory", "unknown"] {
        let fixture = Fixture::new();
        let mut pin = fixture.pin();
        fixture.retire(&pin);
        pin.release();
        match change {
            "bytes" => fs::write(&fixture.leaf, b"changed bytes").unwrap(),
            "leaf" => {
                fs::rename(&fixture.leaf, fixture._temp.path().join("old-map.json")).unwrap();
                fs::write(&fixture.leaf, b"replacement leaf").unwrap();
            }
            "directory" => {
                fs::rename(
                    &fixture.directory,
                    fixture._temp.path().join("old-generation"),
                )
                .unwrap();
                fs::create_dir(&fixture.directory).unwrap();
                fs::write(&fixture.leaf, b"replacement directory").unwrap();
            }
            "unknown" => fs::write(fixture.directory.join("human.keep"), b"unknown child").unwrap(),
            _ => unreachable!(),
        }
        fixture.assets.collect();
        assert!(fixture.leaf.exists(), "changed {change} must survive");
        assert!(
            !fixture
                .assets
                .map_retirement_pending_for_test(&fixture.leaf)
        );
        assert!(!fixture.assets.status().unwrap().3.is_empty());
    }
}

#[test]
fn replaced_map_generation_junction_never_reaches_foreign_bytes() {
    let fixture = Fixture::new();
    let mut pin = fixture.pin();
    fixture.retire(&pin);
    pin.release();
    let foreign = fixture._temp.path().join("foreign-map");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("map.json"), b"foreign protected map").unwrap();
    fs::rename(
        &fixture.directory,
        fixture._temp.path().join("old-generation"),
    )
    .unwrap();
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&fixture.directory)
        .arg(&foreign)
        .output()
        .unwrap();
    assert!(output.status.success());
    fixture.assets.collect();
    assert_eq!(
        fs::read(foreign.join("map.json")).unwrap(),
        b"foreign protected map"
    );
    assert!(!fixture.assets.status().unwrap().3.is_empty());
    fs::remove_dir(&fixture.directory).unwrap();
}

#[test]
fn map_birth_reservation_shares_capacity_with_existing_retirement_queue() {
    let fixture = Fixture::new();
    let mut slots = Vec::new();
    for _ in 0..MAX_RETIREMENTS {
        slots.push(fixture.assets.reserve_map_retirement().unwrap());
    }
    assert!(fixture.assets.reserve_map_retirement().is_err());
    let mut pin = fixture.pin();
    assert!(
        fixture
            .assets
            .retire_bound_source_map(pin.owner.as_ref().unwrap())
            .is_err()
    );
    assert!(fixture.leaf.exists());
    slots.pop();
    fixture.retire(&pin);
    assert!(fixture.assets.reserve_map_retirement().is_err());
    pin.release();
    fixture.assets.collect();
    assert!(!fixture.leaf.exists());
    assert!(fixture.assets.reserve_map_retirement().is_ok());
    drop(slots);
    assert_eq!(fixture.assets.state.lock().unwrap().reserved_retirements, 0);
}

#[test]
fn another_registered_config_map_only_reference_protects_physical_artifact() {
    Python::initialize();
    let fixture = Fixture::new();
    let config = fixture.root.join("retained.json");
    let mut rows = vec![Value::Null; 216];
    rows[73] = json!({"schema_version":1,"revision":4,"selected":fixture.receipt});
    fs::write(
        &config,
        json!({"pad_source_beatmap_intent":rows}).to_string(),
    )
    .unwrap();
    let mut registration = Python::attach(|py| {
        super::super::material_migration_recovery::MigrationProjectGuard::new(
            py,
            fixture.root.to_string_lossy().into_owned(),
            config.to_string_lossy().into_owned(),
        )
        .unwrap()
    });
    let mut pin = fixture.pin();
    fixture.retire(&pin);
    pin.release();
    fixture.assets.collect();
    assert!(
        fixture.leaf.exists(),
        "map-only saved reference in another registered Config is authoritative retention"
    );
    assert!(
        fixture
            .assets
            .map_retirement_pending_for_test(&fixture.leaf)
    );
    rows[73] = Value::Null;
    fs::write(
        &config,
        json!({"pad_source_beatmap_intent":rows}).to_string(),
    )
    .unwrap();
    // No fresh owner or retirement is admitted after the delayed Config write.
    // The original queued proof alone must survive the registered reference and
    // retry after the worker's bounded backoff.
    std::thread::sleep(Duration::from_millis(1100));
    fixture.assets.collect();
    assert!(!fixture.leaf.exists());
    registration.release();
}
