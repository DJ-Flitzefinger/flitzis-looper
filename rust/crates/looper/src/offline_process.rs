//! Offline child construction, separate from the audio engine and callback.
#[cfg(not(windows))]
use pyo3::exceptions::PyRuntimeError;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyType;
use sha2::{Digest, Sha256};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::PreparedWindowsProcess;

#[cfg(not(windows))]
#[pyclass(immutable_type, module = "flitzis_looper_audio.flitzis_looper_audio")]
pub struct PreparedWindowsProcess;

#[cfg(not(windows))]
#[pymethods]
impl PreparedWindowsProcess {
    #[new]
    #[pyo3(signature = (*, capture_stderr=false, budget_keeper=None))]
    fn new(capture_stderr: bool, budget_keeper: Option<Py<PyAny>>) -> PyResult<Self> {
        let _ = (capture_stderr, budget_keeper);
        Err(PyRuntimeError::new_err(
            "prepared Windows process construction is unsupported on this platform",
        ))
    }
}

/// Bind the actual constructor type to its compiled implementation, not live .rs paths.
#[pyfunction]
pub fn offline_process_identity(
    py: Python<'_>,
    process_type: Bound<'_, PyType>,
) -> PyResult<(&'static str, String)> {
    if !process_type.is(py.get_type::<PreparedWindowsProcess>()) {
        return Err(PyValueError::new_err(
            "unsupported prepared Windows process binding",
        ));
    }
    let mut digest = Sha256::new();
    for source in [
        include_bytes!("offline_process.rs").as_slice(),
        include_bytes!("offline_process/windows.rs").as_slice(),
        include_bytes!("lib.rs").as_slice(),
        include_bytes!("../Cargo.toml").as_slice(),
        include_bytes!("../../../Cargo.lock").as_slice(),
    ] {
        digest.update((source.len() as u64).to_le_bytes());
        digest.update(source);
    }
    digest.update(std::env::consts::OS.as_bytes());
    digest.update(std::env::consts::ARCH.as_bytes());
    digest.update([u8::from(cfg!(debug_assertions))]);
    Ok((
        "prepared-windows-process-v1",
        format!("{:x}", digest.finalize()),
    ))
}
