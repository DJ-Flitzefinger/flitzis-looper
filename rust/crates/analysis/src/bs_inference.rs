//! The pinned BS-RoFormer release's external-data mask graph.
use crate::{
    beat_inference::ModelStartupStage,
    trial_lifetime::{CANCELLED, Context, terminate_during},
    trial_ort::{commit_file_observed, is_run_termination, session_builder_observed},
};
use flitzis_stem_dsp::{
    BINS, CHANNELS, HOP, STEMS,
    track::{BATCH, CHUNK},
};
use ort::{
    AsPointer, ep,
    memory::{AllocationDevice, Allocator, AllocatorType, MemoryInfo, MemoryType},
    session::{RunOptions, Session},
    sys::{OrtKeyValuePairs, OrtMemoryInfo},
    value::{TensorElementType, TensorRef, ValueType},
};
use std::{
    ffi::CStr,
    path::Path,
    ptr::{self, NonNull},
};

pub const STEPS: usize = CHUNK / HOP + 1;
pub const WIDTH: usize = BINS * CHANNELS * 2;
pub const FEATURES: usize = STEPS * WIDTH;
pub const MASKS: usize = STEMS * FEATURES;

pub struct MaskModel(Session);

#[derive(serde::Serialize)]
pub struct CudaArenaStats {
    kind: &'static str,
    max_in_use_bytes: u64,
    peak_reserved_bytes: Option<u64>,
}

struct AllocatorStats(NonNull<OrtKeyValuePairs>);

impl AllocatorStats {
    fn get(&self, key: &CStr) -> Option<u64> {
        // The string is borrowed from the live ORT key/value collection.
        let value = unsafe { (ort::api().GetKeyValue)(self.0.as_ptr(), key.as_ptr()) };
        if value.is_null() {
            return None;
        }
        let value = unsafe { CStr::from_ptr(value) }.to_bytes();
        if !value.iter().all(u8::is_ascii_digit) {
            return None;
        }
        str::from_utf8(value).ok()?.parse().ok()
    }
}

impl Drop for AllocatorStats {
    fn drop(&mut self) {
        unsafe { (ort::api().ReleaseKeyValuePairs)(self.0.as_ptr()) };
    }
}

impl MaskModel {
    /// Load a verified graph file with file-relative weights. The caller must
    /// retain both Windows file pins until the session and its readers have ended.
    pub fn load(
        graph: &Path,
        profile: Option<&Path>,
        cuda: bool,
        context: &Context,
        mut observe: impl FnMut(ModelStartupStage, ort::ErrorCode, &str),
    ) -> Result<Self, String> {
        context.checkpoint("bs.model.prepare")?;
        let cpu = ep::CPU::default()
            .with_arena_allocator(false)
            .build()
            .error_on_failure();
        let providers = if cuda {
            vec![
                ep::CUDA::default()
                    .with_device_id(0)
                    .with_tf32(false)
                    .with_arena_extend_strategy(ep::ArenaExtendStrategy::SameAsRequested)
                    .build()
                    .error_on_failure(),
                cpu,
            ]
        } else {
            vec![cpu]
        };
        let builder = session_builder_observed(&providers, |code, message| {
            observe(ModelStartupStage::ProviderRegistration, code, message);
        })?
        .with_no_environment_execution_providers()
        .map_err(|error| error.to_string())?;
        // Batch size changes at the track tail. Let the CUDA provider reuse
        // arena allocations by extent without retaining a shape-specific memory pattern.
        let builder = if cuda {
            builder
                .with_memory_pattern(false)
                .map_err(|error| error.to_string())?
        } else {
            builder
        };
        let builder = match profile {
            Some(path) => builder
                .with_profiling(path)
                .map_err(|error| error.to_string())?,
            None => builder,
        };
        context.checkpoint("bs.model.load")?;
        let mut session = commit_file_observed(builder, graph, context, |code, message| {
            observe(ModelStartupStage::SessionInitialization, code, message);
        })?;
        let validate = (|| {
            context.cleanup()?;
            context.check()?;
            let [input] = session.inputs() else {
                return Err("BS mask graph requires exactly one input".into());
            };
            let [output] = session.outputs() else {
                return Err("BS mask graph requires exactly one output".into());
            };
            if input.name() != "features"
                || !signature(input.dtype(), &[-1, STEPS as i64, WIDTH as i64])
                || output.name() != "masks"
                || !signature(
                    output.dtype(),
                    &[-1, STEMS as i64, STEPS as i64, WIDTH as i64],
                )
            {
                return Err("BS mask graph signature differs from the release".into());
            }
            Ok(())
        })();
        if let Err(error) = validate {
            if profile.is_some() {
                context.record(
                    session
                        .end_profiling()
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                );
            }
            return Err(error);
        }
        Ok(Self(session))
    }

    /// Borrow sealed features until ORT returns. The output consumer also
    /// borrows ORT-owned storage, avoiding a second complete mask allocation.
    pub fn run<T>(
        &mut self,
        batch: usize,
        features: &[f32],
        context: &Context,
        observe: impl FnOnce(ort::ErrorCode, &str),
        consume: impl FnOnce(&[f32]) -> Result<T, String>,
    ) -> Result<T, String> {
        context.checkpoint("bs.model.run")?;
        if !(1..=BATCH).contains(&batch) || features.len() != batch * FEATURES {
            return Err("invalid complete BS feature batch".into());
        }
        finite(features, context)?;
        let options = RunOptions::new().map_err(|error| error.to_string())?;
        let mut native_terminated = false;
        let (outputs, requested) = terminate_during(
            context,
            None,
            || options.terminate().map_err(|error| error.to_string()),
            || {
                let tensor = TensorRef::from_array_view(([batch, STEPS, WIDTH], features))
                    .map_err(|error| error.to_string())?;
                self.0
                    .run_with_options(ort::inputs!["features" => tensor], &options)
                    .map_err(|error| {
                        native_terminated = is_run_termination(error.code(), error.message());
                        observe(error.code(), error.message());
                        error.to_string()
                    })
            },
        );
        let outputs = if native_terminated && requested && context.cancelled() {
            outputs.map_err(|error| format!("{CANCELLED}; native run: {error}"))?
        } else {
            outputs?
        };
        context.cleanup()?;
        context.check()?;
        let output = outputs.get("masks").ok_or("missing complete BS masks")?;
        let (shape, masks) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        if shape.as_ref() != [batch as i64, STEMS as i64, STEPS as i64, WIDTH as i64]
            || masks.len() != batch * MASKS
        {
            return Err("BS output mask batch shape differs".into());
        }
        finite(masks, context)?;
        consume(masks)
    }

    pub fn finish_profile(&mut self) -> Result<String, String> {
        self.0.end_profiling().map_err(|error| error.to_string())
    }

    /// Snapshot the pinned CUDA provider's device arena before the session ends.
    pub fn cuda_arena_stats(&self) -> Option<CudaArenaStats> {
        let info = MemoryInfo::new(
            AllocationDevice::CUDA,
            0,
            AllocatorType::Arena,
            MemoryType::Default,
        )
        .ok()?;
        let allocator = Allocator::new(&self.0, info).ok()?;
        let api = ort::api();
        let mut actual_info: *const OrtMemoryInfo = ptr::null();
        // ORT owns the returned memory info for the lifetime of the allocator.
        unsafe {
            ort::Error::result_from_status((api.AllocatorGetInfo)(
                allocator.ptr(),
                &mut actual_info,
            ))
        }
        .ok()?;
        if actual_info.is_null() {
            return None;
        }
        let mut different = -1;
        unsafe {
            ort::Error::result_from_status((api.CompareMemoryInfo)(
                allocator.memory_info().ptr(),
                actual_info,
                &mut different,
            ))
        }
        .ok()?;
        if different != 0 {
            return None;
        }

        let mut stats = ptr::null_mut();
        let status = unsafe { (api.AllocatorGetStats)(allocator.ptr(), &mut stats) };
        let stats = NonNull::new(stats).map(AllocatorStats);
        unsafe { ort::Error::result_from_status(status) }.ok()?;
        let stats = stats?;
        let max_in_use_bytes = stats.get(c"MaxInUse")?;

        // TotalAllocated is the current reservation. The pinned BFCArena can
        // lower it via region shrinkage or the release of a direct reservation.
        let peak_reserved_bytes =
            match (stats.get(c"NumReserves"), stats.get(c"NumArenaShrinkages")) {
                (Some(0), Some(0)) => stats
                    .get(c"TotalAllocated")
                    .filter(|total| *total >= max_in_use_bytes),
                _ => None,
            };
        Some(CudaArenaStats {
            kind: "ort-cuda-device-arena",
            max_in_use_bytes,
            peak_reserved_bytes,
        })
    }
}

fn signature(value: &ValueType, expected: &[i64]) -> bool {
    matches!(
        value,
        ValueType::Tensor { ty: TensorElementType::Float32, shape, .. }
            if shape.as_ref() == expected
    )
}

fn finite(values: &[f32], context: &Context) -> Result<(), String> {
    for block in values.chunks(4096) {
        context.check()?;
        if block.iter().any(|value| !value.is_finite()) {
            return Err("nonfinite BS feature or mask".into());
        }
    }
    Ok(())
}
