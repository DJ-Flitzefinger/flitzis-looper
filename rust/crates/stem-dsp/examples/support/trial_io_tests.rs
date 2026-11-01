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
