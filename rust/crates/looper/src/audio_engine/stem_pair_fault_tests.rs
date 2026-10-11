//! Real writer I/O fault seams, rollback ownership and bounded retry. These are
//! precise injected block/reserve/commit failures, not measured disk/RAM exhaustion
//! or a process/power-loss matrix. Process and resource evidence is separate.
#![cfg(windows)]

use super::project_assets::{self, FileIdentity};
use super::stem_cache::STEM_FILE_NAMES;
use super::stem_pair::{
    StemPairFault, open_verified_pair, prepare_complete_pair, set_fault_for_test,
};
use super::stem_pair_tests::Fixture;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

const POINTS: [StemPairFault; 9] = [
    StemPairFault::AlignmentScan,
    StemPairFault::PcmConvert,
    StemPairFault::PcmWrite,
    StemPairFault::PcmFlush,
    StemPairFault::StagingReopen,
    StemPairFault::PcmRename,
    StemPairFault::ReadyReopen,
    StemPairFault::CommonFlush,
    StemPairFault::CommonReopen,
];

#[test]
fn cooperative_midphase_cancellation_after_io_returns_rolls_back_only_new_owners() {
    use super::stem_pair::set_io_observer_for_test;
    use std::cell::Cell;
    use std::rc::Rc;
    for point in [
        StemPairFault::AlignmentScan,
        StemPairFault::PcmConvert,
        StemPairFault::PcmWrite,
        StemPairFault::PcmVerify,
        StemPairFault::PcmRename,
        StemPairFault::CommonReopen,
    ] {
        let fixture = Fixture::new_with_frames(5, 16_393);
        let wav_before = leaves(&fixture.wav_path);
        let source_before = fs::read(&fixture.material.lease.original_path).unwrap();
        let cancel = Rc::new(Cell::new(false));
        let visits = Rc::new(Cell::new(0));
        let signal = cancel.clone();
        let observed = visits.clone();
        set_io_observer_for_test(Some(Box::new(move |stage| {
            if stage == point {
                observed.set(observed.get() + 1);
                // The checkpoint runs between operations, never by interrupting
                // a held file operation/flush/rename or killing a worker.
                signal.set(true);
            }
        })));
        let result = prepare_complete_pair(
            &fixture.root,
            &fixture.material,
            &fixture.version,
            &fixture.wav_reference,
            &|| cancel.get(),
        );
        set_io_observer_for_test(None);
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("cancel at {point:?} committed"),
        };
        assert!(visits.get() > 0, "unexecuted checkpoint {point:?}");
        assert!(error.contains("cancelled"), "wrong error {error}");
        assert_eq!(children(&fixture.stem_pcm_base()), [".pairs"]);
        assert!(children(&fixture.stem_pcm_base().join(".pairs")).is_empty());
        assert_eq!(leaves(&fixture.wav_path), wav_before);
        assert_eq!(
            fs::read(&fixture.material.lease.original_path).unwrap(),
            source_before
        );
        let retry = fixture.prepare().unwrap();
        assert!(retry.created_pcm && retry.created_descriptor);
    }
}

#[test]
fn low_level_write_and_postwrite_verification_faults_preserve_reused_wavs_and_allow_retry() {
    for point in [StemPairFault::PcmWrite, StemPairFault::PcmVerify] {
        let fixture = Fixture::new_with_frames(-4, 16_393);
        let before = leaves(&fixture.wav_path);
        use super::stem_pair::set_io_observer_for_test;
        use std::cell::Cell;
        use std::rc::Rc;
        let operations = Rc::new(Cell::new(0));
        let seen = operations.clone();
        set_io_observer_for_test(Some(Box::new(move |stage| {
            if stage == point {
                seen.set(seen.get() + 1);
                if seen.get() == 2 {
                    set_fault_for_test(Some(point));
                }
            }
        })));
        let result = fixture.prepare();
        set_io_observer_for_test(None);
        set_fault_for_test(None);
        let error = match result {
            Err(e) => e,
            Ok(_) => panic!("missing {point:?}"),
        };
        assert_eq!(operations.get(), 2);
        assert_injected(&error, point);
        assert_eq!(children(&fixture.stem_pcm_base()), [".pairs"]);
        assert!(children(&fixture.stem_pcm_base().join(".pairs")).is_empty());
        assert_eq!(leaves(&fixture.wav_path), before);
        let retry = fixture.prepare().unwrap();
        assert!(retry.created_pcm && retry.created_descriptor);
    }
}

#[test]
fn allocation_failure_after_productive_global_scan_preserves_files_and_retries() {
    use super::stem_cache::streamed::set_allocation_failure_for_test;
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            set_allocation_failure_for_test(None);
        }
    }
    let fixture = Fixture::new_with_frames(5, 16_393);
    let before = leaves(&fixture.wav_path);
    let error = {
        let _reset = Reset;
        // Two first-pass buffers have already allocated and the full source
        // scan/digests have returned when the second-pass reserve is injected.
        set_allocation_failure_for_test(Some(3));
        match fixture.prepare() {
            Err(e) => e,
            Ok(_) => panic!("missing reserve failure"),
        }
    };
    assert!(error.contains("allocation failure before try_reserve_exact"));
    assert_eq!(children(&fixture.stem_pcm_base()), [".pairs"]);
    assert!(children(&fixture.stem_pcm_base().join(".pairs")).is_empty());
    assert_eq!(leaves(&fixture.wav_path), before);
    let retry = fixture.prepare().unwrap();
    assert!(retry.created_pcm && retry.created_descriptor);
}

#[test]
fn cancellation_after_common_verified_commit_keeps_the_committed_generation_and_actual_readers() {
    use super::stem_pair::set_io_observer_for_test;
    use std::cell::Cell;
    use std::rc::Rc;
    let fixture = Fixture::new_with_frames(-4, 16_393);
    let cancel = Rc::new(Cell::new(false));
    let signal = cancel.clone();
    set_io_observer_for_test(Some(Box::new(move |point| {
        if point == StemPairFault::PairCommitted {
            signal.set(true);
        }
    })));
    let result = prepare_complete_pair(
        &fixture.root,
        &fixture.material,
        &fixture.version,
        &fixture.wav_reference,
        &|| cancel.get(),
    );
    set_io_observer_for_test(None);
    let pair = result.unwrap();
    assert!(cancel.get());
    assert!(pair.created_pcm && pair.created_descriptor);
    let reference = pair.descriptor_reference.clone();
    let descriptor = pair.descriptor.clone();
    // A signal after the verified descriptor-last commit cannot retroactively
    // turn committed bytes into private staging or delete underneath readers.
    let pins = pair.clone_pins(fixture.root.parent().unwrap()).unwrap();
    let retry = fixture.prepare().unwrap();
    assert!(!retry.created_pcm && !retry.created_descriptor);
    assert_eq!(retry.descriptor, descriptor);
    drop(retry);
    drop(pair);
    let reopened =
        open_verified_pair(&fixture.root, &reference, &fixture.material, &|| false).unwrap();
    assert_eq!(reopened.descriptor, descriptor);
    drop(reopened);
    drop(pins);
}

struct FaultReset;
impl FaultReset {
    fn arm(point: StemPairFault) -> Self {
        set_fault_for_test(Some(point));
        Self
    }
}
impl Drop for FaultReset {
    fn drop(&mut self) {
        // The same thread is reset even if an assertion/preparation unwinds.
        set_fault_for_test(None);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LeafProof {
    name: String,
    identity: FileIdentity,
    bytes: u64,
    sha256: String,
}

fn leaves(path: &Path) -> Vec<LeafProof> {
    let mut proof = fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let encoded = fs::read(entry.path()).unwrap();
            LeafProof {
                name: entry.file_name().to_str().unwrap().into(),
                identity: project_assets::capture_identity(&entry.path())
                    .unwrap()
                    .unwrap(),
                bytes: encoded.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&encoded)),
            }
        })
        .collect::<Vec<_>>();
    proof.sort_by(|left, right| left.name.cmp(&right.name));
    proof
}

fn children(path: &Path) -> Vec<String> {
    let mut result = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().into())
        .collect::<Vec<_>>();
    result.sort();
    result
}

fn assert_injected(error: &str, point: StemPairFault) {
    assert!(
        error.contains(&format!("injected stem pair I/O failure at {point:?}")),
        "wrong fault boundary: {error}"
    );
}

#[test]
fn each_writer_io_fault_rolls_back_new_eligibility_and_exact_creations_then_retries() {
    for point in POINTS {
        let fixture = Fixture::new(5);
        let wav_before = leaves(&fixture.wav_path);
        assert_eq!(wav_before.len(), 6);
        let error = {
            let _reset = FaultReset::arm(point);
            match fixture.prepare() {
                Err(error) => error,
                Ok(_) => panic!("new writer did not encounter {point:?}"),
            }
        };
        assert_injected(&error, point);
        let base = fixture.stem_pcm_base();
        assert_eq!(
            children(&base),
            [".pairs"],
            "new PCM/staging must roll back at {point:?}"
        );
        assert!(
            children(&base.join(".pairs")).is_empty(),
            "partial/common eligibility remained at {point:?}"
        );
        assert_eq!(
            leaves(&fixture.wav_path),
            wav_before,
            "reused WAV ownership changed at {point:?}"
        );

        let pair = fixture.prepare().unwrap();
        assert!(
            pair.created_pcm && pair.created_descriptor,
            "retry must report its actual fresh creations at {point:?}"
        );
        let descriptor = pair.descriptor.clone();
        let reference = pair.descriptor_reference.clone();
        let pcm = fixture
            .root
            .parent()
            .unwrap()
            .join(&descriptor.pcm_generation);
        assert_eq!(leaves(&pcm).len(), 6);
        for name in STEM_FILE_NAMES {
            assert!(pcm.join(format!("{name}.f32le")).is_file());
        }
        drop(pair);
        let reopened =
            open_verified_pair(&fixture.root, &reference, &fixture.material, &|| false).unwrap();
        assert_eq!(reopened.descriptor, descriptor);
        drop(reopened);
        let warm = fixture.prepare().unwrap();
        assert!(!warm.created_pcm && !warm.created_descriptor);
        assert_eq!(warm.descriptor_reference, reference);
        assert_eq!(warm.descriptor, descriptor);
        assert_eq!(leaves(&fixture.wav_path), wav_before);
        assert_eq!(
            children(&base),
            [
                ".pairs".to_owned(),
                pcm.file_name().unwrap().to_str().unwrap().to_owned()
            ]
        );
    }
}

#[test]
fn fault_after_reusing_verified_half_ready_pcm_never_gains_pcm_rollback_rights() {
    for point in [
        StemPairFault::ReadyReopen,
        StemPairFault::CommonFlush,
        StemPairFault::CommonReopen,
    ] {
        let fixture = Fixture::new(-4);
        let pair = fixture.prepare().unwrap();
        let descriptor = pair.descriptor.clone();
        let reference = pair.descriptor_reference.clone();
        let pcm = fixture
            .root
            .parent()
            .unwrap()
            .join(&descriptor.pcm_generation);
        let common = fixture.root.parent().unwrap().join(&reference);
        // All sealed pair handles really end before removing this known own marker.
        drop(pair);
        let wav_before = leaves(&fixture.wav_path);
        let pcm_before = leaves(&pcm);
        let pcm_identity = project_assets::capture_identity(&pcm).unwrap();
        fs::remove_file(&common).unwrap();
        assert!(
            open_verified_pair(&fixture.root, &reference, &fixture.material, &|| false).is_err()
        );
        let error = {
            let _reset = FaultReset::arm(point);
            match fixture.prepare() {
                Err(error) => error,
                Ok(_) => panic!("half-pair repair did not encounter {point:?}"),
            }
        };
        assert_injected(&error, point);
        assert!(
            !common.exists(),
            "failed repair must not expose common eligibility"
        );
        assert_eq!(
            leaves(&pcm),
            pcm_before,
            "preexisting verified PCM was retired at {point:?}"
        );
        assert_eq!(
            project_assets::capture_identity(&pcm).unwrap(),
            pcm_identity
        );
        assert_eq!(leaves(&fixture.wav_path), wav_before);
        assert_eq!(
            children(&fixture.stem_pcm_base().join(".pairs")),
            Vec::<String>::new()
        );
        let retry = fixture.prepare().unwrap();
        assert!(!retry.created_pcm && retry.created_descriptor);
        assert_eq!(retry.descriptor, descriptor);
        assert_eq!(retry.descriptor_reference, reference);
        assert_eq!(leaves(&pcm), pcm_before);
    }
}

#[test]
fn selected_verified_pair_reuse_never_enters_writer_faults_or_acquires_new_creations() {
    let fixture = Fixture::new(0);
    let pair = fixture.prepare().unwrap();
    let descriptor = pair.descriptor.clone();
    let reference = pair.descriptor_reference.clone();
    drop(pair);
    let pcm = fixture
        .root
        .parent()
        .unwrap()
        .join(&descriptor.pcm_generation);
    let common = fixture.root.parent().unwrap().join(&reference);
    let common_identity = project_assets::capture_identity(&common).unwrap();
    let common_before = fs::read(&common).unwrap();
    let pcm_before = leaves(&pcm);
    let wav_before = leaves(&fixture.wav_path);
    let before = children(&fixture.stem_pcm_base());
    for point in POINTS {
        let _reset = FaultReset::arm(point);
        let reused = fixture.prepare().unwrap();
        assert!(!reused.created_pcm && !reused.created_descriptor);
        assert_eq!(reused.descriptor, descriptor);
        assert_eq!(reused.descriptor_reference, reference);
        drop(reused);
        assert_eq!(
            project_assets::capture_identity(&common).unwrap(),
            common_identity
        );
        assert_eq!(fs::read(&common).unwrap(), common_before);
        assert_eq!(leaves(&pcm), pcm_before);
        assert_eq!(leaves(&fixture.wav_path), wav_before);
        assert_eq!(children(&fixture.stem_pcm_base()), before);
    }
}

#[test]
fn pcm_flush_fault_removes_only_own_leaves_and_preserves_unknown_staging_collision() {
    let fixture = Fixture::new(0);
    let base = fixture.stem_pcm_base();
    let wav_before = leaves(&fixture.wav_path);
    let foreign: RefCell<Option<PathBuf>> = RefCell::new(None);
    let observe_staging = || {
        if foreign.borrow().is_none() && base.is_dir() {
            for entry in fs::read_dir(&base).unwrap().take(3) {
                let entry = entry.unwrap();
                if entry
                    .file_name()
                    .to_str()
                    .unwrap()
                    .starts_with(".generation-")
                {
                    let path = entry.path().join("foreign.keep");
                    fs::write(&path, b"unrecognized interrupted owner data").unwrap();
                    foreign.replace(Some(path));
                    break;
                }
            }
        }
        false
    };
    let error = {
        let _reset = FaultReset::arm(StemPairFault::PcmFlush);
        match prepare_complete_pair(
            &fixture.root,
            &fixture.material,
            &fixture.version,
            &fixture.wav_reference,
            &observe_staging,
        ) {
            Err(error) => error,
            Ok(_) => panic!("expected actual PCM flush fault"),
        }
    };
    assert_injected(&error, StemPairFault::PcmFlush);
    let foreign = foreign
        .into_inner()
        .expect("actual newly created staging was observed");
    let stage = foreign.parent().unwrap();
    assert_eq!(children(stage), ["foreign.keep"]);
    let identity = project_assets::capture_identity(&foreign).unwrap();
    assert_eq!(
        fs::read(&foreign).unwrap(),
        b"unrecognized interrupted owner data"
    );
    assert!(children(&base.join(".pairs")).is_empty());
    assert_eq!(leaves(&fixture.wav_path), wav_before);
    // The deterministic staging target now belongs to unknown data. A retry
    // may not overwrite it or infer rollback permission from its name.
    assert!(fixture.prepare().is_err());
    assert_eq!(children(stage), ["foreign.keep"]);
    assert_eq!(
        project_assets::capture_identity(&foreign).unwrap(),
        identity
    );
    assert_eq!(
        fs::read(&foreign).unwrap(),
        b"unrecognized interrupted owner data"
    );
    assert!(children(&base.join(".pairs")).is_empty());
    assert_eq!(leaves(&fixture.wav_path), wav_before);
}
