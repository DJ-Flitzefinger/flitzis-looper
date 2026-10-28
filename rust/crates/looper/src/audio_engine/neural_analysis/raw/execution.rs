//! Exact execution receipt of the one known CUDA-capable worker profile.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecutionReceipt {
    schema_version: u32,
    requested_policy: String,
    device: String,
    device_name: Option<String>,
    precision: String,
    torch_version: String,
    cuda_build_version: String,
    fallback_reason: Option<String>,
}

impl ExecutionReceipt {
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
