use super::*;
use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "beatthis-own-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    path
}
fn own(marker: Option<PathBuf>) -> Invocation {
    Invocation {
        context: Context::new(marker),
        paths: Vec::new(),
        profile_directory: None,
    }
}

#[test]
fn exact_marker_latches_and_late_completion_invalidates_closed_output() {
    let directory = fixture();
    let marker = directory.join("own");
    let foreign = directory.join("foreign");
    std::fs::write(&foreign, b"cancel").unwrap();
    let mut invocation = own(Some(marker.clone()));
    invocation.context.checkpoint("before").unwrap();
    let output = directory.join("partial");
    invocation
        .write_floats(&output, &[0.0, -0.0, 0.25])
        .unwrap();
    assert_eq!(
        std::fs::read(&output).unwrap(),
        [0.0f32, -0.0, 0.25]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    std::fs::write(&marker, b"cancel").unwrap();
    let (mut status, mut errors) = (Vec::new(), Vec::new());
    assert_eq!(
        invocation.finish(
            Ok(serde_json::json!({"ok":true})),
            None,
            &mut status,
            &mut errors
        ),
        ExitCode::from(130)
    );
    assert_eq!(
        status,
        b"FLITZIS_STEM_CANCELLED_V1:{\"stage\":\"completion\"}\n"
    );
    assert!(errors.is_empty());
    assert!(!output.exists());
    assert_eq!(std::fs::read(&foreign).unwrap(), b"cancel");
    let context = Context::new(Some(marker.clone()));
    assert!(context.checkpoint("own").is_err());
    std::fs::remove_file(&marker).unwrap();
    assert!(context.cancelled());
    assert_eq!(context.stage(), "own");
    std::fs::remove_file(foreign).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn failed_write_and_collision_preserve_foreign_files_and_primary_error() {
    let directory = fixture();
    let existing = directory.join("foreign");
    std::fs::write(&existing, b"original").unwrap();
    let mut invocation = own(None);
    assert!(invocation.write_floats(&existing, &[1.0]).is_err());
    let output = directory.join("partial");
    let primary = invocation
        .write(&output, |file, _| {
            file.write_all(b"partial").unwrap();
            Err("primary write failure".into())
        })
        .unwrap_err();
    invocation
        .context
        .record(Err("later close qualification failure".into()));
    let (mut status, mut errors) = (Vec::new(), Vec::new());
    assert_eq!(
        invocation.finish(Err(primary), None, &mut status, &mut errors),
        ExitCode::FAILURE
    );
    let errors = String::from_utf8(errors).unwrap();
    assert!(errors.starts_with("Error: primary write failure\n"));
    assert!(errors.contains("Cleanup: later close qualification failure"));
    assert!(!output.exists());
    assert_eq!(std::fs::read(&existing).unwrap(), b"original");
    std::fs::remove_file(existing).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn profile_failure_is_independent_and_private_profile_files_are_invalidated() {
    let directory = fixture();
    let mut invocation = own(None);
    let prefix = invocation.profile(&directory.join("requested")).unwrap();
    let actual = prefix.with_extension("actual.json");
    std::fs::write(&actual, b"partial profile").unwrap();
    let calls = AtomicUsize::new(0);
    let mut result: Result<(), String> = Err("primary run".into());
    assert!(
        invocation
            .finish_profile_with(&mut result, || {
                calls.fetch_add(1, Ordering::Relaxed);
                Err("profile close".into())
            })
            .is_none()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(result.unwrap_err(), "primary run");
    assert!(
        invocation
            .context
            .cleanup()
            .unwrap_err()
            .contains("profile close")
    );
    let (mut status, mut errors) = (Vec::new(), Vec::new());
    assert_eq!(
        invocation.finish(Err(CANCELLED.into()), None, &mut status, &mut errors),
        ExitCode::FAILURE
    );
    assert!(status.is_empty());
    assert!(!actual.exists());
    assert!(std::fs::read_dir(&directory).unwrap().next().is_none());
    std::fs::remove_dir(directory).unwrap();
}

struct Terminal {
    bytes: Vec<u8>,
    fail_write: bool,
    fail_flush: bool,
}
impl Write for Terminal {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.fail_write {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_flush {
            Err(std::io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn terminal_failure_suppresses_ack_and_invalidates_success_files() {
    for cancel in [false, true] {
        for (write, flush, stderr) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let directory = fixture();
            let output = directory.join("output");
            let mut invocation = own(None);
            invocation.write_floats(&output, &[1.0]).unwrap();
            let mut status = Terminal {
                bytes: Vec::new(),
                fail_write: write,
                fail_flush: flush,
            };
            let mut errors = Terminal {
                bytes: Vec::new(),
                fail_write: false,
                fail_flush: stderr,
            };
            let result = if cancel {
                Err(CANCELLED.into())
            } else {
                Ok(serde_json::json!({"ok":true}))
            };
            assert_eq!(
                invocation.finish(result, None, &mut status, &mut errors),
                ExitCode::FAILURE
            );
            assert!(status.bytes.is_empty());
            assert!(!output.exists());
            std::fs::remove_dir(directory).unwrap();
        }
    }
}

#[test]
fn termination_helper_joins_before_work_borrows_end_and_retains_both_failures() {
    let directory = fixture();
    let marker = directory.join("cancel");
    let context = Context::new(Some(marker.clone()));
    let terminated = AtomicBool::new(false);
    let (result, requested) = terminate_during(
        &context,
        None,
        || {
            terminated.store(true, Ordering::Release);
            Err("termination qualification".into())
        },
        || {
            std::fs::write(&marker, b"cancel").unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while !terminated.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            Err::<(), _>("primary run".into())
        },
    );
    assert!(!requested); // a failed API call is not successful termination
    assert_eq!(result.unwrap_err(), "primary run");
    assert_eq!(context.cleanup().unwrap_err(), "termination qualification");
    std::fs::remove_file(marker).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn fast_run_wakes_and_joins_diagnostic_helper_without_waiting_for_timer() {
    let context = Context::new(None);
    let terminated = AtomicBool::new(false);
    let (result, requested) = terminate_during(
        &context,
        Some(1000),
        || {
            terminated.store(true, Ordering::Release);
            Ok(())
        },
        || Ok(7),
    );
    assert_eq!(result.unwrap(), 7);
    assert!(!requested);
    assert!(!terminated.load(Ordering::Acquire));
    assert!(context.cleanup().is_ok());
}

#[test]
fn panicking_work_wakes_the_scoped_helper_before_unwinding_its_owner() {
    let directory = fixture();
    let context = Context::new(Some(directory.join("absent")));
    let panic = std::panic::catch_unwind(|| {
        terminate_during(
            &context,
            None,
            || panic!("unexpected terminate"),
            || -> Result<(), String> {
                panic!("primary work panic");
            },
        )
    });
    assert!(panic.is_err());
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn input_error_closes_reader_and_success_preserves_exact_bytes() {
    let directory = fixture();
    let path = directory.join("input");
    std::fs::write(&path, [0u8; 17]).unwrap();
    let invocation = own(None);
    assert!(
        invocation
            .read_checked(&path, 32, |_| Err("geometry".into()))
            .is_err()
    );
    assert_eq!(invocation.read(&path, 32).unwrap().0, [0u8; 17]);
    assert!(invocation.context.cleanup().is_ok());
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let exclusive = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        flitzis_looper_analysis::trial_lifetime::close_file(exclusive).unwrap();
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn concurrent_marker_and_successful_termination_do_not_hide_unrelated_native_error() {
    let directory = fixture();
    let marker = directory.join("cancel");
    let context = Context::new(Some(marker.clone()));
    std::fs::write(&marker, b"cancel").unwrap();
    context.checkpoint("model.run").unwrap_err();
    let ordinary = qualify_run::<()>(Err("CUDA failure".into()), false, true, &context);
    assert_eq!(ordinary.unwrap_err(), "CUDA failure");
    let failed_terminate =
        qualify_run::<()>(Err("native termination".into()), true, false, &context);
    assert_eq!(failed_terminate.unwrap_err(), "native termination");
    let cancelled = qualify_run::<()>(Err("exact native termination".into()), true, true, &context);
    assert_eq!(
        cancelled.unwrap_err(),
        format!("{CANCELLED}; native run: exact native termination")
    );
    std::fs::remove_file(marker).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn helper_failure_keeps_a_successful_owner_available_for_explicit_completion() {
    let directory = fixture();
    let marker = directory.join("cancel");
    let context = Context::new(Some(marker.clone()));
    std::fs::write(&marker, b"cancel").unwrap();
    let called = AtomicBool::new(false);
    let (result, terminated) = terminate_during(
        &context,
        None,
        || {
            called.store(true, Ordering::Release);
            Err("load cancel failed".into())
        },
        || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !called.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            Ok("constructed session owner")
        },
    );
    assert_eq!(result.unwrap(), "constructed session owner");
    assert!(!terminated);
    assert_eq!(context.cleanup().unwrap_err(), "load cancel failed");
    std::fs::remove_file(marker).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
