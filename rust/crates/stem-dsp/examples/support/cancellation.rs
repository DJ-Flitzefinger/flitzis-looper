//! One standalone invocation's explicit supervisor channel. No watcher or child.
use crate::trial_io::{Failure, IoPhase, Result};
use flitzis_stem_dsp::Error;
use std::{
    cell::Cell,
    io::Write,
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(20);

pub struct Cancellation {
    marker: Option<PathBuf>,
    last: Cell<Option<Instant>>,
    observed: Cell<bool>,
    stage: Cell<&'static str>,
}

impl Cancellation {
    pub fn from_environment() -> Self {
        Self::new(
            std::env::var_os("FLITZIS_STEM_CANCEL_PATH")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from),
        )
    }

    fn new(marker: Option<PathBuf>) -> Self {
        Self {
            marker,
            last: Cell::new(None),
            observed: Cell::new(false),
            stage: Cell::new("startup"),
        }
    }

    pub fn bound(&self) -> bool {
        self.marker.is_some()
    }

    /// Core callbacks can be frequent. Only time boundaries access the marker;
    /// native FFT/SGEMM/file calls themselves are not interruptible by this hook.
    pub fn cancelled(&self) -> bool {
        if self.observed.get() {
            return true;
        }
        let Some(marker) = &self.marker else {
            return false;
        };
        let now = Instant::now();
        if self
            .last
            .get()
            .is_none_or(|last| now.duration_since(last) >= POLL)
        {
            self.last.set(Some(now));
            self.observed.set(marker.is_file());
        }
        self.observed.get()
    }

    /// Phase changes force a fresh read, including immediately before success.
    pub fn checkpoint(&self, stage: &'static str) -> Result<()> {
        if !self.observed.get() {
            self.stage.set(stage);
        }
        self.last.set(None);
        self.check()
    }

    pub fn check(&self) -> Result<()> {
        if self.cancelled() {
            Err(Error::Cancelled.into())
        } else {
            Ok(())
        }
    }
}

/// Cancellation is qualified only after independent cleanup and both borrowed
/// terminal sinks succeed. ACK is not a process-tree/pipe-reader END receipt.
pub fn report(
    result: Result<()>,
    cleanup: Result<()>,
    cancel: &Cancellation,
    status: &mut impl Write,
    errors: &mut impl Write,
) -> ExitCode {
    if matches!(result, Err(Failure::Core(Error::Cancelled))) && cleanup.is_ok() {
        let terminal = (|| {
            status.flush().at("status.flush")?;
            errors.flush().at("error.flush")?;
            writeln!(
                status,
                "FLITZIS_STEM_CANCELLED_V1:{{\"stage\":\"{}\"}}",
                cancel.stage.get()
            )
            .at("cancel.write")?;
            status.flush().at("cancel.flush")
        })();
        if terminal.is_ok() {
            return ExitCode::from(130);
        }
        let _ = crate::trial_io::report_terminal(terminal, status, errors);
        return ExitCode::FAILURE;
    }
    let result = crate::trial_io::report_terminal(result, status, errors);
    let cleanup = if cleanup.is_err() {
        crate::trial_io::report_terminal(cleanup, status, errors)
    } else {
        cleanup
    };
    if result.is_ok() && cleanup.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_exact_forced_at_boundaries_and_latched() {
        let directory = std::env::temp_dir().join(format!("bs-cancel-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("own");
        let cancel = Cancellation::new(Some(path.clone()));
        cancel.checkpoint("initial").unwrap();
        std::fs::write(directory.join("foreign"), b"cancel").unwrap();
        cancel.checkpoint("foreign").unwrap();
        std::fs::write(&path, b"cancel").unwrap();
        assert!(matches!(
            cancel.checkpoint("own"),
            Err(Failure::Core(Error::Cancelled))
        ));
        std::fs::remove_file(path).unwrap();
        assert!(cancel.cancelled());
        assert!(cancel.checkpoint("later").is_err());
        assert_eq!(cancel.stage.get(), "own");
        std::fs::remove_file(directory.join("foreign")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn qualified_ack_requires_cleanup_and_both_terminal_sinks() {
        struct Sink {
            bytes: Vec<u8>,
            fail: bool,
        }
        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                if self.fail {
                    Err(std::io::ErrorKind::BrokenPipe.into())
                } else {
                    Ok(())
                }
            }
        }
        for (cleanup_failed, stdout_failed, stderr_failed) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let cancel = Cancellation::new(None);
            cancel.stage.set("model.masks");
            let mut status = Sink {
                bytes: Vec::new(),
                fail: stdout_failed,
            };
            let mut errors = Sink {
                bytes: Vec::new(),
                fail: stderr_failed,
            };
            let cleanup = if cleanup_failed {
                Err("unresolved close".into())
            } else {
                Ok(())
            };
            let code = report(
                Err(Error::Cancelled.into()),
                cleanup,
                &cancel,
                &mut status,
                &mut errors,
            );
            if cleanup_failed || stdout_failed || stderr_failed {
                assert_eq!(code, ExitCode::FAILURE);
                assert!(status.bytes.is_empty());
            } else {
                assert_eq!(code, ExitCode::from(130));
                assert_eq!(
                    status.bytes,
                    b"FLITZIS_STEM_CANCELLED_V1:{\"stage\":\"model.masks\"}\n"
                );
                assert!(errors.bytes.is_empty());
            }
        }
    }
}
