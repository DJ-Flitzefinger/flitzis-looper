use super::*;

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
