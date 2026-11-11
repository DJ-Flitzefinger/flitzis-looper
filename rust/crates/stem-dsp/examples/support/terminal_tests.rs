use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    closes: usize,
}

struct Sink {
    state: Rc<RefCell<State>>,
    write_fail: bool,
    flush_fail: bool,
    close_fail: bool,
    marker: Option<std::path::PathBuf>,
}

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.write_fail {
            return Err(io::Error::from_raw_os_error(109));
        }
        self.state.borrow_mut().bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.flush_fail {
            Err(io::Error::from_raw_os_error(109))
        } else {
            Ok(())
        }
    }
}

impl Close for Sink {
    fn close(self) -> io::Result<()> {
        self.state.borrow_mut().closes += 1;
        if let Some(marker) = self.marker {
            std::fs::write(marker, b"cancel").unwrap();
        }
        if self.close_fail {
            Err(io::Error::from_raw_os_error(6))
        } else {
            Ok(())
        }
    }
}

fn sink() -> Sink {
    Sink {
        state: Rc::new(RefCell::new(State::default())),
        write_fail: false,
        flush_fail: false,
        close_fail: false,
        marker: None,
    }
}

fn prepared(name: &str) -> (std::path::PathBuf, PrivateOutputs) {
    let path = std::env::temp_dir().join(format!("bs-terminal-{}-{name}", std::process::id()));
    let outputs = trial_io::Outputs::prepare(&path, true, true).unwrap();
    let mut private = PrivateOutputs::default();
    let mut cleanup = Ok(());
    trial_io::complete_retained(
        Ok(()),
        trial_io::Inputs::default(),
        Some(outputs),
        &mut cleanup,
        &|| Ok(()),
        &mut private,
    )
    .unwrap();
    assert!(cleanup.is_ok());
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 4);
    (path, private)
}

#[test]
fn success_keeps_private_outputs_until_both_consuming_closes() {
    let (path, private) = prepared("success");
    let status = sink();
    let errors = sink();
    let observed = [status.state.clone(), errors.state.clone()];
    let cancel = cancellation::Cancellation::new(None);
    assert_eq!(
        complete(Ok(()), Ok(()), &cancel, status, errors, private),
        ExitCode::SUCCESS
    );
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 4);
    for state in observed {
        assert_eq!(state.borrow().closes, 1);
    }
    for entry in std::fs::read_dir(&path).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn terminal_write_flush_and_either_close_failure_invalidate_exact_outputs() {
    for failure in 0..4 {
        let (path, private) = prepared(&format!("failure{failure}"));
        let foreign = path.join("foreign");
        std::fs::write(&foreign, b"keep").unwrap();
        let mut status = sink();
        let mut errors = sink();
        let observed = [status.state.clone(), errors.state.clone()];
        status.write_fail = failure == 0;
        status.flush_fail = failure == 1;
        status.close_fail = failure == 2;
        errors.close_fail = failure == 3;
        let result = writeln!(status, "BS completed").at("status.write");
        let cancel = cancellation::Cancellation::new(None);
        assert_eq!(
            complete(result, Ok(()), &cancel, status, errors, private),
            ExitCode::FAILURE
        );
        for state in observed {
            assert_eq!(state.borrow().closes, 1);
        }
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
        assert_eq!(std::fs::read(&foreign).unwrap(), b"keep");
        std::fs::remove_file(foreign).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
}

#[test]
fn original_error_is_reported_before_independent_close_failures() {
    let mut status = sink();
    let mut errors = sink();
    status.close_fail = true;
    errors.close_fail = true;
    let observed = errors.state.clone();
    let cancel = cancellation::Cancellation::new(None);
    assert_eq!(
        complete(
            Err("original work failed".into()),
            Ok(()),
            &cancel,
            status,
            errors,
            PrivateOutputs::default()
        ),
        ExitCode::FAILURE
    );
    assert_eq!(observed.borrow().bytes, b"original work failed\n");
    assert_eq!(observed.borrow().closes, 1);
}

#[test]
fn acknowledgement_requires_both_close_results_and_cleanup() {
    for failure in 0..4 {
        let mut status = sink();
        let mut errors = sink();
        status.close_fail = failure == 1;
        errors.close_fail = failure == 2;
        let observed = [status.state.clone(), errors.state.clone()];
        let cancel = cancellation::Cancellation::new(None);
        let cleanup = if failure == 3 {
            Err("input close failed".into())
        } else {
            Ok(())
        };
        let code = complete(
            Err(flitzis_stem_dsp::Error::Cancelled.into()),
            cleanup,
            &cancel,
            status,
            errors,
            PrivateOutputs::default(),
        );
        assert_eq!(
            code,
            if failure == 0 {
                ExitCode::from(130)
            } else {
                ExitCode::FAILURE
            }
        );
        for state in &observed {
            assert_eq!(state.borrow().closes, 1);
        }
        assert_eq!(
            observed[0]
                .borrow()
                .bytes
                .starts_with(b"FLITZIS_STEM_CANCELLED_V1:"),
            failure != 3
        );
    }
}

#[test]
fn marker_during_final_close_invalidates_without_inventing_ack() {
    let (path, private) = prepared("late");
    let marker = path.join("marker");
    let cancel = cancellation::Cancellation::new(Some(marker.clone()));
    cancel.checkpoint("before-status").unwrap();
    let status = sink();
    let mut errors = sink();
    errors.marker = Some(marker.clone());
    let observed = status.state.clone();
    assert_eq!(
        complete(Ok(()), Ok(()), &cancel, status, errors, private),
        ExitCode::FAILURE
    );
    assert!(observed.borrow().bytes.is_empty());
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
    std::fs::remove_file(marker).unwrap();
    std::fs::remove_dir(path).unwrap();
}

#[cfg(windows)]
#[test]
fn native_write_is_bounded_ascii_and_consuming_close_is_observable() {
    use std::os::windows::io::AsRawHandle;
    let path = std::env::temp_dir().join(format!("bs-native-terminal-{}", std::process::id()));
    let file = File::create_new(&path).unwrap();
    let handle = file.as_raw_handle();
    let mut terminal = Native { file: Some(file) };
    assert_eq!(
        terminal.write(&[b'x'; trial_io::BUFFER_BYTES + 1]).unwrap(),
        trial_io::BUFFER_BYTES
    );
    assert_eq!(
        terminal.write("ä".as_bytes()).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    terminal.flush().unwrap();
    terminal.close().unwrap();
    trial_io::observe_closed_control(handle).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        trial_io::BUFFER_BYTES as u64
    );
    std::fs::remove_file(path).unwrap();
}
