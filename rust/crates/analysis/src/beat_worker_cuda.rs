use flitzis_looper_analysis::{beat_inference::Provider, trial_lifetime::Context};
use libloading::os::windows::Library;
use serde_json::{Value, json};
use std::{ffi::CStr, path::Path};

use super::invocation::Invocation;

pub const ORT_MANIFEST_SHA: &str =
    "7ebc26a630b9fb721539e7821b918cf9f4a9aa94b856dcc153b81a7b140b240d";
pub const ORT_PACKAGE_SHA: &str =
    "d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b";
const EP_SHA: &str = "110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494";
const CUDART_SHA: &str = "c2c9a9c22a9bcba90e261825968836787b331038047a26770cffb7a583c28344";

pub fn provider() -> Provider {
    Provider::Cuda {
        memory_limit: 1024 * 1024 * 1024,
    }
}

pub struct Device {
    name: String,
    driver_api_version: i32,
}

pub fn device(context: &Context) -> Result<Device, String> {
    context.checkpoint("cuda.device")?;
    // The driver is supplied by Windows/NVIDIA. The explicit search flag keeps
    // the App directory, working directory and package out of this driver lookup.
    let library = unsafe { Library::load_with_flags("nvcuda.dll", 0x800) }
        .map_err(|error| format!("NVIDIA driver load: {error}"))?;
    let library = context.library(library.into());
    let result = (|| {
        // CUDA Driver API signatures. Resolve symbols while holding the library.
        unsafe {
            let init = library
                .get::<unsafe extern "system" fn(u32) -> i32>(b"cuInit\0")
                .map_err(|error| error.to_string())?;
            let get = library
                .get::<unsafe extern "system" fn(*mut i32, i32) -> i32>(b"cuDeviceGet\0")
                .map_err(|error| error.to_string())?;
            let name = library
                .get::<unsafe extern "system" fn(*mut std::ffi::c_char, i32, i32) -> i32>(
                    b"cuDeviceGetName\0",
                )
                .map_err(|error| error.to_string())?;
            let version = library
                .get::<unsafe extern "system" fn(*mut i32) -> i32>(b"cuDriverGetVersion\0")
                .map_err(|error| error.to_string())?;
            let mut id = 0;
            let mut label = [0u8; 256];
            let mut driver_api_version = 0;
            check_driver("cuInit", init(0))?;
            check_driver("cuDeviceGet", get(&mut id, 0))?;
            check_driver(
                "cuDeviceGetName",
                name(label.as_mut_ptr().cast(), label.len() as i32, id),
            )?;
            check_driver("cuDriverGetVersion", version(&mut driver_api_version))?;
            let name = CStr::from_bytes_until_nul(&label)
                .map_err(|error| error.to_string())?
                .to_str()
                .map_err(|error| error.to_string())?;
            if name.trim().is_empty()
                || name.chars().any(char::is_control)
                || driver_api_version <= 0
            {
                return Err("invalid NVIDIA device identity".into());
            }
            Ok(Device {
                name: name.to_owned(),
                driver_api_version,
            })
        }
    })();
    library.complete()?;
    result
}

fn check_driver(operation: &str, status: i32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "NVIDIA {operation} failed with driver status {status}"
        ))
    }
}

/// Allow only pinned ORT CUDA runtime diagnostics from the typed Run error.
/// BFCArena, host/pinned allocations, cuDNN, unknown ordinals and OS faults
/// have no qualifying evidence here.
pub fn device_error(code: ort::ErrorCode, message: &str) -> Option<u32> {
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
    let (_, file) = detail
        .split_once(" ; GPU=0 ; hostname=")?
        .1
        .split_once(" ; file=")?;
    let (file, call) = file.split_once(" ; line=")?;
    let (line, call) = call.split_once(" ; expr=")?;
    let file = file.replace('\\', "/");
    let call = call.split_once(';')?.0.trim();
    if !file.contains("/core/providers/cuda/") || line.parse::<u32>().ok()? == 0 || call.is_empty()
    {
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

#[cfg(test)]
mod tests {
    use super::device_error;
    use ort::ErrorCode::{GenericFailure, InvalidGraph, RuntimeException};

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
