//! Closed execution receipts for the Torch and Native CUDA-capable profiles.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TorchExecutionReceipt {
    schema_version: u32,
    requested_policy: String,
    device: String,
    device_name: Option<String>,
    precision: String,
    torch_version: String,
    cuda_build_version: String,
    fallback_reason: Option<String>,
}

impl TorchExecutionReceipt {
    pub(super) fn validate(&self) -> Result<(), String> {
        let cpu = self.device == "cpu";
        let cuda = self.device == "cuda:0";
        let fallback = self.fallback_reason.as_deref().is_some_and(|reason| {
            matches!(
                reason,
                "cuda_unavailable"
                    | "cuda_probe_error"
                    | "cuda_placement_error"
                    | "cuda_inference_error"
            )
        });
        if self.schema_version != 1
            || self.requested_policy != "auto"
            || self.precision != "float32"
            || self.torch_version != "2.8.0+cu128"
            || self.cuda_build_version != "12.8"
            || !(cpu && self.device_name.is_none() && fallback
                || cuda
                    && self.fallback_reason.is_none()
                    && self.device_name.as_ref().is_some_and(|name| {
                        !name.is_empty() && name.len() <= 1024 && name.chars().count() <= 256
                    }))
        {
            return Err("Beat This execution receipt differs from the known auto profile".into());
        }
        Ok(())
    }
}

const CUDA_PACKAGE: &str = "d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b";
const CPU_PACKAGE: &str = "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319";
const CUDA_PROVIDER: &str = "110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494";

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum ExecutionReceipt {
    Torch(TorchExecutionReceipt),
    NativeCuda(NativeCudaExecutionReceipt),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeCudaRuntime {
    device_name: String,
    driver_api_version: i32,
    package_sha256: String,
    provider_sha256: String,
}

impl NativeCudaRuntime {
    fn valid(&self) -> bool {
        self.package_sha256 == CUDA_PACKAGE
            && self.provider_sha256 == CUDA_PROVIDER
            && self.driver_api_version > 0
            && !self.device_name.trim().is_empty()
            && self.device_name.chars().count() <= 256
            && !self.device_name.chars().any(char::is_control)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeCudaStartupError {
    stage: String,
    #[serde(deserialize_with = "Option::deserialize")]
    code: Option<u32>,
}

impl NativeCudaStartupError {
    fn fallback_reason(&self) -> Option<&'static str> {
        match (self.stage.as_str(), self.code) {
            ("driver_load", Some(126))
            | ("driver_init", Some(100))
            | ("driver_device" | "session_initialization", Some(100 | 101))
            | ("provider_registration", None) => Some("cuda_unavailable"),
            ("driver_load" | "driver_symbol", Some(127))
            | ("driver_init" | "driver_device", Some(3 | 46 | 802 | 803 | 804))
            | (
                "session_initialization",
                Some(2 | 3 | 35 | 46 | 214 | 700 | 702 | 719 | 802 | 803 | 804),
            ) => Some("cuda_startup_error"),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeCudaExecutionReceipt {
    schema_version: u32,
    requested_policy: String,
    device: String,
    device_name: Option<String>,
    precision: String,
    ort_version: String,
    ort_api: u32,
    ort_package_sha256: String,
    #[serde(deserialize_with = "Option::deserialize")]
    cuda: Option<NativeCudaRuntime>,
    cuda_kernel_events: Option<u32>,
    fallback_reason: Option<String>,
    cuda_error: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cuda_startup_error: Option<NativeCudaStartupError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cuda_cache_error: Option<String>,
}

impl NativeCudaExecutionReceipt {
    pub(super) fn validate(&self, policy: &str) -> Result<(), String> {
        let qualified = match self.device.as_str() {
            "cuda:0" => {
                self.ort_package_sha256 == CUDA_PACKAGE
                    && self.cuda.as_ref().is_some_and(|cuda| {
                        cuda.valid() && self.device_name.as_ref() == Some(&cuda.device_name)
                    })
                    && self
                        .cuda_kernel_events
                        .is_some_and(|n| (1..=100_000).contains(&n))
                    && self.fallback_reason.is_none()
                    && self.cuda_error.is_none()
                    && self.cuda_startup_error.is_none()
                    && self.cuda_cache_error.is_none()
            }
            "cpu" => {
                policy == "auto"
                    && self.ort_package_sha256 == CPU_PACKAGE
                    && self.device_name.is_none()
                    && self.cuda_kernel_events.is_none()
                    && self.valid_fallback()
            }
            _ => false,
        };
        if !qualified
            || self.schema_version != 1
            || self.requested_policy != policy
            || !matches!(policy, "auto" | "cuda")
            || self.precision != "float32"
            || self.ort_version != "1.24.4"
            || self.ort_api != 24
        {
            return Err("Beat This Native CUDA execution evidence differs from the request".into());
        }
        Ok(())
    }

    fn valid_fallback(&self) -> bool {
        if let Some(detail) = &self.cuda_cache_error {
            return self.cuda_startup_error.is_none()
                && self.cuda.is_none()
                && self.cuda_error.is_none()
                && self.fallback_reason.as_deref() == Some("cuda_cache_unavailable")
                && (1..=512).contains(&detail.chars().count())
                && !detail.chars().any(char::is_control);
        }
        match (&self.cuda_startup_error, &self.cuda) {
            (None, Some(cuda)) => {
                cuda.valid()
                    && self.fallback_reason.as_deref() == Some("cuda_device_error")
                    && matches!(self.cuda_error, Some(2 | 214 | 700 | 702 | 719))
            }
            (Some(startup), None) => {
                self.cuda_error.is_none()
                    && startup
                        .fallback_reason()
                        .is_some_and(|reason| self.fallback_reason.as_deref() == Some(reason))
            }
            _ => false,
        }
    }
}
