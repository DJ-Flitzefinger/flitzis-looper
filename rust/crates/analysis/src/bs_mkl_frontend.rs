//! CPU STFT matching the pinned original Windows BS-RoFormer reference.
use super::files;
use flitzis_looper_analysis::{
    bs_inference::STEPS,
    trial_lifetime::{Context, OwnedFile, OwnedLibrary},
};
use flitzis_stem_dsp::{
    BINS, CHANNELS, FFT, HOP,
    track::{BATCH, CHUNK},
};
use libloading::os::windows::Library;
use std::{
    ffi::{CStr, c_char, c_int, c_long, c_void},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

const DLL_LOAD_DIR: u32 = 0x100;
const SYSTEM32: u32 = 0x800;
const HANN: &[u8; FFT * size_of::<f32>()] =
    include_bytes!("../../../../workers/bs_roformer/hann-cpu.f32le");
const VERSION: &str = "Intel(R) oneAPI Math Kernel Library Version 2026.0-Product Build 20260401 for Intel(R) 64 architecture applications";

// The pins identify the Intel 2026.0.0 Windows runtime subset and the exact
// OpenMP component used in the independent full-geometry feature comparison.
const COMPONENTS: [(&str, usize, &str); 9] = [
    (
        "libiomp5md.dll",
        1_617_256,
        "2299b0460e8118e8187fd57a8d17df836c2a3d59f2639c3681582070da66b7be",
    ),
    (
        "mkl_rt.3.dll",
        27_890_848,
        "6a312eb1c4a92447e8d4c11d07420678bd07e0666c279fc621a3a6b03db527d8",
    ),
    (
        "mkl_avx10.3.dll",
        67_684_512,
        "c5122e4cb2aaa681039ca2cd39e8fe706f44ed697c943c85e87661e40c0fc609",
    ),
    (
        "mkl_avx2.3.dll",
        47_482_016,
        "b3d7212310180988f4d2974b212f6a2e978ea3e2377715a2de7d8a070d10d54f",
    ),
    (
        "mkl_avx512.3.dll",
        75_416_736,
        "d35544ffe706d75e93ce7f318c3587007ee3be9816a5a14215054721e6bf3d42",
    ),
    (
        "mkl_core.3.dll",
        71_903_904,
        "54293955a106c62a4683d306851a78e6b3dd6ba64749f33639e61b9876956e18",
    ),
    (
        "mkl_def.3.dll",
        37_053_600,
        "66cfb9935d2ce7811b49b56ed02ac454500e3d2f8e399724d6fc829a7fe91aba",
    ),
    (
        "mkl_intel_thread.3.dll",
        39_083_680,
        "eb6a026ae6124f51b8716a901f83441778cb4b63540ca8f4374d5e188299aa03",
    ),
    (
        "mkl_mc3.3.dll",
        47_141_024,
        "5fd371f139b1041ab3c76e8261ed5dcbde86cc351c7aaf589f9f0a27e16c800f",
    ),
];
const DFTI_NUMBER_OF_TRANSFORMS: c_int = 7;
const DFTI_CONJUGATE_EVEN_STORAGE: c_int = 10;
const DFTI_PLACEMENT: c_int = 11;
const DFTI_INPUT_STRIDES: c_int = 12;
const DFTI_OUTPUT_STRIDES: c_int = 13;
const DFTI_INPUT_DISTANCE: c_int = 14;
const DFTI_OUTPUT_DISTANCE: c_int = 15;
const DFTI_REAL: c_int = 33;
const DFTI_SINGLE: c_int = 35;
const DFTI_COMPLEX_COMPLEX: c_int = 39;
const DFTI_NOT_INPLACE: c_int = 44;
const LP64: c_int = 0;
const INTEL_THREADING: c_int = 0;

type Handle = *mut c_void;
type Layer = unsafe extern "C" fn(c_int) -> c_int;
type SetInt = unsafe extern "C" fn(c_int);
type GetInt = unsafe extern "C" fn() -> c_int;
type GetVersion = unsafe extern "C" fn(*mut c_char, c_int);
type Create = unsafe extern "C" fn(*mut Handle, c_int, c_int, c_long, ...) -> c_long;
type Set = unsafe extern "C" fn(Handle, c_int, ...) -> c_long;
type Commit = unsafe extern "C" fn(Handle) -> c_long;
type Compute = unsafe extern "C" fn(Handle, *mut c_void, ...) -> c_long;
type Free = unsafe extern "C" fn(*mut Handle) -> c_long;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AddDllDirectory(path: *const u16) -> Handle;
    fn RemoveDllDirectory(cookie: Handle) -> c_int;
    fn GetModuleHandleW(path: *const u16) -> Handle;
    fn GetModuleFileNameW(module: usize, path: *mut u16, size: u32) -> u32;
}

/// Keep the loader's controlled directory and byte pins until local references end.
struct DirectoryCookie(Handle, Context);

impl Drop for DirectoryCookie {
    fn drop(&mut self) {
        // SAFETY: the cookie is a single live AddDllDirectory result owned by self.
        if unsafe { RemoveDllDirectory(self.0) } == 0 {
            self.1.record(Err(format!(
                "close BS oneMKL directory: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
}

pub struct Frontend {
    create: Create,
    set: Set,
    commit: Commit,
    compute: Compute,
    free: Free,
    hann: [f32; FFT],
    expected: [PathBuf; COMPONENTS.len()],
    _runtime: OwnedLibrary,
    _openmp: OwnedLibrary,
    _directory: DirectoryCookie,
    _files: Vec<OwnedFile>,
    _ancestors: Vec<OwnedFile>,
}

impl Frontend {
    /// Called exclusively in the fresh CPU worker after ORT configured DLL search.
    pub fn load(root: &Path, context: &Context) -> Result<Self, String> {
        context.checkpoint("bs.mkl.verify")?;
        let directory = root.join("mkl");
        let ancestors = files::pin_directories(&directory, context)?;
        let directory = directory.canonicalize().map_err(|e| e.to_string())?;
        let expected = COMPONENTS.map(|(name, _, _)| directory.join(name));
        let mut pins = Vec::with_capacity(COMPONENTS.len());
        for ((name, size, digest), path) in COMPONENTS.into_iter().zip(&expected) {
            if !loaded(name).is_null() {
                return Err(format!("BS oneMKL DLL was loaded before admission: {name}"));
            }
            pins.push(files::verified_file(path, Some(size), digest, context)?);
        }
        context.checkpoint("bs.mkl.load")?;
        let wide = directory
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        // SAFETY: wide is a live NUL-terminated verified absolute directory path.
        let cookie = unsafe { AddDllDirectory(wide.as_ptr()) };
        if cookie.is_null() {
            return Err(format!(
                "open BS oneMKL search directory: {}",
                std::io::Error::last_os_error()
            ));
        }
        let directory = DirectoryCookie(cookie, context.clone());
        // SAFETY: these unmodified DLLs and their directory are verified and
        // pinned. The initial load uses only the package directory and System32.
        let openmp = context.library(
            unsafe { Library::load_with_flags(&expected[0], DLL_LOAD_DIR | SYSTEM32) }
                .map_err(|e| format!("load BS OpenMP: {e}"))?
                .into(),
        );
        let runtime = context.library(
            unsafe { Library::load_with_flags(&expected[1], DLL_LOAD_DIR | SYSTEM32) }
                .map_err(|e| format!("load BS oneMKL runtime: {e}"))?
                .into(),
        );

        // SAFETY: these are the documented C exports of the pinned Windows
        // oneMKL single-dynamic runtime. Local module owners outlive all calls.
        let (create, set, commit, compute, free) = unsafe {
            let interface = symbol::<Layer>(&runtime, b"MKL_Set_Interface_Layer\0")?;
            let threading = symbol::<Layer>(&runtime, b"MKL_Set_Threading_Layer\0")?;
            if interface(LP64) != LP64 || threading(INTEL_THREADING) != INTEL_THREADING {
                return Err("BS oneMKL requires LP64 and the Intel threading layer".into());
            }
            symbol::<SetInt>(&runtime, b"MKL_Set_Dynamic\0")?(0);
            symbol::<SetInt>(&runtime, b"MKL_Set_Num_Threads\0")?(1);
            if symbol::<GetInt>(&runtime, b"MKL_Get_Max_Threads\0")?() != 1 {
                return Err("BS oneMKL requires one numerical thread".into());
            }
            let mut version = [0; 512];
            symbol::<GetVersion>(&runtime, b"MKL_Get_Version_String\0")?(
                version.as_mut_ptr().cast(),
                version.len() as c_int,
            );
            let actual = CStr::from_bytes_until_nul(&version)
                .map_err(|e| e.to_string())?
                .to_str()
                .map_err(|e| e.to_string())?;
            if actual.trim() != VERSION {
                return Err(format!("BS oneMKL build differs: {actual}"));
            }
            (
                symbol::<Create>(&runtime, b"DftiCreateDescriptor\0")?,
                symbol::<Set>(&runtime, b"DftiSetValue\0")?,
                symbol::<Commit>(&runtime, b"DftiCommitDescriptor\0")?,
                symbol::<Compute>(&runtime, b"DftiComputeForward\0")?,
                symbol::<Free>(&runtime, b"DftiFreeDescriptor\0")?,
            )
        };
        // DFTI kernels can load lazily. Verify the paths already loaded now and
        // recheck after each forward below; no PE import-table claims are made.
        verify_loaded(&expected, false)?;
        let mut hann = [0.0; FFT];
        for (dst, raw) in hann.iter_mut().zip(HANN.chunks_exact(4)) {
            *dst = f32::from_le_bytes(raw.try_into().unwrap());
        }
        context.check()?;
        Ok(Self {
            create,
            set,
            commit,
            compute,
            free,
            hann,
            expected,
            _runtime: runtime,
            _openmp: openmp,
            _directory: directory,
            _files: pins,
            _ancestors: ancestors,
        })
    }

    /// Compute contiguous features in the graph's [batch, time, frequency, stereo, complex] order.
    pub fn analyze(
        &self,
        planar: &[f32],
        batch: usize,
        output: &mut [f32],
        context: &Context,
    ) -> Result<(), String> {
        context.checkpoint("bs.mkl.prepare")?;
        if !(1..=BATCH).contains(&batch)
            || planar.len() != batch * CHANNELS * CHUNK
            || output.len() != batch * STEPS * BINS * CHANNELS * 2
        {
            return Err("BS oneMKL batch geometry differs from the release".into());
        }
        let transforms = batch * CHANNELS * STEPS;
        let mut windows = files::f32_buffer(transforms * FFT, "mkl.input.windows")?;
        let mut spectrum = files::f32_buffer(transforms * BINS * 2, "mkl.output.spectrum")?;
        for (lane, rows) in planar
            .chunks_exact(CHUNK)
            .zip(windows.chunks_exact_mut(STEPS * FFT))
        {
            for (time, window) in rows.chunks_exact_mut(FFT).enumerate() {
                context.check()?;
                for (index, target) in window.iter_mut().enumerate() {
                    let position = (time * HOP + index) as isize - (FFT / 2) as isize;
                    let source = if position < 0 {
                        -position as usize
                    } else if position as usize >= CHUNK {
                        2 * CHUNK - 2 - position as usize
                    } else {
                        position as usize
                    };
                    *target = lane[source] * self.hann[index];
                }
            }
        }
        context.checkpoint("bs.mkl.fft")?;
        self.transform(transforms, &mut windows, &mut spectrum, context)?;
        verify_loaded(&self.expected, true)?;
        context.checkpoint("bs.mkl.features")?;
        for (sample, target) in output
            .chunks_exact_mut(STEPS * BINS * CHANNELS * 2)
            .enumerate()
        {
            for (time, row) in target.chunks_exact_mut(BINS * CHANNELS * 2).enumerate() {
                context.check()?;
                for channel in 0..CHANNELS {
                    let start = (((sample * CHANNELS + channel) * STEPS + time) * BINS) * 2;
                    for (pair, dst) in spectrum[start..start + BINS * 2]
                        .chunks_exact(2)
                        .zip(row.chunks_exact_mut(CHANNELS * 2))
                    {
                        dst[2 * channel..2 * channel + 2].copy_from_slice(pair);
                    }
                }
            }
        }
        Ok(())
    }

    fn transform(
        &self,
        transforms: usize,
        windows: &mut [f32],
        spectrum: &mut [f32],
        context: &Context,
    ) -> Result<(), String> {
        let mut descriptor = Descriptor {
            pointer: ptr::null_mut(),
            free: self.free,
            context,
        };
        // SAFETY: Windows LP64 DFTI uses c_long and variadic descriptor values
        // present the declared C types. Buffers are exactly the required extents.
        unsafe {
            status(
                (self.create)(
                    &mut descriptor.pointer,
                    DFTI_SINGLE,
                    DFTI_REAL,
                    1,
                    FFT as c_long,
                ),
                "create",
            )?;
            if descriptor.pointer.is_null() {
                return Err("BS oneMKL created a null descriptor".into());
            }
            let handle = descriptor.pointer;
            status(
                (self.set)(handle, DFTI_PLACEMENT, DFTI_NOT_INPLACE),
                "placement",
            )?;
            status(
                (self.set)(handle, DFTI_NUMBER_OF_TRANSFORMS, transforms as c_long),
                "transforms",
            )?;
            status(
                (self.set)(handle, DFTI_INPUT_DISTANCE, FFT as c_long),
                "input distance",
            )?;
            status(
                (self.set)(handle, DFTI_OUTPUT_DISTANCE, BINS as c_long),
                "output distance",
            )?;
            let strides: [c_long; 2] = [0, 1];
            status(
                (self.set)(handle, DFTI_INPUT_STRIDES, strides.as_ptr()),
                "input strides",
            )?;
            status(
                (self.set)(handle, DFTI_OUTPUT_STRIDES, strides.as_ptr()),
                "output strides",
            )?;
            status(
                (self.set)(handle, DFTI_CONJUGATE_EVEN_STORAGE, DFTI_COMPLEX_COMPLEX),
                "complex storage",
            )?;
            status((self.commit)(handle), "commit")?;
            context.check()?;
            status(
                (self.compute)(
                    handle,
                    windows.as_mut_ptr().cast(),
                    spectrum.as_mut_ptr().cast::<c_void>(),
                ),
                "forward",
            )?;
        }
        drop(descriptor);
        context.cleanup()?;
        context.check()
    }
}

struct Descriptor<'a> {
    pointer: Handle,
    free: Free,
    context: &'a Context,
}

impl Drop for Descriptor<'_> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: descriptor was freshly created and has no concurrent users.
            self.context
                .record(status(unsafe { (self.free)(&mut self.pointer) }, "free"));
        }
    }
}

fn status(code: c_long, phase: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("BS oneMKL DFTI {phase} failed: {code}"))
    }
}

unsafe fn symbol<T: Copy>(library: &OwnedLibrary, name: &[u8]) -> Result<T, String> {
    // SAFETY: callers specify the declared oneMKL C ABI signature for each export.
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|error| error.to_string())
}

fn loaded(name: &str) -> Handle {
    let name = name.encode_utf16().chain([0]).collect::<Vec<_>>();
    // SAFETY: NUL-terminated module basename; kernel owns the borrowed handle.
    unsafe { GetModuleHandleW(name.as_ptr()) }
}

fn verify_loaded(expected: &[PathBuf; COMPONENTS.len()], executed: bool) -> Result<(), String> {
    let mut wide = [0u16; 32768];
    for ((name, _, _), expected) in COMPONENTS.into_iter().zip(expected) {
        let handle = loaded(name);
        if handle.is_null() {
            if matches!(name, "libiomp5md.dll" | "mkl_rt.3.dll")
                || (executed && matches!(name, "mkl_core.3.dll" | "mkl_intel_thread.3.dll"))
            {
                return Err(format!(
                    "BS oneMKL did not load its required component: {name}"
                ));
            }
            continue;
        }
        // SAFETY: valid borrowed handle with this process as nominal owner.
        let count =
            unsafe { GetModuleFileNameW(handle as usize, wide.as_mut_ptr(), wide.len() as u32) }
                as usize;
        if count == 0 || count >= wide.len() {
            return Err(format!("read mapped BS oneMKL DLL path: {name}"));
        }
        use std::os::windows::ffi::OsStringExt;
        let observed = PathBuf::from(std::ffi::OsString::from_wide(&wide[..count]))
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if &observed != expected {
            return Err(format!("mapped BS oneMKL DLL escaped its package: {name}"));
        }
    }
    Ok(())
}
