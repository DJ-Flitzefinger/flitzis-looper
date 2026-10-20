//! A real original source capture retains custody across an unrelated publication.

use super::CapturedPadStart;
use crate::audio_engine::input_mapping::launch_admission_tests::RATE;
use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};
use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

thread_local! {
    static AFTER_OWNER: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

pub(super) fn after_owner() {
    let hook = AFTER_OWNER.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn capture(fixture: &Fixture) -> pyo3::PyResult<CapturedPadStart> {
    fixture.control.engine.capture_pad_start(
        0,
        LOOP_START as f64 / f64::from(RATE),
        Some(LOOP_END as f64 / f64::from(RATE)),
        1.0,
        false,
        None,
        None,
    )
}

fn pending_capture_then(change: &str) {
    let mut fixture = Fixture::new(false);
    let ownership = fixture.control.engine.input_runtime_ownership.clone();
    let original = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    let peer = ownership.slot_snapshot(1).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(peer, peer.lifetime())
        .unwrap();
    let (enter_tx, enter_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        enter_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        ownership
            .commit_slot_write_fixed(
                &write,
                || {
                    ready_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    true
                },
                |_| {},
            )
            .unwrap();
    });
    AFTER_OWNER.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(move || {
            enter_tx.send(()).unwrap();
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
    });

    // The actual engine has already frozen source generation, request, timing,
    // timestamp and its one Work before the independent marker becomes odd.
    let result = capture(&fixture);
    let observed_pending = result.as_ref().ok().map(|start| {
        assert!(!start.accepted());
        assert!(!start.is_published());
        (start.content_key(), start.original_owner_status())
    });
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    let start = result.expect("a proven original capture must wait through a scalar marker");
    assert_eq!(observed_pending, Some((original, "pending")));
    assert_eq!(start.content_key(), original);
    assert_eq!(start.original_owner_status(), "current");
    let sequence = start.sequence();
    let timestamp = start.received_timestamp();
    fixture.drain();
    let target = start.target_frame();
    assert!(target.is_some());

    match change {
        "stop" => fixture.control.engine.stop_sample(0).unwrap(),
        "source" => fixture.control.engine.unload_sample(0).unwrap(),
        "none" => {}
        _ => unreachable!(),
    }
    let publication = fixture.control.engine.publish_pad_start(&start, None);
    if change == "none" {
        assert!(publication.unwrap());
        fixture.drain();
        fixture.render(1);
        assert_eq!(start.effective_frame(), target);
        assert_eq!(fixture.starts(0), 1);
    } else {
        assert!(publication.is_err());
        fixture.drain();
        fixture.render(1);
        assert_eq!(start.effective_frame(), None);
        assert_eq!(fixture.starts(0), 0);
    }
    assert_eq!(start.content_key(), original);
    assert_eq!(start.sequence(), sequence);
    assert_eq!(start.received_timestamp(), timestamp);
    assert_eq!(start.target_frame(), target);
}

#[test]
fn captured_original_waits_through_final_unrelated_marker() {
    pending_capture_then("none");
}

#[test]
fn pending_original_does_not_outlive_an_actual_stop() {
    pending_capture_then("stop");
}

#[test]
fn pending_original_cannot_acquire_a_new_source_lifetime() {
    pending_capture_then("source");
}
