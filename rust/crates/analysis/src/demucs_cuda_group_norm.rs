//! Stock-compatible GroupNorm for the checked HTDemucs CUDA candidate.

use crate::trial_lifetime::{Context, OwnedLibrary};
use libloading::os::windows::Library;
use ort::{
    Error, Result,
    memory::AllocationDevice,
    operator::{
        Kernel, KernelAttributes, KernelContext, Operator, OperatorDomain, OperatorInput,
        OperatorOutput,
    },
    value::{TensorElementType, TensorValueType},
};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr::{NonNull, null_mut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
    },
};

const DOMAIN: &str = "flitzis.htdemucs.cuda";
const PTX: &CStr = match CStr::from_bytes_with_nul(
    concat!(include_str!("demucs_cuda_group_norm.ptx"), "\0").as_bytes(),
) {
    Ok(ptx) => ptx,
    Err(_) => panic!("invalid embedded HTDemucs GroupNorm PTX"),
};

type ContextGet = unsafe extern "system" fn(*mut *mut c_void) -> i32;
type ContextDevice = unsafe extern "system" fn(*mut i32) -> i32;
type ContextSwitch = unsafe extern "system" fn(*mut c_void) -> i32;
type DeviceAttribute = unsafe extern "system" fn(*mut i32, i32, i32) -> i32;
type ModuleLoad = unsafe extern "system" fn(*mut *mut c_void, *const c_void) -> i32;
type ModuleFunction =
    unsafe extern "system" fn(*mut *mut c_void, *mut c_void, *const c_char) -> i32;
type ModuleUnload = unsafe extern "system" fn(*mut c_void) -> i32;
type Launch = unsafe extern "system" fn(
    *mut c_void,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    *mut c_void,
    *mut *mut c_void,
    *mut *mut c_void,
) -> i32;

struct Driver {
    _library: OwnedLibrary,
    current_context: ContextGet,
    context_device: ContextDevice,
    set_context: ContextSwitch,
    device_attribute: DeviceAttribute,
    module_load: ModuleLoad,
    module_function: ModuleFunction,
    module_unload: ModuleUnload,
    launch: Launch,
}

impl Driver {
    fn open(context: &Context) -> std::result::Result<Self, String> {
        const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;
        // Resolve the NVIDIA driver only through Windows System32 and retain
        // the library until its module and all kernel readers have retired.
        let library =
            unsafe { Library::load_with_flags("nvcuda.dll", LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|e| format!("HTDemucs GroupNorm driver load: {e}"))?;
        let library = context.library(library.into());
        Ok(Self {
            current_context: symbol(&library, c"cuCtxGetCurrent")?,
            context_device: symbol(&library, c"cuCtxGetDevice")?,
            set_context: symbol(&library, c"cuCtxSetCurrent")?,
            device_attribute: symbol(&library, c"cuDeviceGetAttribute")?,
            module_load: symbol(&library, c"cuModuleLoadData")?,
            module_function: symbol(&library, c"cuModuleGetFunction")?,
            module_unload: symbol(&library, c"cuModuleUnload")?,
            launch: symbol(&library, c"cuLaunchKernel")?,
            _library: library,
        })
    }

    fn context(&self) -> std::result::Result<*mut c_void, String> {
        let mut value = null_mut();
        status("cuCtxGetCurrent", unsafe {
            (self.current_context)(&mut value)
        })?;
        Ok(value)
    }

    fn qualify_device(&self) -> std::result::Result<(), String> {
        const COMPUTE_MAJOR: i32 = 75;
        const COMPUTE_MINOR: i32 = 76;
        let mut device = -1;
        status("cuCtxGetDevice", unsafe {
            (self.context_device)(&mut device)
        })?;
        let mut major = 0;
        let mut minor = 0;
        status("cuDeviceGetAttribute", unsafe {
            (self.device_attribute)(&mut major, COMPUTE_MAJOR, device)
        })?;
        status("cuDeviceGetAttribute", unsafe {
            (self.device_attribute)(&mut minor, COMPUTE_MINOR, device)
        })?;
        if device != 0 || (major, minor) != (12, 0) {
            return Err(format!(
                "unqualified HTDemucs GroupNorm CUDA device {device}, capability {major}.{minor}"
            ));
        }
        Ok(())
    }
}

fn symbol<T: Copy>(library: &OwnedLibrary, name: &CStr) -> std::result::Result<T, String> {
    // Each caller uses the driver's documented ABI for the exact symbol.
    unsafe { library.get::<T>(name.to_bytes_with_nul()) }
        .map(|value| *value)
        .map_err(|e| format!("HTDemucs GroupNorm {}: {e}", name.to_string_lossy()))
}

fn status(operation: &str, code: i32) -> std::result::Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "HTDemucs GroupNorm {operation} returned CUDA status {code}"
        ))
    }
}

#[derive(Clone, Copy)]
struct Module {
    handle: NonNull<c_void>,
    function: NonNull<c_void>,
    cuda_context: NonNull<c_void>,
}

// CUDA module, function and context handles are opaque. Calls check the
// recorded ORT CUDA context, and no pointer is dereferenced on the host.
unsafe impl Send for Module {}

struct Owner {
    driver: Driver,
    module: Mutex<Option<Module>>,
    context: Context,
    failed_launch: Arc<AtomicI32>,
}

impl Owner {
    fn module(&self) -> std::result::Result<Module, String> {
        let cuda_context = NonNull::new(self.driver.context()?)
            .ok_or("HTDemucs GroupNorm has no active ORT CUDA context")?;
        let mut module = self
            .module
            .lock()
            .map_err(|_| "HTDemucs GroupNorm module mutex poisoned")?;
        if let Some(module) = *module {
            if module.cuda_context != cuda_context {
                return Err("HTDemucs GroupNorm changed CUDA context".into());
            }
            return Ok(module);
        }
        self.driver.qualify_device()?;

        let mut handle = null_mut();
        status("cuModuleLoadData", unsafe {
            (self.driver.module_load)(&mut handle, PTX.as_ptr().cast())
        })?;
        let handle = NonNull::new(handle).ok_or("missing HTDemucs GroupNorm CUDA module")?;
        let mut function = null_mut();
        let result = status("cuModuleGetFunction", unsafe {
            (self.driver.module_function)(
                &mut function,
                handle.as_ptr(),
                c"htdemucs_group_norm".as_ptr(),
            )
        })
        .and_then(|()| {
            NonNull::new(function).ok_or_else(|| "missing HTDemucs GroupNorm CUDA function".into())
        });
        if result.is_err() {
            self.context.record(status("cuModuleUnload", unsafe {
                (self.driver.module_unload)(handle.as_ptr())
            }));
        }
        let ready = Module {
            handle,
            function: result?,
            cuda_context,
        };
        *module = Some(ready);
        Ok(ready)
    }

    fn retire(&mut self) -> std::result::Result<(), String> {
        let Some(module) = self
            .module
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        else {
            return Ok(());
        };
        let previous = self.driver.context()?;
        let owned = module.cuda_context.as_ptr();
        if previous != owned {
            status("cuCtxSetCurrent", unsafe {
                (self.driver.set_context)(owned)
            })?;
        }
        // ORT owns the tensors and stream until its synchronous run returns.
        let first = status("cuModuleUnload", unsafe {
            (self.driver.module_unload)(module.handle.as_ptr())
        });
        if previous != owned {
            let restore = status("cuCtxSetCurrent", unsafe {
                (self.driver.set_context)(previous)
            });
            self.context.record(restore);
        }
        first
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let result = self.retire();
        self.context.record(result);
    }
}

struct GroupNorm(Arc<Owner>);

impl Operator for GroupNorm {
    fn name(&self) -> &str {
        "GroupNorm"
    }

    fn execution_provider_type(&self) -> Option<&str> {
        Some("CUDAExecutionProvider")
    }

    fn min_version(&self) -> i32 {
        1
    }
    fn max_version(&self) -> i32 {
        1
    }

    fn inputs(&self) -> Vec<OperatorInput> {
        (0..3)
            .map(|_| OperatorInput::required(TensorElementType::Float32))
            .collect()
    }

    fn outputs(&self) -> Vec<OperatorOutput> {
        vec![OperatorOutput::required(TensorElementType::Float32)]
    }

    fn create_kernel(&self, attributes: &KernelAttributes) -> Result<Box<dyn Kernel>> {
        if attributes.get::<i64>("groups") != Some(1) {
            return Err(Error::new("unsupported HTDemucs GroupNorm group count"));
        }
        let owner = Arc::clone(&self.0);
        Ok(Box::new(move |context: &KernelContext| {
            compute(&owner, context)
        }))
    }
}

fn geometry(shape: &[i64]) -> Result<(u32, i32, i32, u32)> {
    let [batch, channels, spatial @ ..] = shape else {
        return Err(Error::new("invalid HTDemucs GroupNorm tensor rank"));
    };
    if !matches!(*batch, 1 | 8 | 32 | 128 | 512)
        || !matches!(*channels, 6 | 12 | 24 | 48 | 96 | 192 | 384 | 512 | 768)
        || !matches!(spatial.len(), 1 | 2)
    {
        return Err(Error::new("invalid HTDemucs GroupNorm tensor geometry"));
    }
    let spatial = spatial
        .iter()
        .try_fold(1i32, |acc, axis| {
            i32::try_from(*axis)
                .ok()
                .filter(|&axis| axis > 0)
                .and_then(|axis| acc.checked_mul(axis))
        })
        .ok_or_else(|| Error::new("invalid HTDemucs GroupNorm spatial extent"))?;
    let channels = *channels as i32;
    let batch = *batch as u32;
    let row_size = channels
        .checked_mul(spatial)
        .and_then(|row| row.checked_mul(batch as i32).map(|_| row))
        .ok_or_else(|| Error::new("HTDemucs GroupNorm extent overflow"))?;
    Ok((
        batch,
        channels,
        spatial,
        if row_size < 512 { 32 } else { 512 },
    ))
}

fn compute(owner: &Owner, context: &KernelContext) -> Result<()> {
    owner.context.check().map_err(Error::new)?;
    let input = |index| {
        context
            .input(index)?
            .ok_or_else(|| Error::new("missing HTDemucs GroupNorm input"))
    };
    let mixture = input(0)?;
    let gamma = input(1)?;
    let beta = input(2)?;
    let mixture = mixture.downcast_ref::<TensorValueType<f32>>()?;
    let gamma = gamma.downcast_ref::<TensorValueType<f32>>()?;
    let beta = beta.downcast_ref::<TensorValueType<f32>>()?;
    let shape = mixture.shape();
    let (batch, mut channels, mut spatial, threads) = geometry(shape)?;
    if **gamma.shape() != [i64::from(channels)] || **beta.shape() != [i64::from(channels)] {
        return Err(Error::new("invalid HTDemucs GroupNorm affine dimensions"));
    }
    let mut output = context
        .output(0, shape.clone())?
        .ok_or_else(|| Error::new("missing HTDemucs GroupNorm output"))?;
    let mut output = output.downcast_mut::<TensorValueType<f32>>()?;
    for memory in [
        mixture.memory_info(),
        gamma.memory_info(),
        beta.memory_info(),
        output.memory_info(),
    ] {
        if memory.allocation_device() != AllocationDevice::CUDA
            || memory.device_id() != 0
            || memory.is_cpu_accessible()
        {
            return Err(Error::new(
                "HTDemucs GroupNorm requires CUDA tensors on device 0",
            ));
        }
    }
    let stream = context
        .compute_stream()?
        .ok_or_else(|| Error::new("missing HTDemucs GroupNorm ORT compute stream"))?;
    let mut pointers = [
        mixture.data_ptr().cast_mut(),
        gamma.data_ptr().cast_mut(),
        beta.data_ptr().cast_mut(),
        output.data_ptr_mut(),
    ];
    if pointers.iter().any(|p| p.is_null()) {
        return Err(Error::new("null HTDemucs GroupNorm tensor"));
    }
    let [x, gamma, beta, output] = &mut pointers;
    let mut args = [
        std::ptr::from_mut(x).cast(),
        std::ptr::from_mut(gamma).cast(),
        std::ptr::from_mut(beta).cast(),
        std::ptr::from_mut(output).cast(),
        std::ptr::from_mut(&mut channels).cast(),
        std::ptr::from_mut(&mut spatial).cast(),
    ];
    let module = owner.module().map_err(Error::new)?;
    // CUDA copies the six argument slots synchronously. Work then stays on
    // ORT's stream; no Rust references are retained by the launched kernel.
    let code = unsafe {
        (owner.driver.launch)(
            module.function.as_ptr(),
            batch,
            1,
            1,
            threads,
            1,
            1,
            0,
            stream.as_ptr(),
            args.as_mut_ptr(),
            null_mut(),
        )
    };
    if matches!(code, 214 | 700 | 702 | 719) {
        owner.failed_launch.store(code, Ordering::Relaxed);
    }
    status("cuLaunchKernel", code).map_err(Error::new)
}

pub(crate) fn operators(
    context: &Context,
    failed_launch: Arc<AtomicI32>,
) -> std::result::Result<OperatorDomain, String> {
    context.checkpoint("demucs.cuda.group_norm")?;
    let owner = Arc::new(Owner {
        driver: Driver::open(context)?,
        module: Mutex::new(None),
        context: context.clone(),
        failed_launch,
    });
    OperatorDomain::new(DOMAIN)
        .and_then(|domain| domain.add(GroupNorm(owner)))
        .map_err(|e| e.to_string())
}
