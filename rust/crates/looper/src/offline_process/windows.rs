//! Preallocated owner, selective handle inheritance and an explicitly suspended root.
//! This closes the Python result-allocation seam; it is not a whole-process memory proof.
use pyo3::exceptions::{PyMemoryError, PyOSError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{self, PipeReader, PipeWriter, Read};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

type Handle = *mut c_void;
const READ_LIMIT: usize = 16_384;
const INFINITE: u32 = u32::MAX;
const WAIT_TIMEOUT: u32 = 258;
const CREATE_FLAGS: u32 = 0x0800_0000 | 0x0000_0004 | 0x0000_0400 | 0x0008_0000;
const HANDLE_LIST: usize = 0x0002_0002;

#[repr(C)]
#[derive(Default)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

#[repr(C)]
#[derive(Default)]
struct StartupInformation {
    size: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_chars: u32,
    y_chars: u32,
    fill: u32,
    flags: u32,
    show: u16,
    reserved_size: u16,
    reserved_bytes: *mut u8,
    stdin: Handle,
    stdout: Handle,
    stderr: Handle,
}

#[repr(C)]
struct ExtendedStartup {
    startup: StartupInformation,
    attributes: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *mut c_void,
        size: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        application: *const u16,
        command: *mut u16,
        process_security: *mut c_void,
        thread_security: *mut c_void,
        inherit: i32,
        flags: u32,
        environment: *const c_void,
        cwd: *const u16,
        startup: *const StartupInformation,
        information: *mut ProcessInformation,
    ) -> i32;
    fn DuplicateHandle(
        source_process: Handle,
        source: Handle,
        target_process: Handle,
        target: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
    fn TerminateProcess(handle: Handle, code: u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn Sleep(milliseconds: u32);
    fn CompareStringOrdinal(
        first: *const u16,
        first_len: i32,
        second: *const u16,
        second_len: i32,
        ignore_case: i32,
    ) -> i32;
    fn LCMapStringEx(
        locale: *const u16,
        flags: u32,
        source: *const u16,
        source_len: i32,
        destination: *mut u16,
        destination_len: i32,
        version: *mut c_void,
        reserved: *mut c_void,
        sort: isize,
    ) -> i32;
}

fn os_error(phase: &'static str) -> PyErr {
    PyOSError::new_err((
        io::Error::last_os_error().raw_os_error().unwrap_or(0),
        phase,
    ))
}

fn memory_error() -> PyErr {
    PyMemoryError::new_err("offline process buffer allocation failed")
}

fn lock<T>(value: &Mutex<T>) -> PyResult<MutexGuard<'_, T>> {
    value
        .lock()
        .map_err(|_| PyRuntimeError::new_err("prepared process owner lock poisoned"))
}

fn wide(value: &Bound<'_, PyString>) -> PyResult<Vec<u16>> {
    let mut size = 0;
    // The Windows C API preserves unpaired UTF-16 surrogates, unlike a Rust String.
    let address = unsafe { pyo3::ffi::PyUnicode_AsWideCharString(value.as_ptr(), &mut size) };
    if address.is_null() {
        return Err(PyErr::fetch(value.py()));
    }
    let length = usize::try_from(size).map_err(|_| memory_error());
    let result = length.and_then(|length| {
        let mut output = Vec::new();
        output
            .try_reserve_exact(length + 1)
            .map_err(|_| memory_error())?;
        output.extend_from_slice(unsafe {
            std::slice::from_raw_parts(address.cast::<u16>(), length)
        });
        output.push(0);
        Ok(output)
    });
    unsafe { pyo3::ffi::PyMem_Free(address.cast()) };
    result
}

fn path(value: &Bound<'_, PyString>) -> PyResult<Vec<u16>> {
    let output = wide(value)?;
    if output[..output.len() - 1].contains(&0) {
        return Err(PyValueError::new_err(
            "embedded NUL in process path or command",
        ));
    }
    Ok(output)
}

struct EnvironmentEntry {
    key: Vec<u16>,
    value: Vec<u16>,
    upper: Vec<u16>,
}

fn python_string_order(first: &[u16], second: &[u16]) -> std::cmp::Ordering {
    let scalar = |value: Result<char, std::char::DecodeUtf16Error>| {
        value.map_or_else(|error| u32::from(error.unpaired_surrogate()), u32::from)
    };
    std::char::decode_utf16(first.iter().copied())
        .map(scalar)
        .cmp(std::char::decode_utf16(second.iter().copied()).map(scalar))
}

fn uppercase(key: &[u16]) -> PyResult<Vec<u16>> {
    let count = i32::try_from(key.len() - 1).map_err(|_| memory_error())?;
    let locale = [0u16]; // LOCALE_NAME_INVARIANT.
    let required = unsafe {
        LCMapStringEx(
            locale.as_ptr(),
            0x200,
            key.as_ptr(),
            count,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if required == 0 {
        return Err(os_error("environment uppercase extent"));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(required as usize)
        .map_err(|_| memory_error())?;
    output.resize(required as usize, 0);
    if unsafe {
        LCMapStringEx(
            locale.as_ptr(),
            0x200,
            key.as_ptr(),
            count,
            output.as_mut_ptr(),
            required,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(os_error("environment uppercase"));
    }
    Ok(output)
}

fn environment(values: &Bound<'_, PyDict>) -> PyResult<Vec<u16>> {
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(values.len())
        .map_err(|_| memory_error())?;
    for (key, value) in values.iter() {
        let key = path(key.cast::<PyString>()?)?;
        let value = path(value.cast::<PyString>()?)?;
        if key.len() == 1 || key[1..key.len() - 1].contains(&u16::from(b'=')) {
            return Err(PyValueError::new_err("illegal environment variable name"));
        }
        entries.push(EnvironmentEntry {
            upper: uppercase(&key)?,
            key,
            value,
        });
    }
    // Stable insertion sort requires no additional allocation. CPython orders the
    // invariant-uppercase Python strings, then compares original keys ordinally.
    for index in 1..entries.len() {
        let mut current = index;
        while current > 0
            && python_string_order(&entries[current].upper, &entries[current - 1].upper).is_lt()
        {
            entries.swap(current, current - 1);
            current -= 1;
        }
    }
    let size = entries
        .iter()
        .try_fold(1usize, |total, entry| {
            total
                .checked_add(entry.key.len())
                .and_then(|value| value.checked_add(entry.value.len()))
        })
        .ok_or_else(memory_error)?
        .max(2);
    let mut output = Vec::new();
    output.try_reserve_exact(size).map_err(|_| memory_error())?;
    for (index, entry) in entries.iter().enumerate() {
        if let Some(next) = entries.get(index + 1) {
            let same = unsafe {
                CompareStringOrdinal(
                    entry.key.as_ptr(),
                    (entry.key.len() - 1) as i32,
                    next.key.as_ptr(),
                    (next.key.len() - 1) as i32,
                    1,
                )
            };
            if same == 0 {
                return Err(os_error("environment ordinal comparison"));
            }
            if same == 2 {
                continue;
            }
        }
        output.extend_from_slice(&entry.key[..entry.key.len() - 1]);
        output.push(u16::from(b'='));
        output.extend_from_slice(&entry.value);
    }
    output.push(0);
    if output.len() == 1 {
        output.push(0);
    }
    Ok(output)
}

struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
    handles: Vec<usize>,
}

impl Attributes {
    fn new(handles: Vec<usize>) -> PyResult<Self> {
        let mut bytes = 0;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(os_error("process attribute extent"));
        }
        let words = bytes.div_ceil(size_of::<usize>());
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(words)
            .map_err(|_| memory_error())?;
        storage.resize(words, 0);
        let mut result = Self {
            storage,
            initialized: false,
            handles,
        };
        if unsafe { InitializeProcThreadAttributeList(result.address(), 1, 0, &mut bytes) } == 0 {
            // Initialization did not create a list, so no Delete call is permitted.
            return Err(os_error("process attribute initialization"));
        }
        result.initialized = true;
        if unsafe {
            UpdateProcThreadAttribute(
                result.address(),
                0,
                HANDLE_LIST,
                result.handles.as_mut_ptr().cast(),
                size_of_val(result.handles.as_slice()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(os_error("selective process handle inheritance"));
        }
        Ok(result)
    }

    fn address(&mut self) -> *mut c_void {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { DeleteProcThreadAttributeList(self.address()) };
        }
    }
}

#[pyclass(
    frozen,
    immutable_type,
    module = "flitzis_looper_audio.flitzis_looper_audio"
)]
pub struct ReadPipe {
    reader: Mutex<Option<PipeReader>>,
}

#[pymethods]
impl ReadPipe {
    #[getter]
    fn closed(&self, py: Python<'_>) -> PyResult<bool> {
        py.detach(|| Ok(lock(&self.reader)?.is_none()))
    }

    fn read(&self, py: Python<'_>, size: usize) -> PyResult<Py<PyBytes>> {
        if !(1..=READ_LIMIT).contains(&size) {
            return Err(PyValueError::new_err(
                "offline pipe read must be between 1 and 16384 bytes",
            ));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(|_| memory_error())?;
        bytes.resize(size, 0);
        let count = py.detach(|| {
            let mut owner = lock(&self.reader)?;
            let reader = owner
                .as_mut()
                .ok_or_else(|| PyValueError::new_err("read of closed offline pipe"))?;
            reader.read(&mut bytes).map_err(PyErr::from)
        })?;
        Ok(PyBytes::new_with(py, count, |output| {
            output.copy_from_slice(&bytes[..count]);
            Ok(())
        })?
        .unbind())
    }

    fn close(&self, py: Python<'_>) -> PyResult<()> {
        // A blocking reader must be able to reacquire the GIL while close waits.
        py.detach(|| {
            lock(&self.reader)?.take();
            Ok(())
        })
    }
}

struct Stdio {
    input: File,
    output: PipeWriter,
    error: ErrorOutput,
}

enum ErrorOutput {
    Pipe(PipeWriter),
    Null(File),
}
impl AsRawHandle for ErrorOutput {
    fn as_raw_handle(&self) -> Handle {
        match self {
            Self::Pipe(value) => value.as_raw_handle(),
            Self::Null(value) => value.as_raw_handle(),
        }
    }
}

#[derive(Default)]
struct State {
    information: ProcessInformation,
    stdio: Option<Stdio>,
}

// The raw handles are uniquely owned under State's mutex; nobody closes them
// while a Python method retains this owner's reference.
unsafe impl Send for State {}

#[pyclass(
    frozen,
    immutable_type,
    module = "flitzis_looper_audio.flitzis_looper_audio"
)]
pub struct PreparedWindowsProcess {
    state: Mutex<State>,
    output: Py<ReadPipe>,
    error: Option<Py<ReadPipe>>,
    started: AtomicBool,
    process: AtomicUsize,
    pid: AtomicU32,
    // Preserve the original reservation even if Python unwinds its other owner
    // before this native owner. Drop waits the root before releasing this field.
    _budget_keeper: Option<Py<PyAny>>,
}

fn duplicate(handle: Handle) -> PyResult<OwnedHandle> {
    let mut output = std::ptr::null_mut();
    let current = unsafe { GetCurrentProcess() };
    if unsafe { DuplicateHandle(current, handle, current, &mut output, 0, 1, 2) } == 0 {
        return Err(os_error("prepare inherited standard handle"));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(output) })
}

fn terminate_wait(handle: Handle) {
    // Positive kernel wait is required before releasing ownership. In particular,
    // a close request, failed wait or Rust Child destructor is not an END receipt.
    loop {
        if unsafe { WaitForSingleObject(handle, 0) } == 0 {
            return;
        }
        unsafe { TerminateProcess(handle, 1) };
        if unsafe { WaitForSingleObject(handle, INFINITE) } == 0 {
            return;
        }
        unsafe { Sleep(20) };
    }
}

#[pymethods]
impl PreparedWindowsProcess {
    #[new]
    #[pyo3(signature = (*, capture_stderr=false, budget_keeper=None))]
    fn new(
        py: Python<'_>,
        capture_stderr: bool,
        budget_keeper: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let (reader, output) = io::pipe()?;
        let output_reader = Py::new(
            py,
            ReadPipe {
                reader: Mutex::new(Some(reader)),
            },
        )?;
        let (error, error_reader) = if capture_stderr {
            let (reader, writer) = io::pipe()?;
            (
                ErrorOutput::Pipe(writer),
                Some(Py::new(
                    py,
                    ReadPipe {
                        reader: Mutex::new(Some(reader)),
                    },
                )?),
            )
        } else {
            (
                ErrorOutput::Null(OpenOptions::new().write(true).open("NUL")?),
                None,
            )
        };
        Ok(Self {
            state: Mutex::new(State {
                stdio: Some(Stdio {
                    input: File::open("NUL")?,
                    output,
                    error,
                }),
                ..State::default()
            }),
            output: output_reader,
            error: error_reader,
            started: AtomicBool::new(false),
            process: AtomicUsize::new(0),
            pid: AtomicU32::new(0),
            _budget_keeper: budget_keeper,
        })
    }

    #[getter]
    fn stdout(&self, py: Python<'_>) -> Py<ReadPipe> {
        self.output.clone_ref(py)
    }
    #[getter]
    fn stderr(&self, py: Python<'_>) -> Option<Py<ReadPipe>> {
        self.error.as_ref().map(|value| value.clone_ref(py))
    }
    #[getter]
    fn pid(&self) -> u32 {
        self.pid.load(Ordering::Acquire)
    }
    #[getter]
    fn returncode(&self) -> PyResult<Option<u32>> {
        self.poll()
    }

    fn start(
        &self,
        py: Python<'_>,
        command: &Bound<'_, PyString>,
        cwd: &Bound<'_, PyString>,
        env: &Bound<'_, PyDict>,
    ) -> PyResult<()> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(PyValueError::new_err(
                "prepared process can only be started once",
            ));
        }
        let mut command = path(command)?;
        let cwd = path(cwd)?;
        let environment = environment(env)?;
        let inherited = {
            let state = lock(&self.state)?;
            let stdio = state.stdio.as_ref().ok_or_else(|| {
                PyValueError::new_err("prepared process standard handles are closed")
            })?;
            [
                duplicate(stdio.input.as_raw_handle())?,
                duplicate(stdio.output.as_raw_handle())?,
                duplicate(stdio.error.as_raw_handle())?,
            ]
        };
        let mut handles = Vec::new();
        handles.try_reserve_exact(3).map_err(|_| memory_error())?;
        handles.extend(
            inherited
                .iter()
                .map(|handle| handle.as_raw_handle() as usize),
        );
        // Both buffers remain heap-stable when their Rust owners move into the
        // detached closure. Attribute values must live until the list is deleted.
        let mut attributes = Attributes::new(handles)?;
        py.detach(move || {
            let mut state = lock(&self.state)?;
            if state.stdio.is_none() {
                return Err(PyValueError::new_err(
                    "prepared process standard handles are closed",
                ));
            }
            let startup = ExtendedStartup {
                startup: StartupInformation {
                    size: size_of::<ExtendedStartup>() as u32,
                    flags: 0x100,
                    stdin: inherited[0].as_raw_handle(),
                    stdout: inherited[1].as_raw_handle(),
                    stderr: inherited[2].as_raw_handle(),
                    ..StartupInformation::default()
                },
                attributes: attributes.address(),
            };
            // The destination belongs to this owner BEFORE the OS call; no
            // Python result allocation or GIL reattachment precedes publication.
            if unsafe {
                CreateProcessW(
                    std::ptr::null(),
                    command.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    1,
                    CREATE_FLAGS,
                    environment.as_ptr().cast(),
                    cwd.as_ptr(),
                    std::ptr::from_ref(&startup).cast(),
                    &mut state.information,
                )
            } == 0
            {
                let error = os_error("create suspended offline process");
                state.information = ProcessInformation::default();
                return Err(error);
            }
            self.process
                .store(state.information.process as usize, Ordering::Release);
            self.pid
                .store(state.information.process_id, Ordering::Release);
            drop(attributes);
            drop(inherited);
            state.stdio.take();
            Ok(())
        })
    }

    fn poll(&self) -> PyResult<Option<u32>> {
        let handle = self.process.load(Ordering::Acquire) as Handle;
        if handle.is_null() {
            return Ok(None);
        }
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_TIMEOUT => Ok(None),
            0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
                    return Err(os_error("offline process exit code"));
                }
                Ok(Some(code))
            }
            _ => Err(os_error("offline process status")),
        }
    }

    #[pyo3(signature = (timeout=None))]
    fn wait(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<u32>> {
        let handle = self.process.load(Ordering::Acquire);
        if handle == 0 {
            return Err(PyValueError::new_err("prepared process has not started"));
        }
        if timeout.is_some_and(|value| !value.is_finite()) {
            return Err(PyValueError::new_err("invalid process timeout"));
        }
        let milliseconds = timeout.map_or(INFINITE, |value| {
            (value.max(0.0) * 1000.0)
                .ceil()
                .min(f64::from(INFINITE - 1)) as u32
        });
        let result = py.detach(|| unsafe { WaitForSingleObject(handle as Handle, milliseconds) });
        match result {
            0 => self.poll(),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(os_error("offline process wait")),
        }
    }

    fn kill(&self) -> PyResult<()> {
        let handle = self.process.load(Ordering::Acquire) as Handle;
        if !handle.is_null()
            && self.poll()?.is_none()
            && unsafe { TerminateProcess(handle, 1) } == 0
        {
            let error = io::Error::last_os_error();
            // Job termination can race the direct-root fallback. Match CPython:
            // access denied is harmless only if a fresh exit-code query confirms
            // termination has begun. It does not certify kernel/tree/reader END.
            let mut code = 259;
            if error.raw_os_error() != Some(5)
                || unsafe { GetExitCodeProcess(handle, &mut code) } == 0
                || code == 259
            {
                return Err(PyOSError::new_err((
                    error.raw_os_error().unwrap_or(0),
                    "terminate offline root",
                )));
            }
        }
        Ok(())
    }

    fn abort_startup(&self, py: Python<'_>) -> PyResult<()> {
        py.detach(|| {
            let mut state = lock(&self.state)?;
            // Serialize against detached creation before observing its handle.
            // Closing an unstarted owner also prevents a later start.
            self.started.store(true, Ordering::Release);
            if !state.information.process.is_null() {
                terminate_wait(state.information.process);
            }
            state.stdio.take();
            Ok::<(), PyErr>(())
        })?;
        self.output.borrow(py).close(py)?;
        if let Some(error) = &self.error {
            error.borrow(py).close(py)?;
        }
        Ok(())
    }
}

impl Drop for PreparedWindowsProcess {
    fn drop(&mut self) {
        let handle = self.process.load(Ordering::Acquire);
        if handle != 0 {
            // Primary cleanup is explicit abort_startup/supervisor retirement.
            // This allocation-free fallback also requires a real root wait and
            // releases the GIL when invoked by a Python-owned object teardown.
            if unsafe { WaitForSingleObject(handle as Handle, 0) } != 0
                && Python::try_attach(|py| py.detach(|| terminate_wait(handle as Handle))).is_none()
            {
                // Interpreter finalization cannot admit a fresh attachment.
                // There are no further Python operations on this fallback path.
                terminate_wait(handle as Handle);
            }
        }
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        for handle in [state.information.thread, state.information.process] {
            if !handle.is_null() {
                unsafe { CloseHandle(handle) };
            }
        }
    }
}
