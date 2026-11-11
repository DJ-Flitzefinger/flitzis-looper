//! Standalone diagnostic terminal ownership, never a whole-process allowance.
use crate::{
    cancellation,
    trial_io::{self, IoPhase, PrivateOutputs, Result},
};
use std::{
    fs::File,
    io::{self, Write},
    process::ExitCode,
};

/// A duplicate, never the inherited runtime handle. No stdio LineWriter, lazy
/// stream lock or console conversion is used by this diagnostic path. Fixed
/// messages/phases/numeric errors are ASCII. Native File I/O can still wait.
pub struct Native {
    file: Option<File>,
}

impl Native {
    pub fn prepare(id: u32, phase: &'static str) -> (Self, Result<()>) {
        match Self::acquire(id) {
            Ok(file) => (Self { file: Some(file) }, Ok(())),
            Err(error) => (Self { file: None }, Err(error).at(phase)),
        }
    }

    #[cfg(windows)]
    fn acquire(id: u32) -> io::Result<File> {
        use std::{ffi::c_void, os::windows::io::FromRawHandle};
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetStdHandle(id: u32) -> *mut c_void;
            fn GetCurrentProcess() -> *mut c_void;
            fn DuplicateHandle(
                source: *mut c_void,
                handle: *mut c_void,
                target: *mut c_void,
                duplicate: *mut *mut c_void,
                access: u32,
                inherit: i32,
                options: u32,
            ) -> i32;
        }
        let inherited = unsafe { GetStdHandle(id) };
        if inherited.is_null() || inherited as isize == -1 {
            return Err(io::ErrorKind::NotConnected.into());
        }
        let process = unsafe { GetCurrentProcess() };
        let mut duplicate = std::ptr::null_mut();
        // SAFETY: borrow the inherited handle; acquire one noninheritable owner
        // with identical access, without changing/closing the runtime handle.
        if unsafe { DuplicateHandle(process, inherited, process, &mut duplicate, 0, 0, 2) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(duplicate) })
    }

    #[cfg(not(windows))]
    fn acquire(_id: u32) -> io::Result<File> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl Write for Native {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !bytes.is_ascii() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let file = self.file.as_mut().ok_or(io::ErrorKind::NotConnected)?;
        // Retain pinned std File's pending-I/O completion contract. Raw
        // WriteFile(NULL OVERLAPPED) is unsafe for arbitrary inherited handles.
        // No transfer exceeds the existing bounded block; no terminal buffer.
        file.write(&bytes[..bytes.len().min(trial_io::BUFFER_BYTES)])
    }

    fn flush(&mut self) -> io::Result<()> {
        // No user-space pending bytes. Do not FlushFileBuffers on a pipe:
        // peer consumption is the supervisor's separate reader-END contract.
        self.file
            .as_ref()
            .map(|_| ())
            .ok_or(io::ErrorKind::NotConnected.into())
    }
}

pub trait Close: Write {
    fn close(self) -> io::Result<()>;
}

impl Close for Native {
    fn close(mut self) -> io::Result<()> {
        match self.file.take() {
            Some(file) => trial_io::close_file(file),
            None => Ok(()),
        }
    }
}

/// Both owners are consumed even after an earlier error. A line already sent
/// cannot be retracted: only the final exit status qualifies success/130.
pub fn complete(
    result: Result<()>,
    mut cleanup: Result<()>,
    cancel: &cancellation::Cancellation,
    mut status: impl Close,
    mut errors: impl Close,
    private: PrivateOutputs,
) -> ExitCode {
    let code = cancellation::report(result, cleanup, cancel, &mut status, &mut errors);
    trial_io::retain_first(&mut cleanup, status.close().at("status.close"));
    trial_io::retain_first(&mut cleanup, errors.close().at("error.close"));
    // A newly observed marker after terminal closure cannot receive an ACK.
    // Invalidate and return unqualified failure; never invent cancellation130.
    let late = if code == ExitCode::SUCCESS {
        cancel.checkpoint("terminal.closed")
    } else {
        Ok(())
    };
    let successful = code == ExitCode::SUCCESS && cleanup.is_ok() && late.is_ok();
    private.finish(successful, &mut cleanup);
    if cleanup.is_ok() && late.is_ok() {
        code
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;
