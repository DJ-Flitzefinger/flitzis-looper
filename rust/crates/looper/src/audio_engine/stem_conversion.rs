//! Thin offline-only bridge. No AudioEngine borrow, control lock or file access.
#[cfg(windows)]
use super::resident_host_memory;
use super::resident_host_memory::HostReservation;
use flitzis_stem_conversion::{self as core, Shape, Source};
use pyo3::exceptions::{PyMemoryError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyTuple, PyType};
use sha2::{Digest, Sha256};

#[cfg(windows)]
const HOST_SCRATCH: usize = 64 * 1024;

/// A Windows job commit ceiling promised on the existing native host ledger.
/// The caller must install the ceiling before child activation and close only
/// after the entire owned tree and its pipe readers have returned.
#[pyclass(immutable_type, module = "flitzis_looper_audio.flitzis_looper_audio")]
pub struct OfflineChildMemoryReservation {
    bytes: usize,
    host: Option<HostReservation>,
}

#[pymethods]
impl OfflineChildMemoryReservation {
    #[new]
    fn new(py: Python<'_>) -> PyResult<Self> {
        #[cfg(windows)]
        {
            let (host, bytes) = py
                .detach(|| HostReservation::reserve_child_ceiling(usize::MAX))
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            Ok(Self {
                bytes,
                host: Some(host),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = py;
            Err(PyRuntimeError::new_err(
                "Windows child commit enforcement is unsupported on this platform",
            ))
        }
    }

    #[getter]
    fn limit_bytes(&self) -> PyResult<usize> {
        if self.host.is_none() {
            return Err(PyValueError::new_err("child memory reservation is closed"));
        }
        Ok(self.bytes)
    }

    fn close(&mut self, py: Python<'_>) {
        let host = self.host.take();
        py.detach(move || drop(host));
    }
}

/// Prospective raw/payload/copy memory on the existing shared host ledger.
/// It is born before Python reads the raw spans and retained through writes.
#[pyclass(immutable_type, module = "flitzis_looper_audio.flitzis_looper_audio")]
pub struct StemConversionReservation {
    bytes: usize,
    _host: Option<HostReservation>,
    consumed: bool,
}

impl StemConversionReservation {
    fn reserve(bytes: usize, limit: usize) -> PyResult<Self> {
        if bytes == 0 || bytes > limit || limit > core::MAX_PCM_BYTES {
            return Err(PyValueError::new_err(
                "invalid transient stem PCM reservation",
            ));
        }
        // The fixed metadata/stack allowance is separate from the PCM cap.
        #[cfg(windows)]
        let host = Some(
            HostReservation::reserve_with(bytes + HOST_SCRATCH, || {
                let mut image = resident_host_memory::host_headroom()?;
                // Existing ledger retains 256 MiB commit / 128 MiB physical.
                // Conversion additionally preserves the unchanged stem entry
                // floors of 2 GiB commit / 512 MiB physical before its delta.
                image.caller_available_commit = image
                    .caller_available_commit
                    .saturating_sub(1792 * 1024 * 1024);
                image.system_available_commit = image
                    .system_available_commit
                    .saturating_sub(1792 * 1024 * 1024);
                image.available_physical =
                    image.available_physical.saturating_sub(384 * 1024 * 1024);
                Ok(image)
            })
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?,
        );
        // The pure core remains usable on other OSes with its hard PCM bound.
        // This is explicitly unsupported host monitoring, never invented headroom.
        #[cfg(not(windows))]
        let host = None;
        Ok(Self {
            bytes,
            _host: host,
            consumed: false,
        })
    }
}

#[pymethods]
impl StemConversionReservation {
    #[new]
    fn new(py: Python<'_>, pcm_bytes: usize, limit: usize) -> PyResult<Self> {
        py.detach(|| Self::reserve(pcm_bytes, limit))
    }

    /// Terminal only: a mutable borrow cannot succeed while convert is detached.
    /// Existing bytes are actual OS-accounted allocations, no prospective copies.
    fn close(&mut self, py: Python<'_>) {
        self.consumed = true;
        let host = self._host.take();
        py.detach(move || drop(host));
    }

    /// Retained immutable Python bytes are safely borrowed across synchronous detach.
    fn convert<'py>(
        &mut self,
        py: Python<'py>,
        sources: [(u32, u16, u64, u64, Bound<'py, PyBytes>); 4],
        target: (u32, u16, u64),
        start: u64,
        frames: usize,
    ) -> PyResult<Bound<'py, PyTuple>> {
        if self.consumed {
            return Err(PyValueError::new_err(
                "stem conversion reservation already consumed",
            ));
        }
        self.consumed = true;
        let inputs = sources
            .each_ref()
            .map(|(rate, channels, frames, first, pcm)| Source {
                shape: Shape {
                    rate: *rate,
                    channels: *channels,
                    frames: *frames,
                },
                first: *first,
                pcm: pcm.as_bytes(),
            });
        let target = Shape {
            rate: target.0,
            channels: target.1,
            frames: target.2,
        };
        let output = py
            .detach(|| {
                #[cfg(test)]
                let checks = std::cell::Cell::new(0usize);
                core::convert(inputs, target, start, frames, self.bytes, || {
                    #[cfg(test)]
                    {
                        checks.set(checks.get() + 1);
                        if checks.get() == 3 {
                            // After one real frame and payload allocation.
                            super::native_owned_work::observe(
                                super::native_owned_work::WorkPoint::StemConversion,
                            );
                        }
                    }
                    false
                })
            })
            .map_err(|error| match error {
                core::Error::Allocation => PyMemoryError::new_err(error.to_string()),
                _ => PyValueError::new_err(error.to_string()),
            })?;
        let payloads = output
            .iter()
            .map(|payload| {
                PyBytes::new_with(py, payload.len(), |destination| {
                    destination.copy_from_slice(payload);
                    Ok(())
                })
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyTuple::new(py, payloads)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn retained_failure_and_closed_token_do_not_keep_a_prospective_host_promise() {
        let baseline = resident_host_memory::pending_bytes().unwrap();
        Python::attach(|py| {
            let token = py
                .get_type::<StemConversionReservation>()
                .call1((1024, 1024))
                .unwrap();
            assert!(resident_host_memory::pending_bytes().unwrap() > baseline);
            let raw = PyBytes::new(py, &[0; 2]);
            let inputs = std::array::from_fn::<_, 4, _>(|_| (48000, 1, 2, 0, raw.clone()));
            let retained_error = token
                .call_method1("convert", (inputs, (48000, 1, 2), 0, 2))
                .unwrap_err();
            token.call_method0("close").unwrap();
            assert_eq!(resident_host_memory::pending_bytes().unwrap(), baseline);
            assert!(retained_error.to_string().contains("span"));
            token.call_method0("close").unwrap();
        });
        assert_eq!(resident_host_memory::pending_bytes().unwrap(), baseline);
    }
}

/// Compiled conversion-source identity, not an OS loaded-image/toolchain witness.
/// Every authored executed numeric/bridge declaration is embedded by the build;
/// current .rs pathname bytes cannot impersonate an older loaded converter.
#[pyfunction]
#[pyo3(signature = (converter=None, memory_budget=None))]
pub fn stem_conversion_identity(
    py: Python<'_>,
    converter: Option<Bound<'_, PyType>>,
    memory_budget: Option<Bound<'_, PyType>>,
) -> PyResult<(String, String)> {
    if converter.is_some_and(|value| !value.is(py.get_type::<StemConversionReservation>())) {
        return Err(PyValueError::new_err(
            "unsupported actual stem converter binding",
        ));
    }
    if memory_budget.is_some_and(|value| !value.is(py.get_type::<OfflineChildMemoryReservation>()))
    {
        return Err(PyValueError::new_err(
            "unsupported actual child memory budget binding",
        ));
    }
    let mut digest = Sha256::new();
    for source in [
        include_bytes!("../../../stem-conversion/src/lib.rs").as_slice(),
        include_bytes!("../../../stem-conversion/Cargo.toml").as_slice(),
        include_bytes!("stem_conversion.rs").as_slice(),
        include_bytes!("resident_host_memory.rs").as_slice(),
        include_bytes!("../../Cargo.toml").as_slice(),
        include_bytes!("../../../../Cargo.lock").as_slice(),
    ] {
        digest.update((source.len() as u64).to_le_bytes());
        digest.update(source);
    }
    digest.update(std::env::consts::OS.as_bytes());
    digest.update(std::env::consts::ARCH.as_bytes());
    digest.update([u8::from(cfg!(debug_assertions))]);
    Ok((core::POLICY.into(), format!("{:x}", digest.finalize())))
}
