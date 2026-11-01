//! Actual migration queue bodies, with no device or process-wide directory changes.
use super::super::stem_cache::STEM_FILE_NAMES;
use super::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct HeldWorkers(Vec<mpsc::Sender<()>>);
impl HeldWorkers {
    fn new(engine: &AudioEngine) -> Self {
        let mut held = Self(Vec::new());
        for _ in 0..super::super::cold_jobs::WORKERS {
            let (started, entered) = mpsc::channel();
            let (release, resumed) = mpsc::channel();
            engine
                .cold_jobs
                .submit(engine.cold_jobs.reserve().unwrap(), move || {
                    started.send(()).unwrap();
                    let _ = resumed.recv();
                })
                .unwrap();
            held.0.push(release);
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        held
    }
}
impl Drop for HeldWorkers {
    fn drop(&mut self) {
        for release in self.0.drain(..) {
            let _ = release.send(());
        }
    }
}

fn wait(mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < end, "migration fixture did not settle");
        std::thread::yield_now();
    }
}

fn wav(path: &Path) {
    let mut bytes = Vec::from(&b"RIFF"[..]);
    bytes.extend_from_slice(&(256_u32 + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x02\0");
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&192_000_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&256_u32.to_le_bytes());
    for frame in 0..64_i16 {
        bytes.extend_from_slice(&((frame - 32) * 32).to_le_bytes());
        bytes.extend_from_slice(&((32 - frame) * 16).to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    old: PathBuf,
    old_bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Python::initialize();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let old = root.join("old.wav");
        wav(&old);
        let old_bytes = fs::read(&old).unwrap();
        Self {
            _temp: temp,
            root,
            old,
            old_bytes,
        }
    }
    fn prepare(&self, engine: &AudioEngine) -> MaterialMigrationPreparation {
        prepare_at_root(
            engine,
            "samples/old.wav".into(),
            self.root.clone(),
            48_000,
            2,
        )
        .unwrap()
    }
    fn stem_work(
        &self,
        engine: &AudioEngine,
    ) -> (MaterialMigrationStemPreparation, std::sync::Weak<[f32]>) {
        let material =
            material_migration::prepare_material(&self.root, &self.old, 48_000, 2, &|| false)
                .unwrap();
        let weak = Arc::downgrade(&material.sample.samples);
        let digest = format!("{:x}", Sha256::digest(&self.old_bytes));
        let old_version = format!("samples/old.wav|sha256-v1:{digest}");
        let new_version = format!(
            "{}|sha256-v1:{digest}",
            material.metadata()["new_reference"].as_str().unwrap()
        );
        let old_stems = self.root.join("stems/#1");
        fs::create_dir_all(&old_stems).unwrap();
        let hashes: serde_json::Map<String, serde_json::Value> = STEM_FILE_NAMES
            .into_iter()
            .map(|stem| {
                let path = old_stems.join(format!("{stem}.wav"));
                wav(&path);
                (
                    stem.into(),
                    serde_json::json!(format!("{:x}", Sha256::digest(fs::read(path).unwrap()))),
                )
            })
            .collect();
        fs::write(
            old_stems.join(".complete.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema":"stem-set-sha256-v1", "source_version":old_version, "stems":hashes
            }))
            .unwrap(),
        )
        .unwrap();
        let preparation = prepared_for_test(engine, material, &self.root).unwrap();
        let work = prepare_stems_at_root(
            engine,
            preparation.material().unwrap(),
            "samples/stems/#1".into(),
            old_version,
            new_version,
            "0123456789abcdef0123456789abcdef".into(),
            self.root.clone(),
        )
        .unwrap();
        drop(preparation);
        (work, weak)
    }
}

fn act(engine: &AudioEngine, close: bool) {
    if close {
        engine.cold_jobs.close_admission();
    } else {
        Python::attach(|py| assert!(engine.poll_loader_events(py).unwrap().is_none()));
    }
}

fn peer(engine: &AudioEngine) -> Arc<AtomicUsize> {
    let ran = Arc::new(AtomicUsize::new(0));
    let counter = ran.clone();
    engine
        .cold_jobs
        .submit(engine.cold_jobs.reserve().unwrap(), move || {
            counter.fetch_add(1, Ordering::AcqRel);
        })
        .unwrap();
    ran
}

fn queued_material(close: bool) {
    let fixture = Fixture::new();
    let engine = AudioEngine::new().unwrap();
    let held = HeldWorkers::new(&engine);
    let peer = peer(&engine);
    let preparation = fixture.prepare(&engine);
    assert_eq!(preparation.status().unwrap(), "preparing");
    assert_eq!(engine.cold_jobs.counts_for_test(), (2, 2, 4));
    preparation.cancel();
    act(&engine, close);
    let status = preparation.status().unwrap();
    let error = preparation.error().unwrap();
    let counts = engine.cold_jobs.counts_for_test();
    // The public preparation intentionally retains its own original pin.
    let retained_pin = engine.project_assets.held_owner_count(&fixture.old);
    drop(preparation);
    let pins_after_ticket = engine.project_assets.held_owner_count(&fixture.old);
    let peer_before = peer.load(Ordering::Acquire);
    eprintln!(
        "material close={close} before release: status={status} error={error:?} counts={counts:?} retained_pin={retained_pin} pins_after_ticket={pins_after_ticket} peer={peer_before}"
    );
    drop(held);
    wait(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    drop(engine); // Join actual workers before any final RED assertion.
    assert_eq!(status, "failed");
    assert!(error.unwrap().contains("cancelled"));
    assert_eq!(counts, if close { (2, 0, 2) } else { (2, 1, 3) });
    assert_eq!((retained_pin, pins_after_ticket), (1, 0));
    assert_eq!(peer_before, 0);
    assert_eq!(peer.load(Ordering::Acquire), usize::from(!close));
    assert_eq!(fs::read(&fixture.old).unwrap(), fixture.old_bytes);
}

fn queued_stems(close: bool) {
    let fixture = Fixture::new();
    let engine = AudioEngine::new().unwrap();
    let held = HeldWorkers::new(&engine);
    let peer = peer(&engine);
    let (work, weak) = fixture.stem_work(&engine);
    assert_eq!(work.status().unwrap(), "preparing");
    assert!(weak.upgrade().is_some());
    work.cancel();
    act(&engine, close);
    let status = work.status().unwrap();
    let error = work.error().unwrap();
    let counts = engine.cold_jobs.counts_for_test();
    let pcm_ended = weak.upgrade().is_none();
    let pins = engine
        .project_assets
        .held_owner_count(&fixture.root.join("stems/#1"));
    let peer_before = peer.load(Ordering::Acquire);
    eprintln!(
        "stems close={close} before release: status={status} error={error:?} counts={counts:?} pcm_ended={pcm_ended} pins={pins} peer={peer_before}"
    );
    drop(held);
    wait(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    drop(engine);
    assert_eq!(status, "failed");
    assert!(error.unwrap().contains("cancelled"));
    assert_eq!(counts, if close { (2, 0, 2) } else { (2, 1, 3) });
    assert!(
        pcm_ended,
        "queued job must end its actual last prepared PCM owner"
    );
    assert_eq!(pins, 0);
    assert_eq!(peer_before, 0);
    assert_eq!(peer.load(Ordering::Acquire), usize::from(!close));
    assert_eq!(fs::read(&fixture.old).unwrap(), fixture.old_bytes);
    assert!(fixture.root.join("stems/#1/.complete.json").is_file());
}

#[test]
fn cancelled_queued_material_preparation_settles_before_workers_end() {
    queued_material(false);
}
#[test]
fn closed_queued_material_preparation_settles_before_workers_end() {
    queued_material(true);
}
#[test]
fn cancelled_queued_material_stems_settle_and_end_last_pcm_before_workers_end() {
    queued_stems(false);
}
#[test]
fn closed_queued_material_stems_settle_and_end_last_pcm_before_workers_end() {
    queued_stems(true);
}

#[test]
fn claimed_material_preparation_preserves_success_and_explicit_rollback() {
    let fixture = Fixture::new();
    let engine = AudioEngine::new().unwrap();
    let preparation = fixture.prepare(&engine);
    wait(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_eq!(preparation.status().unwrap(), "ready");
    let material = preparation.material().unwrap();
    let new_original = material.lease.original_path.clone();
    let new_cache = material.lease.cache_path.clone();
    drop(material);
    preparation.cancel();
    act(&engine, false);
    assert_eq!(preparation.status().unwrap(), "ready");
    assert!(new_original.is_file() && new_cache.join("playback.f32le").is_file());
    preparation.abort_unpublished().unwrap();
    assert_eq!(preparation.status().unwrap(), "failed");
    drop(preparation);
    engine.project_assets.collect_for_test();
    assert_eq!(fs::read(&fixture.old).unwrap(), fixture.old_bytes);
}

#[test]
fn claimed_material_stems_preserve_created_cleanup_receipt_after_cancel() {
    let fixture = Fixture::new();
    let engine = AudioEngine::new().unwrap();
    let (work, weak) = fixture.stem_work(&engine);
    wait(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_eq!(
        work.status().unwrap(),
        "ready",
        "{:?}",
        work.error().unwrap()
    );
    let reference = work.cache_reference().unwrap();
    assert!(work.created().unwrap());
    let generation = super::super::material_paths::resolve(&fixture.root, Path::new(&reference))
        .unwrap()
        .path;
    let marker = fs::read(generation.join(".complete.json")).unwrap();
    work.cancel();
    act(&engine, false);
    assert_eq!(work.status().unwrap(), "ready");
    assert_eq!(work.cache_reference().unwrap(), reference);
    assert!(work.created().unwrap());
    assert!(weak.upgrade().is_none());
    for stem in STEM_FILE_NAMES {
        assert_eq!(
            fs::read(generation.join(format!("{stem}.wav"))).unwrap(),
            fs::read(fixture.root.join(format!("stems/#1/{stem}.wav"))).unwrap()
        );
    }
    assert_eq!(fs::read(generation.join(".complete.json")).unwrap(), marker);
}
