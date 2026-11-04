//! Windows ASIO control owner. The thread owns every callback dependency before
//! driver activation and retains uncertain resources if external shutdown fails.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::audio_output::{OutputBackend, OutputEndpoint, OutputOptions};
use super::audio_stream::{AsioRenderContext, asio_invalidate, asio_render};
use super::buffer_retirement::AudioBufferRetirementWorker;

#[cfg(all(test, feature = "asio-contract-tests"))]
#[path = "asio_test_driver.rs"]
pub(super) mod test_driver;

#[repr(C)]
struct NativeSession {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(super) struct DriverInfo {
    pub sample_rate: f64,
    pub output_channels: i32,
    pub min_frames: i32,
    pub max_frames: i32,
    pub preferred_frames: i32,
    pub granularity: i32,
    pub prepared_frames: i32,
    pub input_latency_frames: i32,
    pub output_latency_frames: i32,
    pub output_ready: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct DriverChannel {
    pub index: i32,
    pub sample_type: i32,
    pub group: i32,
    pub name: [c_char; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DriverTime {
    pub system_ns: u64,
    pub sample_position: u64,
    pub sample_rate: f64,
    pub flags: u32,
    pub direct_process: i32,
    pub latency_revision: u32,
    pub notifications: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(super) struct DriverStatus {
    pub notifications: u32,
    pub callback_error: i32,
    pub in_flight: u32,
    pub latency_revision: u32,
    pub last_notified_rate: f64,
    pub rate_notice_count: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(super) struct RateTransaction {
    original_rate: f64,
    target_rate: f64,
    observed_rate: f64,
    restore_before_rate: f64,
    restore_after_rate: f64,
    set_status: i32,
    query_status: i32,
    restore_set_status: i32,
    restore_query_status: i32,
    disposition: i32,
    flags: u32,
}

type Render =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, i32, i32, i32, *const DriverTime);
type Enumerate = unsafe extern "C" fn(*mut c_void, *const u16, *const u16);
type Invalidate = unsafe extern "C" fn(*mut c_void, u32, u32);

const _: () = {
    assert!(std::mem::size_of::<DriverInfo>() == 48);
    assert!(std::mem::offset_of!(DriverInfo, output_ready) == 40);
    assert!(std::mem::size_of::<DriverTime>() == 40);
    assert!(std::mem::offset_of!(DriverTime, flags) == 24);
    assert!(std::mem::offset_of!(DriverTime, latency_revision) == 32);
    assert!(std::mem::size_of::<DriverChannel>() == 44);
    assert!(std::mem::offset_of!(DriverChannel, name) == 12);
    assert!(std::mem::size_of::<DriverStatus>() == 32);
    assert!(std::mem::offset_of!(DriverStatus, last_notified_rate) == 16);
    assert!(std::mem::size_of::<RateTransaction>() == 64);
};

unsafe extern "C" {
    fn fl_asio_enumerate(context: *mut c_void, emit: Enumerate) -> i32;
    fn fl_asio_new(session: *mut *mut NativeSession) -> i32;
    fn fl_asio_open(session: *mut NativeSession, clsid: *const u16) -> i32;
    fn fl_asio_info(session: *mut NativeSession, info: *mut DriverInfo) -> i32;
    fn fl_asio_channel(session: *mut NativeSession, index: i32, info: *mut DriverChannel) -> i32;
    fn fl_asio_can_rate(session: *mut NativeSession, rate: f64) -> i32;
    fn fl_asio_set_rate(session: *mut NativeSession, rate: f64) -> i32;
    fn fl_asio_commit_rate(session: *mut NativeSession) -> i32;
    fn fl_asio_abort_candidate_rate(session: *mut NativeSession) -> i32;
    fn fl_asio_rate_transaction(session: *mut NativeSession, receipt: *mut RateTransaction) -> i32;
    fn fl_asio_prepare(
        session: *mut NativeSession,
        frames: i32,
        left: i32,
        right: i32,
        context: *mut c_void,
        render: Render,
        invalidate: Invalidate,
    ) -> i32;
    fn fl_asio_start(session: *mut NativeSession) -> i32;
    fn fl_asio_close(session: *mut NativeSession) -> i32;
    fn fl_asio_delete(session: *mut NativeSession) -> i32;
    fn fl_asio_status(session: *mut NativeSession, status: *mut DriverStatus);
    fn fl_asio_pump_messages();
}

fn checked(operation: &str, code: i32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "ASIO {operation} failed (status {code}, {code:#010x})"
        ))
    }
}

unsafe fn registry_string(pointer: *const u16, max: usize) -> String {
    let length = (0..max)
        .take_while(|offset| unsafe { *pointer.add(*offset) != 0 })
        .count();
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
}

unsafe extern "C" fn collect_endpoint(context: *mut c_void, id: *const u16, name: *const u16) {
    // The bridge supplies bounded, terminated local registry strings. Enumeration
    // is synchronous control work and never loads or initializes a driver.
    let endpoints = unsafe { &mut *context.cast::<Vec<OutputEndpoint>>() };
    let id = unsafe { registry_string(id, 40) };
    if !endpoints.iter().any(|endpoint| endpoint.id == id) {
        endpoints.push(OutputEndpoint {
            id,
            name: unsafe { registry_string(name, 256) },
        });
    }
}

pub(super) fn inventory() -> OutputBackend {
    let mut backend = OutputBackend {
        id: "asio".into(),
        name: "ASIO".into(),
        devices: Vec::new(),
        discovery_error: None,
    };
    let result =
        unsafe { fl_asio_enumerate(ptr::from_mut(&mut backend.devices).cast(), collect_endpoint) };
    backend.discovery_error = checked("registry enumeration", result).err();
    backend
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub(super) struct OwnerObservation {
    pub driver: DriverInfo,
    pub initialized_sample_rate_hz: f64,
    pub last_control_query_rate_hz: Option<f64>,
    pub info_query_error: Option<String>,
    pub status: DriverStatus,
    pub backend_released: bool,
    pub retirement_verified: bool,
    pub owner_ended: bool,
    pub owner_join_verified: bool,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub channels: Vec<ChannelDescription>,
    pub common_rate_probes: Vec<RateProbe>,
    pub rate_transaction: RateTransaction,
    pub failure: Option<String>,
    #[cfg(all(test, feature = "asio-contract-tests"))]
    pub test_tick: Option<test_driver::TickObservation>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct ChannelDescription {
    pub index: i32,
    pub name: String,
    pub group: i32,
    pub native_sample_type: i32,
    pub pcm_supported: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct RateProbe {
    pub sample_rate_hz: u32,
    pub driver_status: i32,
    pub supported: bool,
}

pub(super) struct Installation {
    pub context: Box<AsioRenderContext>,
    pub worker: AudioBufferRetirementWorker,
    pub frames: i32,
    pub left: i32,
    pub right: i32,
}

enum Operation {
    Open(OutputOptions),
    Install(Installation),
    Start,
    Commit,
    AbortCandidate,
    Close,
    Shutdown,
    #[cfg(all(test, feature = "asio-contract-tests"))]
    Tick,
}
struct Command {
    operation: Operation,
    reply: mpsc::SyncSender<Result<DriverInfo, String>>,
}

/// Stored in Engine's pending slot before Open. Installed stream handles retain
/// the same owner; errors never transfer resources into a message-only receipt.
pub(super) struct Control {
    commands: mpsc::SyncSender<Command>,
    thread: Option<JoinHandle<()>>,
    owner_join_failed: bool,
    observation: Arc<Mutex<OwnerObservation>>,
    pub failed: Arc<AtomicBool>,
    /// One atomic pair: revision in upper bits, nonnegative latency frames below.
    pub latency: Arc<AtomicU64>,
}

impl Control {
    pub fn spawn() -> Result<Self, String> {
        Self::spawn_configured(
            #[cfg(all(test, feature = "asio-contract-tests"))]
            None,
        )
    }

    #[cfg(all(test, feature = "asio-contract-tests"))]
    pub(super) fn spawn_injected(factory: Arc<test_driver::Factory>) -> Result<Self, String> {
        Self::spawn_configured(Some(factory))
    }

    fn spawn_configured(
        #[cfg(all(test, feature = "asio-contract-tests"))] factory: Option<
            Arc<test_driver::Factory>,
        >,
    ) -> Result<Self, String> {
        let (commands, receiver) = mpsc::sync_channel(1);
        let observation = Arc::new(Mutex::new(OwnerObservation::default()));
        let failed = Arc::new(AtomicBool::new(false));
        let latency = Arc::new(AtomicU64::new(u64::MAX));
        let (seen, failure, delay) = (observation.clone(), failed.clone(), latency.clone());
        let thread = thread::Builder::new()
            .name("asio-control".into())
            .spawn(move || {
                owner_loop(
                    receiver,
                    seen,
                    failure,
                    delay,
                    #[cfg(all(test, feature = "asio-contract-tests"))]
                    factory,
                );
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            commands,
            thread: Some(thread),
            owner_join_failed: false,
            observation,
            failed,
            latency,
        })
    }

    fn request(&mut self, operation: Operation) -> Result<DriverInfo, String> {
        let (reply, response) = mpsc::sync_channel(1);
        self.commands
            .send(Command { operation, reply })
            .map_err(|_| "ASIO control owner ended".to_owned())?;
        response
            .recv()
            .map_err(|_| "ASIO control owner ended without a receipt".to_owned())?
    }

    pub fn open(&mut self, options: OutputOptions) -> Result<DriverInfo, String> {
        self.request(Operation::Open(options))
    }
    pub fn install(&mut self, installation: Installation) -> Result<DriverInfo, String> {
        self.request(Operation::Install(installation))
    }
    pub fn start(&mut self) -> Result<(), String> {
        self.request(Operation::Start).map(|_| ())
    }
    pub fn commit(&mut self) -> Result<(), String> {
        self.request(Operation::Commit).map(|_| ())
    }
    pub fn abort_candidate(&mut self) -> Result<(), String> {
        self.request(Operation::AbortCandidate).map(|_| ())
    }

    pub fn try_close(&mut self) -> std::io::Result<()> {
        if self.owner_join_failed {
            return Err(std::io::Error::other(
                "ASIO control owner join previously failed",
            ));
        }
        if self.thread.is_none() {
            let observed = self.observation();
            return if observed.retirement_verified && observed.owner_join_verified {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "ASIO owner ended without verified retirement",
                ))
            };
        }
        self.request(Operation::Close)
            .map_err(std::io::Error::other)?;
        self.request(Operation::Shutdown)
            .map_err(std::io::Error::other)?;
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| {
                self.owner_join_failed = true;
                std::io::Error::other("ASIO control owner panicked")
            })?;
            self.observation
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .owner_join_verified = true;
        }
        Ok(())
    }

    pub fn observation(&self) -> OwnerObservation {
        self.observation
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn observation_handle(&self) -> Arc<Mutex<OwnerObservation>> {
        self.observation.clone()
    }

    #[cfg(all(test, feature = "asio-contract-tests"))]
    pub(super) fn tick(&mut self) -> Result<test_driver::TickObservation, String> {
        self.request(Operation::Tick)?;
        self.observation()
            .test_tick
            .ok_or_else(|| "private Tick ended without original callback observation".into())
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        let (reply, response) = mpsc::sync_channel(1);
        if self
            .commands
            .try_send(Command {
                operation: Operation::Shutdown,
                reply,
            })
            .is_ok()
        {
            // Normal explicit close has already finished. A stuck external driver
            // cannot make a finite destructor prove END: the original thread keeps
            // its complete owner after this bounded wait and receiver disconnect.
            let _ = response.recv_timeout(Duration::from_millis(100));
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

struct Owner {
    raw: *mut NativeSession,
    context: Option<Box<AsioRenderContext>>,
    worker: Option<AudioBufferRetirementWorker>,
    retirement_failed: bool,
    backend_released: bool,
    info: DriverInfo,
    device_id: Option<String>,
    device_name: Option<String>,
    channels: Vec<ChannelDescription>,
    rates: Vec<RateProbe>,
    rate_transaction: RateTransaction,
    last_status: DriverStatus,
    #[cfg(all(test, feature = "asio-contract-tests"))]
    test_driver: Option<Arc<test_driver::Factory>>,
    #[cfg(all(test, feature = "asio-contract-tests"))]
    test_tick: Option<test_driver::TickObservation>,
}

impl Owner {
    fn new() -> Self {
        Self {
            raw: ptr::null_mut(),
            context: None,
            worker: None,
            retirement_failed: false,
            backend_released: false,
            info: DriverInfo::default(),
            device_id: None,
            device_name: None,
            channels: Vec::new(),
            rates: Vec::new(),
            rate_transaction: RateTransaction::default(),
            last_status: DriverStatus::default(),
            #[cfg(all(test, feature = "asio-contract-tests"))]
            test_driver: None,
            #[cfg(all(test, feature = "asio-contract-tests"))]
            test_tick: None,
        }
    }

    fn info(&mut self) -> Result<DriverInfo, String> {
        let mut info = DriverInfo::default();
        checked("get current format/latencies", unsafe {
            fl_asio_info(self.raw, &mut info)
        })?;
        self.info = info;
        Ok(self.info)
    }

    fn acquire(&mut self, options: &OutputOptions) -> Result<(), String> {
        #[cfg(all(test, feature = "asio-contract-tests"))]
        if let Some(factory) = self.test_driver.as_ref() {
            if options
                .device_id
                .as_deref()
                .is_some_and(|id| id != "private-asio-fixture")
            {
                return Err("private driver refuses every real device id".into());
            }
            self.device_id = Some("private-asio-fixture".into());
            self.device_name = Some("Private hardware-free ASIO fixture".into());
            return factory.acquire(&mut self.raw);
        }
        // Adopt even a partially acquired session BEFORE propagating its status.
        let result = unsafe { fl_asio_new(&mut self.raw) };
        checked("create session", result)?;
        let endpoints = inventory();
        let endpoint = match options.device_id.as_ref() {
            Some(id) => endpoints.devices.iter().find(|endpoint| &endpoint.id == id),
            None => endpoints.devices.first(),
        }
        .ok_or_else(|| "Selected ASIO driver is unavailable".to_owned())?;
        self.device_id = Some(endpoint.id.clone());
        self.device_name = Some(endpoint.name.clone());
        let id: Vec<_> = endpoint.id.encode_utf16().chain(Some(0)).collect();
        checked("open selected driver", unsafe {
            fl_asio_open(self.raw, id.as_ptr())
        })?;
        Ok(())
    }

    fn open(&mut self, options: OutputOptions) -> Result<DriverInfo, String> {
        if !self.raw.is_null() {
            return Err("ASIO owner is already open".into());
        }
        self.acquire(&options)?;
        let info = self.info()?;
        if let Some(rate) = options.sample_rate_hz
            && info.sample_rate != f64::from(rate)
        {
            checked("validate rate", unsafe {
                fl_asio_can_rate(self.raw, f64::from(rate))
            })?;
            checked("set rate", unsafe {
                fl_asio_set_rate(self.raw, f64::from(rate))
            })?;
        }
        let info = self.info()?; // Rate changes can alter legal buffer sizes.
        if options
            .sample_rate_hz
            .is_some_and(|rate| info.sample_rate != f64::from(rate))
            || info.sample_rate.fract() != 0.0
            || info.sample_rate > f64::from(u32::MAX)
        {
            return Err("ASIO driver did not install the requested integral rate".into());
        }
        options.physical_pair(info.output_channels as usize)?;
        if info.output_channels > 4096 {
            return Err("ASIO driver exceeds the supported physical-channel inventory".into());
        }
        for index in 0..info.output_channels {
            let mut channel = DriverChannel {
                index,
                sample_type: 0,
                group: 0,
                name: [0; 32],
            };
            checked("query physical output", unsafe {
                fl_asio_channel(self.raw, index, &mut channel)
            })?;
            self.channels.push(ChannelDescription {
                index,
                group: channel.group,
                native_sample_type: channel.sample_type,
                name: unsafe { std::ffi::CStr::from_ptr(channel.name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
                pcm_supported: super::asio_sample_format::SampleLayout::from_asio(
                    channel.sample_type,
                )
                .is_some(),
            });
        }
        // ASIO does not enumerate all rates. Keep these explicit probes separate
        // from an exhaustive capability list; custom rates validate again at Apply.
        for rate in [
            44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
        ] {
            let status = unsafe { fl_asio_can_rate(self.raw, f64::from(rate)) };
            self.rates.push(RateProbe {
                sample_rate_hz: rate,
                driver_status: status,
                supported: status == 0,
            });
        }
        Ok(info)
    }

    fn install(&mut self, installation: Installation) -> Result<DriverInfo, String> {
        if self.raw.is_null() || self.context.is_some() || self.worker.is_some() {
            return Err("ASIO callback owner is unavailable for installation".into());
        }
        let Installation {
            context,
            worker,
            frames,
            left,
            right,
        } = installation;
        self.context = Some(context);
        self.worker = Some(worker);
        let context = self.context.as_ref().unwrap().callback_pointer();
        checked("create selected output buffers", unsafe {
            fl_asio_prepare(
                self.raw,
                frames,
                left,
                right,
                context,
                asio_render,
                asio_invalidate,
            )
        })?;
        self.info()
    }

    fn close(&mut self) -> Result<DriverInfo, String> {
        if !self.raw.is_null() {
            checked("stop/detach/dispose/release", unsafe {
                fl_asio_close(self.raw)
            })?;
            self.backend_released = true;
            checked("read independent rate restoration receipt", unsafe {
                fl_asio_rate_transaction(self.raw, &mut self.rate_transaction)
            })?;
            self.last_status = self.status();
        }
        // No device callback can reach context now. Its complete state is handed
        // to the original retirement worker, preserving off-RT destruction.
        self.context = None;
        if let Some(worker) = self.worker.as_mut()
            && let Err(error) = worker.join_checked()
        {
            self.retirement_failed = true;
            return Err(error.to_string());
        }
        self.worker = None;
        if self.retirement_failed {
            return Err("ASIO callback retirement verification failed".into());
        }
        if !self.raw.is_null() {
            checked("delete released session", unsafe {
                fl_asio_delete(self.raw)
            })?;
            self.raw = ptr::null_mut();
        }
        Ok(self.info)
    }

    fn status(&self) -> DriverStatus {
        let mut status = self.last_status;
        if !self.raw.is_null() {
            unsafe { fl_asio_status(self.raw, &mut status) };
        }
        status
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        // Also protects an unexpected control-thread unwind. Never destroy live
        // callback dependencies after a failed external close. This is the SAME
        // original owner, not a new observer, orphan registry or cleanup thread.
        if self.close().is_err() && !self.raw.is_null() {
            loop {
                thread::park();
            }
        }
    }
}

fn owner_loop(
    receiver: mpsc::Receiver<Command>,
    observation: Arc<Mutex<OwnerObservation>>,
    failed: Arc<AtomicBool>,
    latency: Arc<AtomicU64>,
    #[cfg(all(test, feature = "asio-contract-tests"))] factory: Option<Arc<test_driver::Factory>>,
) {
    let mut owner = Owner::new();
    #[cfg(all(test, feature = "asio-contract-tests"))]
    {
        owner.test_driver = factory;
    }
    let mut latency_revision = None;
    let mut rate_notice_count = None;
    loop {
        unsafe { fl_asio_pump_messages() };
        let command = match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if owner.close().is_err() && !owner.raw.is_null() {
                    // Retain all uncertain resources. No caller remains to accept
                    // another receipt or authorize a replacement session.
                    loop {
                        thread::park();
                    }
                }
                break;
            }
        };
        if let Some(Command { operation, reply }) = command {
            let shutdown = matches!(operation, Operation::Shutdown);
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match operation {
                    Operation::Open(options) => owner.open(options),
                    Operation::Install(installation) => owner.install(installation),
                    Operation::Start => {
                        if failed.load(Ordering::Acquire) {
                            Err("ASIO preparation was faulted before Start".into())
                        } else {
                            checked("start", unsafe { fl_asio_start(owner.raw) })?;
                            let status = owner.status();
                            if failed.load(Ordering::Acquire)
                                || status.callback_error != 0
                                || status.notifications & (1 | 2 | 4 | 32) != 0
                            {
                                Err(format!("ASIO callback fault during Start: {status:?}"))
                            } else {
                                Ok(owner.info)
                            }
                        }
                    }
                    Operation::Commit => {
                        let was_committed = owner.rate_transaction.flags & 4 != 0;
                        checked("commit installed rate transaction", unsafe {
                            fl_asio_commit_rate(owner.raw)
                        })?;
                        let status = owner.status();
                        if failed.load(Ordering::Acquire)
                            || status.callback_error != 0
                            || status.notifications & (1 | 2 | 4 | 32) != 0
                        {
                            if !was_committed {
                                checked("re-arm failed first commit rate restoration", unsafe {
                                    fl_asio_abort_candidate_rate(owner.raw)
                                })?;
                            }
                            return Err("ASIO callback fault during configuration commit".into());
                        }
                        Ok(owner.info)
                    }
                    Operation::AbortCandidate => {
                        checked("abort original unfinished candidate rate", unsafe {
                            fl_asio_abort_candidate_rate(owner.raw)
                        })?;
                        Ok(owner.info)
                    }
                    Operation::Close | Operation::Shutdown => owner.close(),
                    #[cfg(all(test, feature = "asio-contract-tests"))]
                    Operation::Tick => {
                        if failed.load(Ordering::Acquire) {
                            return Err("failed original owner cannot Tick".into());
                        }
                        let factory = owner
                            .test_driver
                            .as_ref()
                            .ok_or("Tick requires the original injected owner")?;
                        let context = owner
                            .context
                            .as_ref()
                            .ok_or("Tick requires the installed original callback")?;
                        let scope = super::native_render_observation::Scope::begin();
                        let callback = factory.tick(owner.raw, context.test_clock_ns())?;
                        let native = scope.finish();
                        owner.test_tick = Some(context.test_observation(callback, native));
                        Ok(owner.info)
                    }
                }))
                .unwrap_or_else(|_| Err("ASIO control owner panicked; custody retained".into()));
            if let Err(error) = result.as_ref() {
                failed.store(true, Ordering::Release);
                if let Some(context) = owner.context.as_ref() {
                    context.invalidate_clock();
                }
                observation
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .failure = Some(error.clone());
            }
            let closed = owner.raw.is_null();
            if !closed {
                // A query of cached owner-only scalars, also valid after close.
                let _ = unsafe { fl_asio_rate_transaction(owner.raw, &mut owner.rate_transaction) };
            }
            {
                let mut seen = observation
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                seen.driver = owner.info;
                if seen.initialized_sample_rate_hz == 0.0 && owner.info.prepared_frames > 0 {
                    seen.initialized_sample_rate_hz = owner.info.sample_rate;
                }
                seen.device_id.clone_from(&owner.device_id);
                seen.device_name.clone_from(&owner.device_name);
                seen.channels.clone_from(&owner.channels);
                seen.common_rate_probes.clone_from(&owner.rates);
                seen.rate_transaction = owner.rate_transaction;
                seen.backend_released = owner.backend_released;
                seen.retirement_verified =
                    closed && owner.worker.is_none() && !owner.retirement_failed;
                #[cfg(all(test, feature = "asio-contract-tests"))]
                {
                    seen.test_tick = owner.test_tick;
                }
            }
            let _ = reply.send(result);
            if shutdown && closed {
                break;
            }
        }
        let status = owner.status();
        if status.callback_error != 0 || status.notifications & (1 | 2 | 4 | 32) != 0 {
            failed.store(true, Ordering::Release);
        }
        if owner.info.prepared_frames > 0
            && !owner.raw.is_null()
            && !owner.backend_released
            && (latency_revision != Some(status.latency_revision)
                || rate_notice_count
                    != Some((status.rate_notice_count, status.notifications & (2 | 32))))
        {
            rate_notice_count = Some((status.rate_notice_count, status.notifications & (2 | 32)));
            match owner.info() {
                Ok(info) => {
                    let after = owner.status();
                    if let Some(packed) = super::asio_clock::latency_publication(
                        status.latency_revision,
                        after.latency_revision,
                        info.output_latency_frames,
                    ) {
                        latency.store(packed, Ordering::Release);
                        latency_revision = Some(after.latency_revision);
                    }
                    let mut seen = observation
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    seen.driver = info;
                    seen.last_control_query_rate_hz = Some(info.sample_rate);
                    seen.info_query_error = None;
                }
                Err(error) => {
                    latency_revision = Some(status.latency_revision);
                    let mut seen = observation
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    seen.last_control_query_rate_hz = None;
                    seen.info_query_error = Some(error);
                    failed.store(true, Ordering::Release);
                    if let Some(context) = owner.context.as_ref() {
                        context.invalidate_clock();
                    }
                }
            }
        }
        observation
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status = status;
    }
    observation
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .owner_ended = true;
}

#[cfg(all(test, feature = "asio-contract-tests"))]
mod contract_tests {
    unsafe extern "C" {
        fn fl_asio_contract_tests(completed: *mut u32, failed_line: *mut u32) -> i32;
    }

    #[test]
    fn actual_sdk_bridge_obeys_device_free_driver_contracts() {
        let mut completed = 0;
        let mut failed_line = 0;
        let status = unsafe { fl_asio_contract_tests(&mut completed, &mut failed_line) };
        assert_eq!(
            status,
            0,
            "bridge case {} failed at native fixture line {failed_line}",
            completed + 1
        );
        assert_eq!(completed, 19);
    }
}
