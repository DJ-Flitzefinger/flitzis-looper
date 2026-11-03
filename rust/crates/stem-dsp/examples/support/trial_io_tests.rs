use super::*;
use std::{cell::RefCell, panic::AssertUnwindSafe};

#[derive(Default)]
struct Observed {
    bytes: [u8; 16],
    len: usize,
    writes: usize,
    flushes: usize,
    closes: usize,
}

struct Sink<'a> {
    observed: &'a RefCell<Observed>,
    fail_after: Option<usize>,
    fail_flush: bool,
    panic_write: bool,
}

impl Write for Sink<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.observed.borrow_mut();
        state.writes += 1;
        assert!(!self.panic_write, "injected writer panic");
        let available = self.fail_after.unwrap_or(state.bytes.len()) - state.len;
        if available == 0 {
            return Err(io::Error::from_raw_os_error(5));
        }
        let len = available.min(bytes.len());
        let start = state.len;
        state.bytes[start..start + len].copy_from_slice(&bytes[..len]);
        state.len += len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.observed.borrow_mut().flushes += 1;
        if self.fail_flush {
            Err(io::Error::from_raw_os_error(6))
        } else {
            Ok(())
        }
    }
}

impl Drop for Sink<'_> {
    fn drop(&mut self) {
        self.observed.borrow_mut().closes += 1;
    }
}

fn buffered(observed: &RefCell<Observed>) -> ExplicitBuffer<Sink<'_>> {
    ExplicitBuffer::new(Sink {
        observed,
        fail_after: None,
        fail_flush: false,
        panic_write: false,
    })
}

#[test]
fn abandoned_pending_bytes_never_write_on_return_or_caller_unwind() {
    let observed = RefCell::new(Observed::default());
    let mut writer = buffered(&observed);
    writer.write_all(b"pending").unwrap();
    assert_eq!(observed.borrow().writes, 0);
    drop(writer);
    assert_eq!(observed.borrow().writes, 0);
    assert_eq!(observed.borrow().flushes, 0);
    assert_eq!(observed.borrow().closes, 1);
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut writer = buffered(&observed);
        writer.write_all(b"abandoned").unwrap();
        panic!("injected caller failure");
    }));
    assert!(result.is_err());
    assert_eq!(observed.borrow().writes, 0);
    assert_eq!(observed.borrow().flushes, 0);
    assert_eq!(observed.borrow().closes, 2);
}

#[test]
fn partial_flush_failure_preserves_numeric_error_and_never_retries_on_drop() {
    let observed = RefCell::new(Observed::default());
    let mut writer = ExplicitBuffer::new(Sink {
        observed: &observed,
        fail_after: Some(2),
        fail_flush: false,
        panic_write: false,
    });
    writer.write_all(b"123456").unwrap();
    let error = writer.flush().at("output.pcm").unwrap_err();
    check(error, b"BS trial I/O failed: phase=output.pcm os_error=5");
    assert_eq!(observed.borrow().writes, 2);
    assert_eq!(observed.borrow().len, 2);
    drop(writer);
    let state = observed.borrow();
    assert_eq!(state.writes, 2);
    assert_eq!(state.flushes, 0);
    assert_eq!(state.closes, 1);
    assert_eq!(&state.bytes[..state.len], b"12");
}

#[test]
fn explicit_flush_preserves_bytes_and_sink_flush_failures_without_drop_io() {
    for fail_flush in [false, true] {
        let observed = RefCell::new(Observed::default());
        let mut writer = ExplicitBuffer::new(Sink {
            observed: &observed,
            fail_after: None,
            fail_flush,
            panic_write: false,
        });
        writer.write_all(b"exact").unwrap();
        let result = writer.flush();
        if fail_flush {
            assert_eq!(result.unwrap_err().raw_os_error(), Some(6));
        } else {
            result.unwrap();
        }
        drop(writer);
        let state = observed.borrow();
        assert_eq!(&state.bytes[..state.len], b"exact");
        assert_eq!(state.writes, 1);
        assert_eq!(state.flushes, 1);
        assert_eq!(state.closes, 1);
    }
}

#[test]
fn inner_writer_panic_still_closes_once_without_drop_write() {
    let observed = RefCell::new(Observed::default());
    let mut writer = ExplicitBuffer::new(Sink {
        observed: &observed,
        fail_after: None,
        fail_flush: false,
        panic_write: true,
    });
    writer.write_all(b"pending").unwrap();
    assert!(std::panic::catch_unwind(AssertUnwindSafe(|| writer.flush())).is_err());
    drop(writer);
    let state = observed.borrow();
    assert_eq!(state.writes, 1);
    assert_eq!(state.flushes, 0);
    assert_eq!(state.closes, 1);
    assert_eq!(state.len, 0);
}

// A stack-only writer makes the error report exercise the actual Display path
// without String or OS-message formatting. No allocator-counter inference.
struct Fixed {
    bytes: [u8; 256],
    len: usize,
}

impl Write for Fixed {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.len + bytes.len();
        if end > self.bytes.len() {
            return Err(io::ErrorKind::WriteZero.into());
        }
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn check(error: Failure, expected: &[u8]) {
    let mut target = Fixed {
        bytes: [0; 256],
        len: 0,
    };
    write!(target, "{error}").unwrap();
    assert_eq!(&target.bytes[..target.len], expected);
}

#[test]
fn io_failures_keep_phase_and_numeric_code_without_platform_text() {
    check(
        Err::<(), _>(io::Error::from_raw_os_error(5))
            .at("output.pcm")
            .unwrap_err(),
        b"BS trial I/O failed: phase=output.pcm os_error=5",
    );
    check(
        Err::<(), _>(io::Error::from_raw_os_error(-7))
            .at("source.read")
            .unwrap_err(),
        b"BS trial I/O failed: phase=source.read os_error=-7",
    );
    check(
        Err::<(), _>(io::Error::from(io::ErrorKind::UnexpectedEof))
            .at("weights.read")
            .unwrap_err(),
        b"BS trial I/O failed: phase=weights.read os_error=unavailable",
    );
    assert!(
        Ok::<_, io::Error>(17)
            .at("source.read")
            .is_ok_and(|n| n == 17)
    );
}

#[test]
fn core_allocation_phase_and_exact_requested_bytes_survive_reporting() {
    check(
        Error::Allocation {
            phase: "learned.axial",
            requested_bytes: 104850432,
        }
        .into(),
        b"BS allocation failed: phase=learned.axial requested_bytes=104850432",
    );
    check(Error::Cancelled.into(), b"BS spectral work cancelled");
    check(
        "incomplete private track".into(),
        b"incomplete private track",
    );
}

#[test]
fn callback_boundary_preserves_first_io_failure_and_core_error() {
    let mut first = None;
    assert_eq!(
        core_io(Ok(17), "source.read", &mut first, Error::Source),
        Ok(17)
    );
    assert!(first.is_none());
    assert_eq!(
        core_io(
            Err::<(), _>(io::Error::from_raw_os_error(5)),
            "output.pcm.write",
            &mut first,
            Error::Output,
        ),
        Err(Error::Output)
    );
    assert_eq!(
        core_io(
            Err::<(), _>(io::Error::from_raw_os_error(6)),
            "later.failure",
            &mut first,
            Error::Source,
        ),
        Err(Error::Source)
    );
    check(
        first.unwrap(),
        b"BS trial I/O failed: phase=output.pcm.write os_error=5",
    );
}

#[derive(Default)]
struct CompletionObserved {
    bytes: Vec<u8>,
    calls: Vec<&'static str>,
    drops: usize,
}

struct Completing<'a> {
    observed: &'a RefCell<CompletionObserved>,
    failure: Option<&'static str>,
}

impl Write for Completing<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut observed = self.observed.borrow_mut();
        observed.calls.push("write");
        if self.failure == Some("write") {
            return Err(io::Error::from_raw_os_error(5));
        }
        // A real short successful sink write exercises the retained BufWriter's
        // write_all path, without substituting a different completion algorithm.
        let count = bytes.len().min(2);
        observed.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.observed.borrow_mut().calls.push("flush");
        if self.failure == Some("flush") {
            Err(io::Error::from_raw_os_error(6))
        } else {
            Ok(())
        }
    }
}

impl CompleteSink for Completing<'_> {
    fn sync(&mut self) -> io::Result<()> {
        self.observed.borrow_mut().calls.push("sync");
        if self.failure == Some("sync") {
            Err(io::Error::from_raw_os_error(7))
        } else {
            Ok(())
        }
    }

    fn close(self) -> io::Result<()> {
        self.observed.borrow_mut().calls.push("close");
        if self.failure == Some("close") {
            Err(io::Error::from_raw_os_error(8))
        } else {
            Ok(())
        }
    }
}

impl Drop for Completing<'_> {
    fn drop(&mut self) {
        self.observed.borrow_mut().drops += 1;
    }
}

#[test]
fn consuming_completion_orders_short_writes_flush_sync_and_one_close() {
    let observed = RefCell::new(CompletionObserved::default());
    let mut writer = ExplicitBuffer::new(Completing {
        observed: &observed,
        failure: None,
    });
    writer.write_all(b"exact").unwrap();
    complete_buffer(writer, true).unwrap();
    let state = observed.borrow();
    assert_eq!(state.bytes, b"exact");
    assert_eq!(
        state.calls,
        ["write", "write", "write", "flush", "sync", "close"]
    );
    assert_eq!(state.drops, 1);
}

#[test]
fn consuming_failure_still_closes_once_without_flush_retry() {
    for (failure, phase, code, calls) in [
        ("write", "output.flush", 5, vec!["write", "close"]),
        ("flush", "output.flush", 6, vec!["write", "flush", "close"]),
        (
            "sync",
            "output.sync",
            7,
            vec!["write", "flush", "sync", "close"],
        ),
        (
            "close",
            "output.close",
            8,
            vec!["write", "flush", "sync", "close"],
        ),
    ] {
        let observed = RefCell::new(CompletionObserved::default());
        let mut writer = ExplicitBuffer::new(Completing {
            observed: &observed,
            failure: Some(failure),
        });
        writer.write_all(b"12").unwrap();
        let error = complete_buffer(writer, true).unwrap_err();
        assert!(
            matches!(error, Failure::Io { phase: actual, os_error: Some(actual_code) } if actual == phase && actual_code == code)
        );
        let state = observed.borrow();
        assert_eq!(state.calls, calls);
        assert_eq!(state.drops, 1);
    }
}

#[test]
fn failed_work_abandons_bytes_but_consumes_every_close_and_keeps_original_error() {
    let observed = RefCell::new(CompletionObserved::default());
    let mut result: Result<()> = Err("original work failure".into());
    for failure in [Some("close"), None, Some("write")] {
        let mut writer = ExplicitBuffer::new(Completing {
            observed: &observed,
            failure,
        });
        writer.write_all(b"pending").unwrap();
        retain_first(&mut result, complete_buffer(writer, false));
    }
    check(result.unwrap_err(), b"original work failure");
    let state = observed.borrow();
    assert_eq!(state.calls, ["close", "close", "close"]);
    assert!(state.bytes.is_empty());
    assert_eq!(state.drops, 3);
}

#[test]
fn first_completion_failure_does_not_skip_later_successful_work_sinks() {
    let observed = RefCell::new(CompletionObserved::default());
    let mut result = Ok(());
    for failure in [Some("sync"), Some("close"), None] {
        let mut writer = ExplicitBuffer::new(Completing {
            observed: &observed,
            failure,
        });
        writer.write_all(b"12").unwrap();
        retain_first(&mut result, complete_buffer(writer, true));
    }
    check(
        result.unwrap_err(),
        b"BS trial I/O failed: phase=output.sync os_error=7",
    );
    let state = observed.borrow();
    assert_eq!(
        state.calls,
        [
            "write", "flush", "sync", "close", "write", "flush", "sync", "close", "write", "flush",
            "sync", "close"
        ]
    );
    assert_eq!(state.drops, 3);
}

#[test]
fn terminal_report_checks_both_sinks_once_and_keeps_first_failure() {
    for status_failure in [None, Some("flush")] {
        for error_failure in [None, Some("write"), Some("flush")] {
            let status = RefCell::new(CompletionObserved::default());
            let errors = RefCell::new(CompletionObserved::default());
            let mut status_sink = Completing {
                observed: &status,
                failure: status_failure,
            };
            let mut error_sink = Completing {
                observed: &errors,
                failure: error_failure,
            };
            let result = report_terminal(Err("original".into()), &mut status_sink, &mut error_sink);
            check(result.unwrap_err(), b"original");
            assert_eq!(status.borrow().calls, ["flush"]);
            let observed = errors.borrow();
            assert_eq!(
                observed
                    .calls
                    .iter()
                    .filter(|&&call| call == "flush")
                    .count(),
                1
            );
            if error_failure == Some("write") {
                assert_eq!(observed.calls, ["write", "flush"]);
            } else {
                assert_eq!(observed.bytes, b"original\n");
            }
        }
    }
}

#[test]
fn successful_terminal_status_flush_failure_is_a_failure() {
    let status = RefCell::new(CompletionObserved::default());
    let errors = RefCell::new(CompletionObserved::default());
    let mut status_sink = Completing {
        observed: &status,
        failure: Some("flush"),
    };
    let mut error_sink = Completing {
        observed: &errors,
        failure: None,
    };
    check(
        report_terminal(Ok(()), &mut status_sink, &mut error_sink).unwrap_err(),
        b"BS trial I/O failed: phase=status.flush os_error=6",
    );
    assert_eq!(status.borrow().calls, ["flush"]);
    assert_eq!(errors.borrow().calls.last(), Some(&"flush"));
}

#[test]
fn successful_terminal_flush_has_no_diagnostic_write() {
    let status = RefCell::new(CompletionObserved::default());
    let errors = RefCell::new(CompletionObserved::default());
    let mut status_sink = Completing {
        observed: &status,
        failure: None,
    };
    let mut error_sink = Completing {
        observed: &errors,
        failure: None,
    };
    report_terminal(Ok(()), &mut status_sink, &mut error_sink).unwrap();
    assert_eq!(status.borrow().calls, ["flush"]);
    assert!(errors.borrow().calls.is_empty());
}

fn fixture_directory() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "bs-completion-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn incomplete_input_preparation_closes_retained_readers_and_keeps_original_failure() {
    for invalid_size in [false, true] {
        let directory = fixture_directory();
        let source = directory.join("source");
        std::fs::write(&source, b"tiny").unwrap();
        let mut inputs = Inputs::default();
        let mut result = inputs.prepare(
            &source,
            if invalid_size { 5 } else { 4 },
            &directory.join("absent-weights"),
            4,
            None,
        );
        assert!(inputs.source.is_some());
        assert!(inputs.weights.is_none());
        #[cfg(windows)]
        let handle = std::os::windows::io::AsRawHandle::as_raw_handle(
            inputs.source.as_ref().unwrap().get_ref(),
        );
        inputs.complete(&mut result);
        if invalid_size {
            check(result.unwrap_err(), b"incomplete source");
        } else {
            assert!(matches!(
                result,
                Err(Failure::Io {
                    phase: "weights.open",
                    ..
                })
            ));
        }
        #[cfg(windows)]
        observe_closed_control(handle).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn actual_output_failure_still_completes_later_file_owners() {
    let directory = fixture_directory();
    let read_only = directory.join("read-only");
    std::fs::write(&read_only, b"original").unwrap();
    let mut outputs = ChunkOutputs {
        features: Output {
            writer: ExplicitBuffer::new(File::open(&read_only).unwrap()),
            phase: "output.features",
        },
        masks: Output::prepare(&directory, "masks", "output.masks").unwrap(),
        prediction: Output::prepare(&directory, "pcm", "output.pcm").unwrap(),
        original_masks: Some(
            Output::prepare(&directory, "original", "output.original-masks").unwrap(),
        ),
    };
    for output in [
        &mut outputs.features,
        &mut outputs.masks,
        &mut outputs.prediction,
        outputs.original_masks.as_mut().unwrap(),
    ] {
        output.write_values(&[0.25]).unwrap();
    }
    let mut result = Ok(());
    Outputs::Chunk(outputs).complete(true, &mut result);
    assert!(matches!(
        result,
        Err(Failure::Io {
            phase: "output.flush",
            os_error: Some(_)
        })
    ));
    assert_eq!(std::fs::read(&read_only).unwrap(), b"original");
    for name in ["masks", "pcm", "original"] {
        assert_eq!(
            std::fs::read(directory.join(name)).unwrap(),
            0.25f32.to_le_bytes()
        );
    }
}
