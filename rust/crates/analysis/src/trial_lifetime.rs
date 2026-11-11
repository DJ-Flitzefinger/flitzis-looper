//! Standalone offline trial custody. Never used on the audio callback.
use std::{
    fs::File,
    io,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const CANCELLED: &str = "native BeatThis trial cancelled";

struct State {
    marker: Option<PathBuf>,
    start: Instant,
    next_poll: AtomicU64,
    observed: AtomicBool,
    stage: Mutex<&'static str>,
    cleanup: Mutex<Option<String>>,
}

/// One exact invocation marker, latched and shared with its scoped ORT helper.
#[derive(Clone)]
pub struct Context(Arc<State>, &'static str);

impl Context {
    pub fn new(marker: Option<PathBuf>) -> Self {
        Self(
            Arc::new(State {
                marker,
                start: Instant::now(),
                next_poll: AtomicU64::new(0),
                observed: AtomicBool::new(false),
                stage: Mutex::new("startup"),
                cleanup: Mutex::new(None),
            }),
            CANCELLED,
        )
    }

    /// Keep the same marker, latch and cleanup owners with another model label.
    #[cfg(feature = "demucs-inference-trial")]
    pub(crate) fn with_cancel_message(&self, message: &'static str) -> Self {
        Self(Arc::clone(&self.0), message)
    }

    pub fn from_environment() -> Self {
        Self::new(
            std::env::var_os("FLITZIS_STEM_CANCEL_PATH")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from),
        )
    }

    pub fn bound(&self) -> bool {
        self.0.marker.is_some()
    }

    /// Frequent numeric callbacks access the filesystem at most every 20ms.
    pub fn cancelled(&self) -> bool {
        if self.0.observed.load(Ordering::Acquire) {
            return true;
        }
        let Some(marker) = &self.0.marker else {
            return false;
        };
        let now = self.0.start.elapsed().as_millis() as u64 + 1;
        let next = self.0.next_poll.load(Ordering::Relaxed);
        if now >= next
            && self
                .0
                .next_poll
                .compare_exchange(next, now + 20, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            && marker.is_file()
        {
            self.0.observed.store(true, Ordering::Release);
        }
        self.0.observed.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), String> {
        if self.cancelled() {
            Err(self.1.into())
        } else {
            Ok(())
        }
    }

    /// Named boundaries always inspect the exact marker afresh.
    pub fn checkpoint(&self, stage: &'static str) -> Result<(), String> {
        if !self.0.observed.load(Ordering::Acquire) {
            *self
                .0
                .stage
                .lock()
                .map_err(|_| "cancellation stage poisoned")? = stage;
        }
        self.0.next_poll.store(0, Ordering::Release);
        self.check()
    }

    pub fn stage(&self) -> &'static str {
        self.0.stage.lock().map_or("unknown", |s| *s)
    }

    /// Cleanup never replaces a primary work failure and independently gates ACK.
    pub fn record(&self, result: Result<(), String>) {
        if let Err(error) = result {
            let mut first = self.0.cleanup.lock().unwrap_or_else(|e| e.into_inner());
            first.get_or_insert(error);
        }
    }

    pub fn cleanup(&self) -> Result<(), String> {
        match &*self
            .0
            .cleanup
            .lock()
            .map_err(|_| "cleanup state poisoned")?
        {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub fn file(&self, file: File, phase: &'static str) -> OwnedFile {
        OwnedFile {
            file: Some(file),
            context: self.clone(),
            phase,
        }
    }

    pub fn library(&self, library: libloading::Library) -> OwnedLibrary {
        OwnedLibrary {
            library: Some(library),
            context: self.clone(),
        }
    }
}

/// Consume the local module reference once. libloading's ordinary Drop ignores
/// release errors; keep them in independent completion qualification instead.
pub struct OwnedLibrary {
    library: Option<libloading::Library>,
    context: Context,
}
impl OwnedLibrary {
    pub fn complete(mut self) -> Result<(), String> {
        self.finish()
    }
    fn finish(&mut self) -> Result<(), String> {
        let result = self.library.take().map_or(Ok(()), |library| {
            library
                .close()
                .map_err(|e| format!("native.library.close: {e}"))
        });
        self.context.record(result.clone());
        result
    }
}
impl Deref for OwnedLibrary {
    type Target = libloading::Library;
    fn deref(&self) -> &Self::Target {
        self.library.as_ref().expect("live module reference")
    }
}
impl Drop for OwnedLibrary {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Consumes the unique descriptor before one native close; never retry a close.
pub fn close_file(file: File) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::{ffi::c_void, os::windows::io::IntoRawHandle};
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let handle = file.into_raw_handle();
        // SAFETY: unique File ownership was consumed; no destructor can retry.
        if unsafe { CloseHandle(handle) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(unix)]
    {
        use std::os::fd::IntoRawFd;
        unsafe extern "C" {
            fn close(fd: i32) -> i32;
        }
        // Never retry EINTR: the descriptor may already have been released.
        if unsafe { close(file.into_raw_fd()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        drop(file);
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Error exits also consume and check close, recording unresolved ownership.
/// Callers explicitly complete ordinary readers/writers before final reporting.
pub struct OwnedFile {
    file: Option<File>,
    context: Context,
    phase: &'static str,
}

impl OwnedFile {
    pub fn complete(mut self) -> Result<(), String> {
        self.finish()
    }
    fn finish(&mut self) -> Result<(), String> {
        let result = self.file.take().map_or(Ok(()), |f| {
            close_file(f).map_err(|e| format!("{}: {e}", self.phase))
        });
        self.context.record(result.clone());
        result
    }
}
impl Deref for OwnedFile {
    type Target = File;
    fn deref(&self) -> &File {
        self.file.as_ref().expect("live owned file")
    }
}
impl DerefMut for OwnedFile {
    fn deref_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("live owned file")
    }
}
impl Drop for OwnedFile {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub fn retain_first<T>(result: &mut Result<T, String>, completion: Result<(), String>) {
    if result.is_ok()
        && let Err(error) = completion
    {
        *result = Err(error);
    }
}

pub fn terminate_during<T>(
    context: &Context,
    diagnostic_ms: Option<u64>,
    terminate: impl Fn() -> Result<(), String> + Sync,
    work: impl FnOnce() -> Result<T, String>,
) -> (Result<T, String>, bool) {
    struct RunEnd<'a> {
        done: &'a Mutex<bool>,
        wake: &'a Condvar,
        context: &'a Context,
    }
    impl Drop for RunEnd<'_> {
        fn drop(&mut self) {
            let mut done = self.done.lock().unwrap_or_else(|poison| {
                self.context
                    .record(Err("termination state poisoned".into()));
                poison.into_inner()
            });
            *done = true;
            self.wake.notify_all();
        }
    }
    let done = Mutex::new(false);
    let wake = Condvar::new();
    let requested = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        let helper = if context.bound() || diagnostic_ms.is_some() {
            Some(
                std::thread::Builder::new()
                    .name("beatthis-terminate".into())
                    .spawn_scoped(scope, || {
                        let start = Instant::now();
                        let mut ended = done.lock().map_err(|_| "termination state poisoned")?;
                        loop {
                            if *ended {
                                return Ok(());
                            }
                            if context.cancelled()
                                || diagnostic_ms
                                    .is_some_and(|ms| start.elapsed() >= Duration::from_millis(ms))
                            {
                                terminate()?;
                                requested.store(true, Ordering::Release);
                                return Ok(());
                            }
                            ended = wake
                                .wait_timeout(ended, Duration::from_millis(20))
                                .map_err(|_| "termination wait poisoned")?
                                .0;
                        }
                    })
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let end = RunEnd {
            done: &done,
            wake: &wake,
            context,
        };
        let result = work();
        drop(end);
        if let Some(helper) = helper {
            let joined = helper
                .join()
                .map_err(|_| "termination helper panicked".to_string())
                .and_then(|x| x);
            context.record(joined);
            // A successful work value may own a live Session. Keep it available
            // for consuming/profile completion even if helper cleanup failed.
        }
        result
    });
    (result, requested.load(Ordering::Acquire))
}
