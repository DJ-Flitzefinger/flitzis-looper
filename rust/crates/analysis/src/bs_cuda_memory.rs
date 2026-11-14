//! Optional process-local Windows video memory observations for the CUDA worker.

use std::{
    ffi::c_void,
    ptr::{NonNull, null_mut},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
    thread::{self, JoinHandle},
    time::Duration,
};

use libloading::os::windows::{LOAD_LIBRARY_SEARCH_SYSTEM32, Library};
use serde::Serialize;

const SAMPLE_PERIOD: Duration = Duration::from_millis(50);
const DXGI_MEMORY_SEGMENT_GROUP_LOCAL: u32 = 0;

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

const FACTORY4: Guid = Guid(
    0x1bc6ea02,
    0xef36,
    0x464f,
    [0xbf, 0x0c, 0x21, 0xca, 0x39, 0xe5, 0x16, 0x8a],
);
const ADAPTER3: Guid = Guid(
    0x645967a4,
    0x1392,
    0x4310,
    [0xa7, 0x98, 0x80, 0x53, 0xce, 0x3e, 0x93, 0xfd],
);

#[repr(C)]
struct Luid(u32, i32);

impl From<[u8; 8]> for Luid {
    fn from([a, b, c, d, e, f, g, h]: [u8; 8]) -> Self {
        Self(
            u32::from_le_bytes([a, b, c, d]),
            i32::from_le_bytes([e, f, g, h]),
        )
    }
}

#[repr(C)]
struct IUnknownVtable {
    _query_interface_and_add_ref: [*const c_void; 2],
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

// The inherited slots and signatures are from the Windows SDK dxgi1_4.h.
#[repr(C)]
struct FactoryVtable {
    base: IUnknownVtable,
    _inherited_methods: [*const c_void; 23],
    enum_by_luid:
        unsafe extern "system" fn(*mut c_void, Luid, *const Guid, *mut *mut c_void) -> i32,
}

#[repr(C)]
struct AdapterVtable {
    base: IUnknownVtable,
    _previous_methods: [*const c_void; 11],
    query_memory: unsafe extern "system" fn(*mut c_void, u32, u32, *mut VideoMemoryInfo) -> i32,
}

struct Interface(NonNull<c_void>);

impl Interface {
    fn from_output(status: i32, pointer: *mut c_void) -> Option<Self> {
        if status != 0 {
            return None;
        }
        NonNull::new(pointer).map(Self)
    }

    fn pointer(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// The caller must select the vtable for the IID that was requested.
    unsafe fn vtable<T>(&self) -> &T {
        unsafe { &**self.pointer().cast::<*const T>() }
    }
}

impl Drop for Interface {
    fn drop(&mut self) {
        // Every interface derives from IUnknown, at the start of its vtable.
        unsafe { (self.vtable::<IUnknownVtable>().release)(self.pointer()) };
    }
}

#[derive(Default)]
#[repr(C)]
struct VideoMemoryInfo {
    budget: u64,
    usage: u64,
    _available_for_reservation: u64,
    _current_reservation: u64,
}

#[derive(Serialize)]
pub struct Observation {
    kind: &'static str,
    observed_peak_bytes: u64,
    minimum_budget_bytes: u64,
    first_usage_bytes: u64,
    last_usage_bytes: u64,
    samples: u64,
}

impl Observation {
    fn new(sample: &VideoMemoryInfo) -> Self {
        Self {
            kind: "dxgi_wddm_local_sampled",
            observed_peak_bytes: sample.usage,
            minimum_budget_bytes: sample.budget,
            first_usage_bytes: sample.usage,
            last_usage_bytes: sample.usage,
            samples: 1,
        }
    }

    fn record(&mut self, sample: &VideoMemoryInfo) {
        self.observed_peak_bytes = self.observed_peak_bytes.max(sample.usage);
        self.minimum_budget_bytes = self.minimum_budget_bytes.min(sample.budget);
        self.last_usage_bytes = sample.usage;
        self.samples += 1;
    }
}

/// Samples process-local memory on one selected WDDM node for one worker run.
pub struct Measurement {
    stop: Sender<()>,
    thread: Option<JoinHandle<Option<Observation>>>,
}

impl Measurement {
    /// Capture the baseline before ORT loads. Unavailable telemetry is optional.
    pub fn start(luid: [u8; 8], node_mask: u32) -> Option<Self> {
        if !node_mask.is_power_of_two() || luid == [0; 8] {
            return None;
        }
        let (ready, baseline) = mpsc::sync_channel(0);
        let (stop, stop_request) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("bs-cuda-memory".into())
            .spawn(move || sample(luid.into(), node_mask.trailing_zeros(), ready, stop_request))
            .ok()?;
        if baseline.recv().is_err() {
            let _ = thread.join();
            return None;
        }
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Take a final sample while the ORT session is live and join the reader.
    pub fn finish(mut self) -> Option<Observation> {
        self.join()
    }

    fn join(&mut self) -> Option<Observation> {
        let thread = self.thread.take()?;
        let _ = self.stop.send(());
        thread.join().ok().flatten()
    }
}

impl Drop for Measurement {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

fn sample(luid: Luid, node: u32, ready: SyncSender<()>, stop: Receiver<()>) -> Option<Observation> {
    type CreateFactory = unsafe extern "system" fn(u32, *const Guid, *mut *mut c_void) -> i32;
    // The DLL and both COM interfaces stay on this thread through Release.
    let dxgi =
        unsafe { Library::load_with_flags("dxgi.dll", LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
    let create = unsafe { dxgi.get::<CreateFactory>(b"CreateDXGIFactory2\0") }.ok()?;
    let mut pointer = null_mut();
    let status = unsafe { create(0, &FACTORY4, &mut pointer) };
    let factory = Interface::from_output(status, pointer)?;
    pointer = null_mut();
    let status = unsafe {
        (factory.vtable::<FactoryVtable>().enum_by_luid)(
            factory.pointer(),
            luid,
            &ADAPTER3,
            &mut pointer,
        )
    };
    let adapter = Interface::from_output(status, pointer)?;
    let query = || {
        let mut info = VideoMemoryInfo::default();
        let status = unsafe {
            (adapter.vtable::<AdapterVtable>().query_memory)(
                adapter.pointer(),
                node,
                DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
                &mut info,
            )
        };
        (status == 0).then_some(info)
    };
    let mut observation = Observation::new(&query()?);
    ready.send(()).ok()?;
    while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(SAMPLE_PERIOD) {
        observation.record(&query()?);
    }
    observation.record(&query()?);
    (observation.observed_peak_bytes > 0).then_some(observation)
}

#[cfg(test)]
mod tests {
    use super::{Observation, VideoMemoryInfo};

    #[test]
    fn retains_the_sampled_peak_when_usage_recovers() {
        let sample = |usage, budget| VideoMemoryInfo {
            budget,
            usage,
            ..VideoMemoryInfo::default()
        };
        let mut observation = Observation::new(&sample(0, 400));
        observation.record(&sample(300, 200));
        observation.record(&sample(100, 350));
        assert_eq!(observation.observed_peak_bytes, 300);
        assert_eq!(observation.minimum_budget_bytes, 200);
        assert_eq!(observation.first_usage_bytes, 0);
        assert_eq!(observation.last_usage_bytes, 100);
        assert_eq!(observation.samples, 3);
    }
}
