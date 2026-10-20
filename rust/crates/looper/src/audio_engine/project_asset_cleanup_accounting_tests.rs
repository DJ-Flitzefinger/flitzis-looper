//! Real verified PCM queue handoff and physical deletion, isolated from other counters.
//!
//! Register as a child of material_migration_recovery::tests to reuse the actual
//! private recovery admission entry points, not manufactured retirement state.
#![cfg(windows)]

use super::*;
use crate::audio_engine::cold_store::{self, ColdTransaction};
use crate::audio_engine::{AudioEngine, material_migration};
use std::os::windows::fs::OpenOptionsExt;

const CHILD: &str = "FLITZI_PCM_CLEANUP_ACCOUNTING_CHILD";
const BLOCKED_TEST: &str = "audio_engine::material_migration_recovery::tests::project_asset_cleanup_accounting_tests::blocked_verified_pcm_handoff_is_not_a_successful_deletion";
const COMPLETE_TEST: &str = "audio_engine::material_migration_recovery::tests::project_asset_cleanup_accounting_tests::completed_verified_pcm_generation_is_counted_exactly_once";
const ORIGINAL_TEST: &str = "audio_engine::material_migration_recovery::tests::project_asset_cleanup_accounting_tests::direct_original_retirement_counts_one_physical_deletion";

fn isolated_child(profile: &str, test: &str) -> bool {
    if std::env::var(CHILD).ok().as_deref() == Some(profile) {
        return false;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, profile)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated cleanup accounting {profile} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    true
}

fn owned_root() -> (tempfile::TempDir, PathBuf) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("looper crate is below its workspace");
    let artifacts = workspace.join("test-artifacts/integration-bug-review-20261019");
    fs::create_dir_all(&artifacts).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("pcm-cleanup-accounting-")
        .tempdir_in(artifacts)
        .unwrap();
    let root = temp.path().join("samples");
    fs::create_dir(&root).unwrap();
    (temp, root)
}

#[derive(Debug)]
struct PcmCounts {
    before: u64,
    blocked: u64,
    complete: u64,
}

fn verified_pcm_observations() -> PcmCounts {
    let (_temp, root) = owned_root();
    let original = root.join("original.wav");
    wav(&original);
    let original_bytes = fs::read(&original).unwrap();
    let neighbor = root.join("unrelated.keep");
    fs::write(&neighbor, b"independent generated neighbor").unwrap();
    let engine = AudioEngine::new().unwrap();
    let assets = engine.project_assets.clone();
    assert!(Arc::ptr_eq(&assets, &ProjectAssets::shared()));

    // Restore a legacy project original and run the actual streaming decoder and
    // playback writer. This creates one PCM generation, with no material birth
    // receipt, newly copied original, or independently counted root retirement.
    let mut transaction = ColdTransaction::capture(&root, &original, false, &|| false).unwrap();
    material_migration::prepare_disk_pcm(
        &mut transaction,
        &original,
        48_000,
        2,
        crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES,
        &|| false,
        &mut |_| {},
    )
    .unwrap();
    let cache = transaction.commit(&|| false).unwrap();
    let lease = transaction.into_lease();
    assert!(lease.material_id.is_none());
    assert_eq!(lease.original_path, fs::canonicalize(&original).unwrap());
    assert_eq!(
        cache.parent().unwrap(),
        fs::canonicalize(root.join(".pcm-cache/v1")).unwrap()
    );

    // This independent OS handle is not a ProjectAssets or ColdStore reader.
    // It denies DELETE on the first physical leaf until explicitly released.
    let blocker_path = cache.join("decoder.f32le");
    let blocker = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&blocker_path)
        .unwrap();
    let mut inventory = inventory_gate(&root).unwrap();
    let mut artifact = MigrationArtifactLease::capture_native(&root, &cache).unwrap();
    let receipt = &artifact.sealed.as_ref().unwrap().receipt;
    assert_eq!(receipt.kind, ArtifactKind::PcmDirectory);
    assert_eq!(receipt.files.len(), 3);
    assert!(receipt.material_creation.is_none());
    let baseline = engine.project_asset_cleanup_status().unwrap();
    assert_eq!(baseline.0, 0);
    assert!(baseline.3.is_empty());
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    drop(lease);

    // Observe the real worker transfer by its retained original outcome. The OS
    // blocker makes this state stable, independent of worker scheduling speed.
    wait_until(|| {
        assets.status().unwrap().0 == 0 && cold_store::has_verified_retirement(&cache, &outcome)
    });
    let denied = fs::OpenOptions::new()
        .access_mode(0x0001_0080)
        .share_mode(1)
        .custom_flags(0x0020_0000)
        .open(&blocker_path)
        .unwrap_err();
    assert!(matches!(denied.raw_os_error(), Some(32 | 33)));
    assert!(PCM_NAMES.iter().all(|name| cache.join(name).is_file()));
    assert_eq!(outcome.status().unwrap(), ("pending", None));
    let blocked = engine.project_asset_cleanup_status().unwrap();
    assert_eq!(blocked.0, 1);
    assert!(blocked.3.is_empty());

    drop(blocker);
    wait_until(|| !cache.exists() && outcome.status().unwrap().0 == "complete");
    assert!(!cold_store::has_verified_retirement(&cache, &outcome));
    let complete = engine.project_asset_cleanup_status().unwrap();
    assert_eq!(complete.0, 0);
    assert!(complete.3.is_empty());
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    assert_eq!(
        fs::read(&neighbor).unwrap(),
        b"independent generated neighbor"
    );
    assert!(root.exists());
    let counts = PcmCounts {
        before: baseline.2,
        blocked: blocked.2,
        complete: complete.2,
    };
    eprintln!("verified PCM physical cleanup counts: {counts:?}");
    // Both regression assertions execute only after the real worker completed;
    // a RED result leaves no outstanding fixture retirement or sharing handle.
    counts
}

#[test]
fn blocked_verified_pcm_handoff_is_not_a_successful_deletion() {
    if isolated_child("blocked", BLOCKED_TEST) {
        return;
    }
    let counts = verified_pcm_observations();
    assert_eq!(
        counts.blocked, counts.before,
        "handing a verified PCM generation to another queue is not physical deletion"
    );
}

#[test]
fn completed_verified_pcm_generation_is_counted_exactly_once() {
    if isolated_child("complete", COMPLETE_TEST) {
        return;
    }
    let counts = verified_pcm_observations();
    assert_eq!(
        counts.complete,
        counts.before + 1,
        "one physical PCM generation must not be counted at both queues"
    );
}

#[test]
fn direct_original_retirement_counts_one_physical_deletion() {
    if isolated_child("original", ORIGINAL_TEST) {
        return;
    }
    let (_temp, root) = owned_root();
    let original = root.join("original.wav");
    wav(&original);
    let neighbor = root.join("unrelated.keep");
    fs::write(&neighbor, b"independent generated neighbor").unwrap();
    let engine = AudioEngine::new().unwrap();
    let baseline = engine.project_asset_cleanup_status().unwrap();
    assert_eq!(baseline.0, 0);
    assert!(baseline.3.is_empty());
    let mut inventory = inventory_gate(&root).unwrap();
    let mut artifact = MigrationArtifactLease::capture_native(&root, &original).unwrap();
    assert!(
        artifact
            .sealed
            .as_ref()
            .unwrap()
            .receipt
            .material_creation
            .is_none()
    );
    artifact.retire_native(&inventory).unwrap();
    let outcome = artifact.outcome.as_ref().unwrap().clone();
    inventory.release();
    artifact.release();
    drop(artifact);
    wait_until(|| !original.exists() && outcome.status().unwrap().0 == "complete");
    let complete = engine.project_asset_cleanup_status().unwrap();
    assert_eq!(complete.0, 0);
    assert!(complete.3.is_empty());
    assert_eq!(complete.2, baseline.2 + 1);
    assert_eq!(
        fs::read(&neighbor).unwrap(),
        b"independent generated neighbor"
    );
    assert!(root.exists());
}
