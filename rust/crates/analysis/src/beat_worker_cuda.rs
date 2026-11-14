use flitzis_looper_analysis::{beat_inference::ModelStartupStage, trial_lifetime::Context};
use libloading::os::windows::Library;
use serde_json::{Value, json};
#[cfg(feature = "native-beat-this-worker")]
use std::path::Path;
use std::{
    ffi::{CStr, c_char, c_void},
    io,
};

#[cfg(feature = "native-beat-this-worker")]
use super::invocation::Invocation;

pub const ORT_MANIFEST_SHA: &str =
    "7ebc26a630b9fb721539e7821b918cf9f4a9aa94b856dcc153b81a7b140b240d";
pub const ORT_PACKAGE_SHA: &str =
    "d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b";
const EP_SHA: &str = "110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494";
const CUDART_SHA: &str = "c2c9a9c22a9bcba90e261825968836787b331038047a26770cffb7a583c28344";

#[cfg(feature = "native-beat-this-worker")]
pub fn provider() -> flitzis_looper_analysis::beat_inference::Provider {
    flitzis_looper_analysis::beat_inference::Provider::Cuda {
        memory_limit: usize::MAX,
    }
}

pub struct Device {
    name: String,
    driver_api_version: i32,
    #[cfg(feature = "native-bs-roformer-worker")]
    uuid: Option<String>,
    #[cfg(feature = "native-bs-roformer-worker")]
    luid: Option<([u8; 8], u32)>,
}

#[derive(serde::Serialize)]
pub struct StartupError {
    stage: &'static str,
    code: Option<u32>,
}

const SYSTEM32: u32 = 0x800;
const ERROR_MOD_NOT_FOUND: i32 = 126;
const ERROR_PROC_NOT_FOUND: i32 = 127;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

pub fn device(context: &Context, failure: &mut Option<StartupError>) -> Result<Device, String> {
    context.checkpoint("cuda.device")?;
    // The driver comes from Windows/NVIDIA. Retain the exact raw status from
    // System32 lookup, then transfer the handle to the existing library owner.
    let name: Vec<u16> = "nvcuda.dll".encode_utf16().chain(Some(0)).collect();
    let module = unsafe { LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), SYSTEM32) };
    if module.is_null() {
        let error = io::Error::last_os_error();
        if let Some(code @ (ERROR_MOD_NOT_FOUND | ERROR_PROC_NOT_FOUND)) = error.raw_os_error() {
            *failure = Some(StartupError {
                stage: "driver_load",
                code: Some(code as u32),
            });
        }
        return Err(format!("NVIDIA driver load: {error}"));
    }
    let library = context.library(unsafe { Library::from_raw(module as isize) }.into());
    let result = (|| {
        let init = driver_symbol(module, c"cuInit", failure)?;
        let get = driver_symbol(module, c"cuDeviceGet", failure)?;
        let name = driver_symbol(module, c"cuDeviceGetName", failure)?;
        let version = driver_symbol(module, c"cuDriverGetVersion", failure)?;
        let mut id = 0;
        let mut label = [0u8; 256];
        let mut driver_api_version = 0;
        let _uuid;
        // These four entry points are the CUDA Driver API's Windows signatures.
        // The exact System32 module is owned through every call.
        unsafe {
            let init: unsafe extern "system" fn(u32) -> i32 = std::mem::transmute(init);
            let get: unsafe extern "system" fn(*mut i32, i32) -> i32 = std::mem::transmute(get);
            let name: unsafe extern "system" fn(*mut c_char, i32, i32) -> i32 =
                std::mem::transmute(name);
            let version: unsafe extern "system" fn(*mut i32) -> i32 = std::mem::transmute(version);
            check_driver("cuInit", init(0), failure)?;
            check_driver("cuDeviceGet", get(&mut id, 0), failure)?;
            check_driver(
                "cuDeviceGetName",
                name(label.as_mut_ptr().cast(), label.len() as i32, id),
                failure,
            )?;
            _uuid = std::env::var("CUDA_VISIBLE_DEVICES")
                .ok()
                .map(|expected| verify_device_uuid(module, id, &expected, failure))
                .transpose()?;
            check_driver(
                "cuDriverGetVersion",
                version(&mut driver_api_version),
                failure,
            )?;
        }
        let name = CStr::from_bytes_until_nul(&label)
            .map_err(|error| error.to_string())?
            .to_str()
            .map_err(|error| error.to_string())?;
        if name.trim().is_empty() || name.chars().any(char::is_control) || driver_api_version <= 0 {
            return Err("invalid NVIDIA device identity".into());
        }
        Ok(Device {
            name: name.to_owned(),
            driver_api_version,
            #[cfg(feature = "native-bs-roformer-worker")]
            uuid: _uuid,
            #[cfg(feature = "native-bs-roformer-worker")]
            luid: query_adapter_luid(module, id),
        })
    })();
    library.complete()?;
    result
}

fn verify_device_uuid(
    module: *mut c_void,
    device: i32,
    expected: &str,
    failure: &mut Option<StartupError>,
) -> Result<String, String> {
    use std::fmt::Write as _;
    let address = driver_symbol(module, c"cuDeviceGetUuid", failure)?;
    // The Driver API writes one 16-byte CUuuid for the selected ordinal.
    let query: unsafe extern "system" fn(*mut u8, i32) -> i32 =
        unsafe { std::mem::transmute(address) };
    let mut uuid = [0u8; 16];
    let code = unsafe { query(uuid.as_mut_ptr(), device) };
    if code != 0 {
        return Err(format!(
            "CUDA device UUID query failed with driver status {code}"
        ));
    }
    let mut actual = String::from("GPU-");
    for (index, byte) in uuid.into_iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            actual.push('-');
        }
        write!(actual, "{byte:02x}").expect("writing to String");
    }
    if actual != expected {
        return Err("CUDA worker device differs from its prepared cache identity".into());
    }
    Ok(actual)
}

fn driver_symbol(
    module: *mut c_void,
    symbol: &CStr,
    failure: &mut Option<StartupError>,
) -> Result<*mut c_void, String> {
    let address = unsafe { GetProcAddress(module, symbol.as_ptr()) };
    if address.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_PROC_NOT_FOUND) {
            *failure = Some(StartupError {
                stage: "driver_symbol",
                code: Some(ERROR_PROC_NOT_FOUND as u32),
            });
        }
        return Err(format!(
            "NVIDIA driver entry {}: {error}",
            symbol.to_string_lossy()
        ));
    }
    Ok(address)
}

fn check_driver(
    operation: &str,
    status: i32,
    failure: &mut Option<StartupError>,
) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    let initializing = operation == "cuInit";
    if matches!(status, 3 | 46 | 100 | 802 | 803 | 804) || (status == 101 && !initializing) {
        *failure = Some(StartupError {
            stage: if initializing {
                "driver_init"
            } else {
                "driver_device"
            },
            code: Some(status as u32),
        });
    }
    Err(format!(
        "NVIDIA {operation} failed with driver status {status}"
    ))
}

pub fn model_startup_error(
    stage: ModelStartupStage,
    code: ort::ErrorCode,
    message: &str,
) -> Option<StartupError> {
    match stage {
        // Pinned ORT 1.24.4 CUDA option/append statuses. No device or loaded
        // provider assertion can be made at this boundary.
        ModelStartupStage::ProviderRegistration
            if code == ort::ErrorCode::GenericFailure
                && matches!(
                    message,
                    "OrtSessionOptionsAppendExecutionProvider_Cuda: Failed to load shared library"
                        | "CUDA execution provider is not enabled in this build."
                ) =>
        {
            Some(StartupError {
                stage: "provider_registration",
                code: None,
            })
        }
        ModelStartupStage::SessionInitialization => {
            let (status, gpu, file, call) = cuda_diagnostic(code, message)?;
            let unavailable = matches!(status, 3 | 35 | 46 | 100 | 101 | 802 | 803 | 804)
                && matches!(gpu, "0" | "-1")
                && file.ends_with("/cuda_execution_provider.cc")
                && [
                    "cudaSetDevice(",
                    "cudaDeviceSynchronize(",
                    "cudaGetDeviceProperties(",
                    "cudaStreamCreateWithFlags(",
                ]
                .into_iter()
                .any(|prefix| call.starts_with(prefix));
            let status = if unavailable {
                status
            } else {
                qualified_device_error(status, gpu, &file, call)?
            };
            Some(StartupError {
                stage: "session_initialization",
                code: Some(status),
            })
        }
        _ => None,
    }
}

/// Allow only pinned ORT CUDA runtime diagnostics from the typed Run error.
/// BFCArena, host/pinned allocations, cuDNN, unknown ordinals and OS faults
/// have no qualifying evidence here.
pub fn device_error(code: ort::ErrorCode, message: &str) -> Option<u32> {
    let (status, gpu, file, call) = cuda_diagnostic(code, message)?;
    qualified_device_error(status, gpu, &file, call)
}

fn cuda_diagnostic(code: ort::ErrorCode, message: &str) -> Option<(u32, &str, String, &str)> {
    if !matches!(
        code,
        ort::ErrorCode::GenericFailure | ort::ErrorCode::RuntimeException
    ) {
        return None;
    }
    let (lead, detail) = message.split_once("CUDA failure ")?;
    if !(lead.is_empty() || lead.contains("Status Message: ") || lead.contains("cuda_call.cc"))
        || detail.contains("CUDA failure ")
    {
        return None;
    }
    let (status, detail) = detail.split_once(": ")?;
    let status = status.parse::<u32>().ok()?;
    let (_, gpu) = detail.split_once(" ; GPU=")?;
    let (gpu, hostname) = gpu.split_once(" ; hostname=")?;
    let (_, file) = hostname.split_once(" ; file=")?;
    let (file, call) = file.split_once(" ; line=")?;
    let (line, call) = call.split_once(" ; expr=")?;
    let file = file.replace('\\', "/");
    let call = call.split_once(';')?.0.trim();
    if !file.contains("/core/providers/cuda/") || line.parse::<u32>().ok()? == 0 || call.is_empty()
    {
        return None;
    }
    Some((status, gpu, file, call))
}

fn qualified_device_error(status: u32, gpu: &str, file: &str, call: &str) -> Option<u32> {
    if gpu != "0" {
        return None;
    }
    match status {
        // cudaMalloc allocates device memory. cudaMallocHost and the host arena
        // must never trigger CPU fallback.
        2 if file.ends_with("/cuda_allocator.cc") && call.starts_with("cudaMalloc(") => {
            Some(status)
        }
        // Uncorrectable ECC, illegal device address, kernel timeout or launch failure.
        214 | 700 | 702 | 719 => Some(status),
        _ => None,
    }
}

#[cfg(feature = "native-beat-this-worker")]
pub fn kernel_events(
    own: &Invocation,
    prefix: &Path,
    profile: Option<&str>,
) -> Result<usize, String> {
    let profile = Path::new(profile.ok_or("missing finalized CUDA execution profile")?);
    if profile.parent() != prefix.parent() {
        return Err("CUDA execution profile is outside the owned directory".into());
    }
    let (bytes, _) = own.read(profile, 8 * 1024 * 1024)?;
    let events: Vec<Value> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let count = events
        .iter()
        .filter(|event| {
            event["cat"] == "Node"
                && event["ph"] == "X"
                && event["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("_kernel_time"))
                && event["args"]["provider"] == "CUDAExecutionProvider"
                && matches!(
                    event["args"]["op_name"].as_str(),
                    Some("Conv" | "FusedConv" | "MatMul" | "FusedMatMul" | "Gemm")
                )
        })
        .count();
    if count == 0 {
        return Err("no CUDA compute kernel was observed in the completed BeatThis run".into());
    }
    Ok(count)
}

pub fn runtime_evidence(device: &Device, receipt: &Value) -> Result<Value, String> {
    let images = receipt["loaded_package"]["observed_declared_images"]
        .as_array()
        .ok_or("missing CUDA package mapped image evidence")?;
    for (name, sha) in [
        ("onnxruntime_providers_cuda.dll", EP_SHA),
        ("cudart64_12.dll", CUDART_SHA),
    ] {
        if !images
            .iter()
            .any(|image| image["name"] == name && image["sha256"] == sha)
        {
            return Err(format!("missing admitted CUDA image {name}"));
        }
    }
    Ok(json!({
        "device_name": device.name,
        "driver_api_version": device.driver_api_version,
        "package_sha256": ORT_PACKAGE_SHA,
        "provider_sha256": EP_SHA,
    }))
}

#[cfg(feature = "native-bs-roformer-worker")]
fn query_adapter_luid(module: *mut c_void, device: i32) -> Option<([u8; 8], u32)> {
    // The selected CUDA device's Windows adapter LUID binds subsequent DXGI
    // telemetry. A driver without this optional query still supports inference.
    let address = unsafe { GetProcAddress(module, c"cuDeviceGetLuid".as_ptr()) };
    if address.is_null() {
        return None;
    }
    let query: unsafe extern "system" fn(*mut u8, *mut u32, i32) -> i32 =
        unsafe { std::mem::transmute(address) };
    let mut luid = [0; 8];
    let mut mask = 0;
    let result = unsafe { query(luid.as_mut_ptr(), &raw mut mask, device) };
    (result == 0 && mask != 0 && luid != [0; 8]).then_some((luid, mask))
}

#[cfg(feature = "native-bs-roformer-worker")]
impl Device {
    pub fn luid_and_node_mask(&self) -> Option<([u8; 8], u32)> {
        self.luid
    }

    pub fn verified_uuid(&self) -> Result<&str, String> {
        self.uuid
            .as_deref()
            .ok_or_else(|| "BS CUDA device UUID is required".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelStartupStage::{ProviderRegistration, SessionInitialization},
        device_error, model_startup_error,
    };
    use ort::ErrorCode::{GenericFailure, InvalidGraph, RuntimeException};

    #[test]
    fn cuda_startup_requires_the_expected_status_and_call_site() {
        let unavailable =
            "OrtSessionOptionsAppendExecutionProvider_Cuda: Failed to load shared library";
        let provider = model_startup_error(ProviderRegistration, GenericFailure, unavailable)
            .expect("pinned CUDA provider registration status");
        assert_eq!(
            (provider.stage, provider.code),
            ("provider_registration", None)
        );
        assert!(model_startup_error(ProviderRegistration, InvalidGraph, unavailable).is_none());
        assert!(
            model_startup_error(
                ProviderRegistration,
                GenericFailure,
                "Failed to load shared library"
            )
            .is_none()
        );

        let driver = r"cuda_call.cc CUDA failure 35: CUDA driver version is insufficient ; GPU=-1 ; hostname=fixture ; file=D:\ort\onnxruntime\core\providers\cuda\cuda_execution_provider.cc ; line=303 ; expr=cudaSetDevice(info_.device_id);";
        let session = model_startup_error(SessionInitialization, RuntimeException, driver)
            .expect("CUDA provider device initialization status");
        assert_eq!(
            (session.stage, session.code),
            ("session_initialization", Some(35))
        );
        assert!(model_startup_error(SessionInitialization, InvalidGraph, driver).is_none());
        for invalid in [
            driver.replace("GPU=-1", "GPU=1"),
            driver.replace("cudaSetDevice", "cudaMallocHost"),
            driver.replace("cuda_execution_provider.cc", "cuda_allocator.cc"),
        ] {
            assert!(
                model_startup_error(SessionInitialization, RuntimeException, &invalid).is_none()
            );
        }
    }

    #[test]
    fn qualified_cuda_run_errors_exclude_host_and_unrelated_failures() {
        let detail = r" ; GPU=0 ; hostname=fixture ; file=D:\ort\onnxruntime\core\providers\cuda\cuda_allocator.cc ; line=45 ; expr=";
        let oom = format!("CUDA failure 2: out of memory{detail}cudaMalloc(&p, size);");
        let timeout = format!(
            "Run failed. Status Message: CUDA failure 702: launch timeout{detail}cudaDeviceSynchronize();"
        );
        assert_eq!(device_error(GenericFailure, &oom), Some(2));
        assert_eq!(device_error(RuntimeException, &timeout), Some(702));
        assert_eq!(device_error(InvalidGraph, &oom), None);
        for error in [
            oom.replace("cudaMalloc(", "cudaMallocHost("),
            oom.replace("GPU=0", "GPU=1"),
            oom.replace("CUDA failure 2", "CUDNN failure 4003"),
            oom.replace("CUDA failure 2", "CUDA failure 999"),
            oom.replace("cuda_allocator.cc", "cuda_execution_provider.cc"),
            "BFCArena failed allocation".into(),
        ] {
            assert_eq!(device_error(GenericFailure, &error), None, "{error}");
        }
    }
}
