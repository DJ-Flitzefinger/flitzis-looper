use super::*;
use std::cell::RefCell;
use std::sync::mpsc;

thread_local! {
    static AFTER_UNLOCK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn after_unlock() {
    let hook = AFTER_UNLOCK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn overlapping_sweeps(stems: bool, surviving_owner: bool) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("samples");
    let path = if stems {
        root.join("stems/#2/.ready-0123456789abcdef0123456789abcdef")
    } else {
        root.join("original.wav")
    };
    let leaf = if stems {
        path.join("vocals.wav")
    } else {
        path.clone()
    };
    fs::create_dir_all(leaf.parent().unwrap()).unwrap();
    fs::write(&leaf, b"original immutable bytes").unwrap();
    let assets = ProjectAssets::isolated();
    let mut application = assets.acquire_pin(&root, &path).unwrap();
    let observed_owner = Arc::downgrade(application.owner.as_ref().unwrap());
    let mut survivor = surviving_owner.then(|| assets.acquire_pin(&root, &path).unwrap());
    assets.retire(&root, &path, stems).unwrap();

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_assets = assets.clone();
    let first = thread::spawn(move || {
        AFTER_UNLOCK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                let _ = entered_tx.send(());
                // Disconnection also releases this gate on an unwinding test.
                let _ = release_rx.recv();
            }));
        });
        first_assets.collect();
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(5));
    let observation = entered.as_ref().ok().map(|()| {
        // End the real application pin while A is beyond the actual state
        // unlock. Neither the gate nor this Weak probe owns its PathOwner.
        application.release();
        let application_ended = application.released();
        let snapshot_owners = observed_owner.strong_count();
        assets.collect();
        (
            application_ended,
            snapshot_owners,
            leaf.exists(),
            assets.status(),
        )
    });
    let _ = release_tx.send(());
    let joined = first.join();

    // Assert captured facts only after A has ended, including on timeout.
    assert!(
        entered.is_ok(),
        "collector did not reach its actual unlock: {entered:?}"
    );
    assert!(joined.is_ok(), "first collector panicked");
    let (application_ended, snapshot_owners, exists, status) = observation.unwrap();
    assert!(application_ended);
    assert_eq!(
        snapshot_owners, 0,
        "an observation outlived the asset gate: exists={exists}, status={status:?}"
    );
    let (pending, _, deleted, errors) = status.unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(exists, surviving_owner);
    assert_eq!(pending, usize::from(surviving_owner));
    assert_eq!(deleted, u64::from(!surviving_owner));
    if let Some(owner) = survivor.as_mut() {
        assert_eq!(fs::read(&leaf).unwrap(), b"original immutable bytes");
        owner.release();
        assets.collect();
        assert!(!leaf.exists());
        assert_eq!(assets.status().unwrap().0, 0);
    }
}

#[test]
fn overlapping_original_sweeps_cannot_retain_observer_pins_after_application_end() {
    for survivor in [false, true] {
        overlapping_sweeps(false, survivor);
    }
}

#[test]
fn overlapping_stem_sweeps_cannot_retain_observer_pins_after_application_end() {
    for survivor in [false, true] {
        overlapping_sweeps(true, survivor);
    }
}
