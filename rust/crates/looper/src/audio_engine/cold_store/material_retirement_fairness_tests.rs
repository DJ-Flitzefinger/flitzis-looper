//! Exact-process tests of the real pending-material retirement worker.
//!
//! The two lifecycle call sites only expose before/after barriers in test builds.
//! Selection, ownership admission, actual Windows sharing failures and pruning
//! remain production operations; no scheduler or deletion policy is reproduced.

use super::*;
use crate::audio_engine::project_assets;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

const CHILD: &str = "FLITZIS_MATERIAL_RETIREMENT_FAIRNESS_CHILD";
const FAIRNESS_TEST: &str = "audio_engine::cold_store::lifecycle::material_retirement_fairness_tests::actual_worker_rotates_past_eight_sharing_blockers";
const IDENTITY_TEST: &str = "audio_engine::cold_store::lifecycle::material_retirement_fairness_tests::actual_worker_preserves_replaced_binding_after_sharing_retry";
const QUEUE_TEST: &str = "audio_engine::cold_store::lifecycle::material_retirement_fairness_tests::pending_index_keeps_fifo_through_noop_arrival_cancel_and_replacement";
const DEADLOCK_GUARD: Duration = Duration::from_secs(30);

enum Event {
    Selected(Vec<PathBuf>),
    Finished,
}

enum Advance {
    Run,
    Next,
    Disarm,
}

struct Observer {
    events: SyncSender<Event>,
    advance: Receiver<Advance>,
    active_lap: bool,
}

static OBSERVER: Mutex<Option<Observer>> = Mutex::new(None);

/// Called after the real worker selected its bounded paths and dropped StoreState.
pub(super) fn before_prunes_for_test(
    prunes: &[(PathBuf, Option<Arc<project_assets::RetirementProtection>>)],
) {
    if prunes.is_empty() {
        return;
    }
    let mut slot = OBSERVER.lock().expect("material worker observer poisoned");
    let Some(observer) = slot.as_mut() else {
        return;
    };
    observer.active_lap = true;
    if observer
        .events
        .send(Event::Selected(
            prunes.iter().map(|(path, _)| path.clone()).collect(),
        ))
        .is_err()
        || !matches!(
            observer.advance.recv_timeout(DEADLOCK_GUARD),
            Ok(Advance::Run)
        )
    {
        *slot = None;
    }
}

/// A separate completion barrier also covers a lap that exhausts all pending work.
pub(super) fn after_prunes_for_test() {
    let mut slot = OBSERVER.lock().expect("material worker observer poisoned");
    let Some(observer) = slot.as_mut() else {
        return;
    };
    if !observer.active_lap {
        return;
    }
    observer.active_lap = false;
    if observer.events.send(Event::Finished).is_err()
        || !matches!(
            observer.advance.recv_timeout(DEADLOCK_GUARD),
            Ok(Advance::Next)
        )
    {
        *slot = None;
    }
}

struct Driver {
    events: Receiver<Event>,
    advance: Sender<Advance>,
    disarmed: bool,
}

impl Driver {
    fn arm() -> Self {
        let (events, receiver) = mpsc::sync_channel(1);
        let (advance, commands) = mpsc::channel();
        let mut observer = OBSERVER.lock().unwrap();
        assert!(
            observer.is_none(),
            "one exact-process observer owns this worker"
        );
        *observer = Some(Observer {
            events,
            advance: commands,
            active_lap: false,
        });
        Self {
            events: receiver,
            advance,
            disarmed: false,
        }
    }

    fn selected(&self) -> Vec<PathBuf> {
        match self.events.recv_timeout(DEADLOCK_GUARD) {
            Ok(Event::Selected(paths)) => paths,
            _ => panic!("real material worker did not reach its selection barrier"),
        }
    }

    fn run(&self) {
        self.advance.send(Advance::Run).unwrap();
        match self.events.recv_timeout(DEADLOCK_GUARD) {
            Ok(Event::Finished) => (),
            _ => panic!("real material worker did not complete its admitted lap"),
        }
    }

    fn next(&self) -> Vec<PathBuf> {
        self.advance.send(Advance::Next).unwrap();
        self.selected()
    }

    fn disarm(&mut self) {
        self.advance.send(Advance::Disarm).unwrap();
        // The worker holds this mutex through its completion handshake. This
        // acquisition acknowledges actual disarm without a sleep or poll.
        assert!(OBSERVER.lock().unwrap().is_none());
        self.disarmed = true;
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // Also releases a waiting worker after an infrastructure panic. The
        // exact subprocess is terminal before its parent evaluates any failure.
        if !self.disarmed {
            let _ = self.advance.send(Advance::Disarm);
        }
    }
}

struct Birth {
    root: PathBuf,
    identity: project_assets::FileIdentity,
    binding: project_assets::VerifiedFile,
    creation: Option<MaterialCreation>,
}

impl Birth {
    fn new(samples: &Path, source: &Path, bytes: &[u8]) -> Self {
        fs::write(source, bytes).unwrap();
        let digest = format!("{:x}", Sha256::digest(bytes));
        let [slot] = reserve_cleanup::<1>().unwrap();
        let mut selected = material::select(
            samples,
            source,
            &digest,
            bytes.len() as u64,
            &|| false,
            slot,
        )
        .unwrap();
        let root = selected.root.clone();
        let original_path = root.join(&selected.binding.original_leaf);
        let mut original = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&original_path)
            .unwrap();
        original.write_all(bytes).unwrap();
        original.sync_all().unwrap();
        let original_proof = project_assets::VerifiedFile {
            name: selected.binding.original_leaf.clone(),
            identity: file_identity(&original).unwrap(),
            bytes: bytes.len() as u64,
            sha256: digest,
        };
        drop(original);
        let (binding_reader, binding) =
            material::publish_binding(&root, &selected.binding).unwrap();
        let mut creation = selected.creation.take().expect("actual exclusive birth");
        assert_eq!(
            file_identity(&selected.directories[0]).unwrap(),
            creation.identity
        );
        creation.binding = Some(binding.clone());
        drop(binding_reader);
        drop(selected);
        // This is the real root-only state after exact original retirement,
        // without decoding or fabricating a source/cache/Native ACK.
        project_assets::remove_verified_file(&original_path, &original_proof).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        Self {
            root,
            identity: creation.identity.clone(),
            binding,
            creation: Some(creation),
        }
    }

    fn marker(&self) -> PathBuf {
        self.root.join(".material.json")
    }

    fn unchanged(&self) -> bool {
        project_assets::verify_file_proof(&self.marker(), &self.binding).is_ok()
            && project_assets::capture_identity(&self.root)
                .ok()
                .flatten()
                .as_ref()
                == Some(&self.identity)
    }
}

fn exact_child(test: &str) -> bool {
    if std::env::var(CHILD).as_deref() == Ok(test) {
        return false;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(CHILD, test)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "exact material-worker child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    true
}

fn fixture(count: usize) -> (tempfile::TempDir, Vec<Birth>) {
    let parent = std::env::temp_dir().join("codex");
    fs::create_dir_all(&parent).unwrap();
    let temp = tempfile::Builder::new()
        .prefix("looper-material-worker-")
        .tempdir_in(parent)
        .unwrap();
    let samples = temp.path().join("samples");
    fs::create_dir(&samples).unwrap();
    let samples = fs::canonicalize(samples).unwrap();
    let births = (0..count)
        .map(|index| {
            Birth::new(
                &samples,
                &temp.path().join(format!("synthetic-{index}.wav")),
                format!("owned synthetic material retirement source {index}").as_bytes(),
            )
        })
        .collect();
    (temp, births)
}

fn admit(births: &mut [Birth]) {
    let shared = store();
    let mut state = shared.state.lock().unwrap();
    assert!(state.materials.is_empty() && state.deleting.is_empty());
    for birth in births {
        let mut creation = birth.creation.take().unwrap();
        creation.pending = true;
        retain_material_creation_under_gate(&mut state, creation);
    }
    shared.changed.notify_one();
}

fn gone(birth: &Birth) -> bool {
    !birth.root.exists()
        && !store()
            .state
            .lock()
            .unwrap()
            .materials
            .contains_key(&birth.root)
}

/// Entered at the completion barrier after all synthetic sharing handles close.
fn drain(driver: &mut Driver, births: &[Birth]) -> (bool, bool, usize) {
    let mut budget_preserved = true;
    let mut laps = 0;
    while births.iter().any(|birth| !gone(birth)) && laps < 10 {
        budget_preserved &= driver.next().len() <= 8;
        driver.run();
        laps += 1;
    }
    let all_terminal = births.iter().all(gone);
    driver.disarm();
    (all_terminal, budget_preserved, laps)
}

#[test]
fn actual_worker_rotates_past_eight_sharing_blockers() {
    if exact_child(FAIRNESS_TEST) {
        return;
    }
    let (_temp, mut births) = fixture(9);
    let mut driver = Driver::arm();
    admit(&mut births);
    let selected = driver.selected();
    let selected_roots: HashSet<_> = selected
        .iter()
        .map(|path| path.parent().unwrap().to_owned())
        .collect();
    if selected.len() != 8 || selected_roots.len() != 8 {
        driver.run();
        let (terminal, _, _) = drain(&mut driver, &births);
        assert!(
            terminal,
            "invalid-selection fixture failed terminal cleanup"
        );
        panic!("real first lap did not select exactly eight distinct roots");
    }
    let eligible = births
        .iter()
        .find(|birth| !selected_roots.contains(&birth.root))
        .expect("ninth actual stable-HashMap candidate");
    let blockers: Vec<_> = births
        .iter()
        .filter(|birth| selected_roots.contains(&birth.root))
        .map(|birth| sealed_reader(&birth.marker()).unwrap())
        .collect();

    // No timed wait decides success: each check follows the actual completed
    // production lap while the next lap is held at the completion barrier.
    driver.run();
    let first_lap_preserved = births
        .iter()
        .filter(|birth| selected_roots.contains(&birth.root))
        .all(Birth::unchanged);
    let second_lap_bounded = driver.next().len() <= 8;
    driver.run();
    let eligible_progressed = gone(eligible);
    let blocked_preserved = births
        .iter()
        .filter(|birth| selected_roots.contains(&birth.root))
        .all(Birth::unchanged);

    // Defer every behavior assertion until all genuine sharing handles are
    // released and the real worker has terminally retired every owned birth.
    drop(blockers);
    let (all_terminal, cleanup_bounded, cleanup_laps) = drain(&mut driver, &births);
    println!(
        "actual-material-worker: roots=9 sharing_blockers=8 bounded_probe_laps=2 eligible_progressed={eligible_progressed} cleanup_laps={cleanup_laps} all_terminal={all_terminal}"
    );
    assert!(
        all_terminal,
        "owned retirement fixture failed terminal cleanup"
    );
    assert!(
        second_lap_bounded && cleanup_bounded,
        "material cleanup exceeded eight attempts in a lap"
    );
    assert!(
        first_lap_preserved && blocked_preserved,
        "sharing blockers lost exact binding bytes/FileID"
    );
    assert!(
        eligible_progressed,
        "the real ninth pending material starved behind eight persistent Windows sharing failures across two bounded laps"
    );
}

#[test]
fn actual_worker_preserves_replaced_binding_after_sharing_retry() {
    if exact_child(IDENTITY_TEST) {
        return;
    }
    let (_temp, mut births) = fixture(1);
    let mut driver = Driver::arm();
    admit(&mut births);
    let selected = driver.selected();
    let single_selection = selected.len() == 1;
    let birth = &births[0];
    let blocker = sealed_reader(&birth.marker()).unwrap();
    driver.run();
    let preserved_while_blocked =
        project_assets::verify_file_proof(&birth.marker(), &birth.binding).is_ok();

    // Replace only this test's leaf with equal bytes and a demonstrably new
    // FileID after the actual worker has observed the genuine sharing failure.
    let bytes = fs::read(birth.marker()).unwrap();
    drop(blocker);
    let next = birth.root.join("owned-replacement.tmp");
    fs::write(&next, &bytes).unwrap();
    let replacement = project_assets::capture_identity(&next).unwrap().unwrap();
    project_assets::remove_verified_file(&birth.marker(), &birth.binding).unwrap();
    fs::rename(&next, birth.marker()).unwrap();
    let replaced = replacement != birth.binding.identity;
    driver.next();
    driver.run();
    let replacement_preserved = fs::read(birth.marker()).is_ok_and(|actual| actual == bytes)
        && project_assets::capture_identity(&birth.marker())
            .unwrap()
            .as_ref()
            == Some(&replacement);
    let mut state = store().state.lock().unwrap();
    let retired_terminally = state
        .materials
        .get(&birth.root)
        .is_some_and(|creation| !creation.pending);
    // The old authority terminally preserved this replacement. Release only
    // the test's retained admission slot; TempDir owns the synthetic new leaf.
    state.materials.remove(&birth.root);
    drop(state);
    driver.disarm();
    assert!(single_selection && preserved_while_blocked && replaced);
    assert!(
        replacement_preserved && retired_terminally,
        "old birth receipt removed or retried a replaced marker"
    );
}

fn assert_pending_index(state: &StoreState) {
    let queued: HashSet<_> = state.pending_materials.iter().cloned().collect();
    assert_eq!(
        queued.len(),
        state.pending_materials.len(),
        "one routing entry per birth"
    );
    assert!(queued.len() <= state.materials.len() && queued.len() <= CLEANUP_ADMISSION_LIMIT);
    assert!(
        state
            .materials
            .iter()
            .all(|(root, creation)| { creation.pending == queued.contains(root) })
    );
}

#[test]
fn pending_index_keeps_fifo_through_noop_arrival_cancel_and_replacement() {
    if exact_child(QUEUE_TEST) {
        return;
    }
    let (temp, mut births) = fixture(10);
    // This routing-index property test owns an isolated StoreState. The two
    // other tests exercise the same selection through actual worker/file I/O.
    let mut state = StoreState::default();
    for birth in births.iter_mut().take(9) {
        let mut creation = birth.creation.take().unwrap();
        creation.pending = true;
        retain_material_creation_under_gate(&mut state, creation);
    }
    assert_pending_index(&state);
    let original_order = state.pending_materials.clone();
    state.set_material_pending(&births[0].root, true);
    state.set_material_pending(&births[0].root, true);
    assert_eq!(
        state.pending_materials, original_order,
        "repeated pending preserves FIFO position"
    );
    assert_pending_index(&state);

    let first = state.pending_material_prunes();
    assert_eq!(first.len(), 8);
    assert_eq!(state.pending_materials.front(), Some(&births[8].root));
    let mut arrival = births[9].creation.take().unwrap();
    arrival.pending = true;
    retain_material_creation_under_gate(&mut state, arrival);
    state.set_material_pending(&births[8].root, true);
    assert_eq!(state.pending_materials.front(), Some(&births[8].root));
    assert_eq!(state.pending_materials.back(), Some(&births[9].root));
    assert_pending_index(&state);
    let second = state.pending_material_prunes();
    assert_eq!(second.len(), 8);
    assert_eq!(second[0].parent(), Some(births[8].root.as_path()));
    assert!(
        !second
            .iter()
            .any(|path| path.parent() == Some(births[9].root.as_path()))
    );
    assert_pending_index(&state);

    state.set_material_pending(&births[0].root, false);
    let cancelled_order = state.pending_materials.clone();
    state.set_material_pending(&births[0].root, false);
    assert_eq!(state.pending_materials, cancelled_order);
    assert!(!state.pending_materials.contains(&births[0].root));
    assert_pending_index(&state);

    // An actual new exclusive birth replaces the same pathname with a new
    // FileID. Its dormant receipt must remove the predecessor routing token.
    let old_identity = births[1].identity.clone();
    fs::rename(&births[1].root, temp.path().join("superseded owned root")).unwrap();
    let samples = births[1].root.parent().unwrap();
    let mut replacement = Birth::new(
        samples,
        &temp.path().join("synthetic-1.wav"),
        b"owned replacement material with independently created root",
    );
    assert_eq!(replacement.root, births[1].root);
    assert_ne!(replacement.identity, old_identity);
    retain_material_creation_under_gate(&mut state, replacement.creation.take().unwrap());
    assert!(!state.pending_materials.contains(&replacement.root));
    assert_pending_index(&state);
    state.set_material_pending(&replacement.root, true);
    let rearmed_order = state.pending_materials.clone();
    state.set_material_pending(&replacement.root, true);
    assert_eq!(state.pending_materials, rearmed_order);
    assert_eq!(state.pending_materials.back(), Some(&replacement.root));
    assert_pending_index(&state);
    // No global worker owns this isolated index; dropping it releases every
    // actual admission slot before TempDir removes its synthetic source trees.
    drop(state);
}
