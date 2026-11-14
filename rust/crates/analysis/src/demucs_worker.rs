//! Standalone HTDemucs request. The App owns the enclosing Windows Job.
// The App supplies stdout/stderr pipes. A console companion would join the
// dedicated worker Job and invalidate its single-worker admission.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "beat_worker_cuda.rs"]
mod cuda;
#[cfg(windows)]
#[path = "bs_cuda_memory.rs"]
mod cuda_memory;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "bs_worker_io.rs"]
mod files;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../examples/beat_this_audio/invocation.rs"]
mod invocation;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../examples/beat_this_audio/runtime.rs"]
mod runtime;

#[cfg(not(windows))]
fn main() -> Result<(), String> {
    Err("Native HTDemucs CPU requires a Windows x64 worker".into())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let mut invocation = invocation::Invocation::new();
    let mut cuda_failure = false;
    let result = windows::run(&mut invocation, &mut cuda_failure);
    invocation.finish_stem(
        result,
        cuda_failure,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(windows)]
mod windows {
    use super::{cuda, cuda_memory, files, invocation::Invocation, runtime};
    use flitzis_looper_analysis::{
        demucs_inference::{CpuModel, FRAMES, SOURCES, Scan64Release},
        trial_lifetime::{Context, retain_first},
    };
    use flitzis_stem_dsp::demucs::{
        RATE,
        audio::{Channels, ConversionGeometry, Converter, Options},
        host::apply::{MAX_SHIFT, Model},
        offline::Workspace,
        reduction::{CpuProfile, TORCH_GIT, TORCH_VERSION},
    };
    use sha2::{Digest, Sha256};
    use std::{
        env,
        ffi::{OsStr, c_void},
        fs::OpenOptions,
        io::{Read, Write},
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
        path::Path,
        ptr::null_mut,
        str::FromStr,
    };

    const RELEASE: &str = include_str!("../../../../workers/htdemucs/native_cpu/release.json");
    const CUDA_RELEASE: &str =
        include_str!("../../../../workers/htdemucs/native_cuda/release.json");
    const ORT_MANIFEST: &str = "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d";
    const ORT_PACKAGE: &str = "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319";
    const BLOCK_FRAMES: usize = 4096;
    const PCM_BLOCK_BYTES: usize = BLOCK_FRAMES * 2 * size_of::<f32>();
    const JOB_KILL_ON_CLOSE: u32 = 0x2000;
    const JOB_EXTENDED_LIMITS: i32 = 9;
    const JOB_BASIC_ACCOUNTING: i32 = 1;

    #[repr(C)]
    #[derive(Default)]
    struct JobBasicLimits {
        process_user_time: i64,
        job_user_time: i64,
        flags: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct JobLimits {
        basic: JobBasicLimits,
        io: [u64; 6],
        process_limit: usize,
        job_limit: usize,
        peak_process: usize,
        peak_job: usize,
    }

    #[repr(C)]
    #[derive(Default)]
    struct JobAccounting {
        times: [i64; 4],
        faults: u32,
        processes: u32,
        active: u32,
        terminated: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryInformationJobObject(
            job: *mut c_void,
            class: i32,
            data: *mut c_void,
            size: u32,
            returned: *mut u32,
        ) -> i32;
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            available: *mut u64,
            capacity: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }

    fn check_owned_job() -> Result<(), String> {
        let mut limits = JobLimits::default();
        let mut accounting = JobAccounting::default();
        // SAFETY: null means the current process's Job; each output buffer has
        // the correct documented layout and exact size for the information class.
        let (class, error) = unsafe {
            if QueryInformationJobObject(
                null_mut(),
                JOB_EXTENDED_LIMITS,
                (&raw mut limits).cast(),
                size_of::<JobLimits>() as u32,
                null_mut(),
            ) == 0
            {
                (JOB_EXTENDED_LIMITS, Some(std::io::Error::last_os_error()))
            } else if QueryInformationJobObject(
                null_mut(),
                JOB_BASIC_ACCOUNTING,
                (&raw mut accounting).cast(),
                size_of::<JobAccounting>() as u32,
                null_mut(),
            ) == 0
            {
                (JOB_BASIC_ACCOUNTING, Some(std::io::Error::last_os_error()))
            } else {
                (0, None)
            }
        };
        if let Some(error) = error {
            return Err(format!("HTDemucs Windows Job query class {class}: {error}"));
        }
        if limits.basic.flags & JOB_KILL_ON_CLOSE == 0 || accounting.active != 1 {
            return Err(format!(
                "HTDemucs must run alone in its admitted Windows Job (flags {:#x}, active {})",
                limits.basic.flags, accounting.active
            ));
        }
        Ok(())
    }

    fn check_output_disk(output: &Path, bytes: usize) -> Result<(), String> {
        let directory: Vec<u16> = output.as_os_str().encode_wide().chain([0]).collect();
        let mut available = 0;
        // SAFETY: directory is NUL-terminated and available is a writable u64.
        if unsafe {
            GetDiskFreeSpaceExW(
                directory.as_ptr(),
                &raw mut available,
                null_mut(),
                null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "HTDemucs output disk query: {}",
                std::io::Error::last_os_error()
            ));
        }
        if available < bytes as u64 {
            return Err(format!(
                "HTDemucs output needs {bytes} disk bytes; {available} available"
            ));
        }
        Ok(())
    }

    fn argument<T: FromStr>(value: &OsStr, name: &str) -> Result<T, String> {
        value
            .to_str()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| format!("invalid HTDemucs {name}"))
    }

    fn buffer(samples: usize) -> Result<Vec<f32>, String> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(samples)
            .map_err(|e| e.to_string())?;
        values.resize(samples, 0.0);
        Ok(values)
    }

    fn read_pcm(
        file: &mut impl Read,
        planar: &mut [f32],
        channels: usize,
        frames: usize,
        context: &Context,
    ) -> Result<String, String> {
        context.checkpoint("demucs.source.read")?;
        let mut hash = Sha256::new();
        let mut block = [0u8; PCM_BLOCK_BYTES];
        let block_frames = block.len() / (channels * size_of::<f32>());
        for start in (0..frames).step_by(block_frames) {
            context.check()?;
            let length = block_frames.min(frames - start);
            let bytes = &mut block[..length * channels * 4];
            file.read_exact(bytes).map_err(|e| e.to_string())?;
            hash.update(&*bytes);
            for (index, raw) in bytes.chunks_exact(4).enumerate() {
                let value = f32::from_le_bytes(raw.try_into().unwrap());
                if !value.is_finite() {
                    return Err("nonfinite HTDemucs source PCM".into());
                }
                planar[index % channels * frames + start + index / channels] = value;
            }
        }
        if file.read(&mut block[..1]).map_err(|e| e.to_string())? != 0 {
            return Err("unexpected trailing HTDemucs source PCM".into());
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    fn qualify_startup<T>(
        result: Result<T, String>,
        fault: Option<cuda::StartupError>,
        context: &Context,
        cuda_failure: &mut bool,
    ) -> Result<T, String> {
        if result.is_err() && fault.is_some() {
            context.checkpoint("demucs.cuda.startup_failure")?;
            context.cleanup()?;
            *cuda_failure = true;
        }
        result
    }

    fn write_pcm(
        invocation: &mut Invocation,
        path: &Path,
        left: &[f32],
        right: &[f32],
    ) -> Result<String, String> {
        let mut hash = Sha256::new();
        invocation.write(path, |file, context| {
            let mut block = [0u8; BLOCK_FRAMES * 2 * 4];
            for (left, right) in left.chunks(BLOCK_FRAMES).zip(right.chunks(BLOCK_FRAMES)) {
                context.check()?;
                let bytes = &mut block[..left.len() * 8];
                for ((&left, &right), dst) in left.iter().zip(right).zip(bytes.chunks_exact_mut(8))
                {
                    if !left.is_finite() || !right.is_finite() {
                        return Err("nonfinite HTDemucs output PCM".into());
                    }
                    dst[..4].copy_from_slice(&left.clamp(-0.99, 0.99).to_le_bytes());
                    dst[4..].copy_from_slice(&right.clamp(-0.99, 0.99).to_le_bytes());
                }
                hash.update(&*bytes);
                file.write_all(bytes).map_err(|e| e.to_string())?;
            }
            Ok(())
        })?;
        Ok(format!("{:x}", hash.finalize()))
    }

    struct PreparedTrack {
        rate: u32,
        channels: usize,
        frames: usize,
        overlap: f64,
        offsets: Vec<usize>,
        digest: String,
        mixture: Vec<f32>,
        output_frames: usize,
        output_len: usize,
        cpu_capability: &'static str,
        profile: CpuProfile,
    }

    fn prepare_track(
        args: &[std::ffi::OsString],
        output: &Path,
        context: &Context,
    ) -> Result<PreparedTrack, String> {
        let [pcm, rate, channels, frames, pcm_sha, overlap, offsets, _] = args else {
            return Err("expected PCM RATE CHANNELS FRAMES PCM_SHA OVERLAP OFFSETS OUTPUT".into());
        };
        let pcm = Path::new(pcm);
        if !pcm.is_absolute() {
            return Err("HTDemucs requires absolute owned input and scratch paths".into());
        }
        let rate: u32 = argument(rate, "source rate")?;
        let channels: usize = argument(channels, "source channels")?;
        let frames: usize = argument(frames, "source frames")?;
        let overlap: f64 = argument(overlap, "overlap")?;
        let offsets: Vec<usize> = serde_json::from_str(
            offsets
                .to_str()
                .filter(|text| text.len() <= 256)
                .ok_or("invalid shift offsets")?,
        )
        .map_err(|e| e.to_string())?;
        if !(1..=PCM_BLOCK_BYTES / size_of::<f32>()).contains(&channels)
            || offsets.len() > 20
            || offsets.iter().any(|&value| value > MAX_SHIFT)
        {
            return Err("unsupported HTDemucs input channels or shift offset".into());
        }
        let cpu_capability = CpuProfile::host_capability();
        let profile = CpuProfile::admit(TORCH_VERSION, TORCH_GIT, cpu_capability, 1)
            .ok_or("HTDemucs CPU reduction profile is unavailable")?;
        let cancelled = || context.cancelled();
        let conversion = ConversionGeometry::new(
            Channels::new(frames, 1, channels, 2).map_err(|e| e.to_string())?,
            rate,
            RATE,
            Options::default(),
        )
        .map_err(|e| e.to_string())?;
        let model = Model {
            sources: &SOURCES,
            channels: 2,
            samplerate: RATE,
            segment: FRAMES,
            overlap,
            transition_power: 1.0,
            valid_frames: FRAMES,
            training_frames: FRAMES,
            split: true,
            offsets: &offsets,
            weights: &[1.0; SOURCES.len()],
        };
        let output_frames = conversion.output_frames();
        let output_len = conversion
            .output_len()
            .checked_mul(SOURCES.len())
            .ok_or("HTDemucs output extent overflow")?;
        let output_bytes = output_len
            .checked_mul(size_of::<f32>())
            .ok_or("HTDemucs output byte extent overflow")?;
        Workspace::planned_payload_bytes(
            output_frames,
            std::slice::from_ref(&model),
            0,
            &cancelled,
        )
        .map_err(|e| e.to_string())?;
        check_output_disk(output, output_bytes)?;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(pcm)
            .map_err(|e| e.to_string())?;
        let mut file = context.file(file, "demucs.source.close");
        let pcm_extent = conversion
            .input_len()
            .checked_mul(size_of::<f32>())
            .ok_or("HTDemucs source PCM byte extent overflow")?;
        let info = file.metadata().map_err(|e| e.to_string())?;
        if !info.is_file() || info.len() != pcm_extent as u64 {
            return Err("HTDemucs source PCM extent differs".into());
        }
        let mut input = buffer(conversion.input_len())?;
        let mut digest = read_pcm(&mut *file, &mut input, channels, frames, context);
        retain_first(&mut digest, file.complete());
        let digest = digest?;
        if Some(digest.as_str()) != pcm_sha.to_str() {
            return Err("HTDemucs staged source PCM digest differs".into());
        }
        let mut mixture = buffer(conversion.output_len())?;
        Converter::new(conversion, &cancelled)
            .and_then(|mut converter| converter.convert_into(&input, &mut mixture, &cancelled))
            .map_err(|e| e.to_string())?;
        drop(input);
        Ok(PreparedTrack {
            rate,
            channels,
            frames,
            overlap,
            offsets,
            digest,
            mixture,
            output_frames,
            output_len,
            cpu_capability,
            profile,
        })
    }

    pub fn run(own: &mut Invocation, cuda_failure: &mut bool) -> Result<serde_json::Value, String> {
        let context = own.context.clone();
        context.checkpoint("demucs.startup")?;
        if !context.bound() || env::consts::ARCH != "x86_64" || ort::MINOR_VERSION != 24 {
            return Err("HTDemucs requires Windows x64, its cancel marker and ORT API 24".into());
        }
        check_owned_job()?;
        let args: Vec<_> = env::args_os().skip(1).collect();
        let (is_cuda, args) = match args.as_slice() {
            [flag, rest @ ..] if flag == "--cuda" => (true, rest),
            args => (false, args),
        };
        let (graph_name, release_bytes) = if is_cuda {
            ("htdemucs-cuda.onnx", CUDA_RELEASE)
        } else {
            ("htdemucs.onnx", RELEASE)
        };
        let release = serde_json::from_str::<Option<Scan64Release>>(release_bytes)
            .map_err(|e| e.to_string())?
            .ok_or("HTDemucs learned Scan64 release is unavailable")?;
        let output = Path::new(args.last().ok_or("missing HTDemucs worker scratch")?);
        if !output.is_absolute() || !output.is_dir() {
            return Err("HTDemucs requires an absolute owned scratch directory".into());
        }
        let _output_directories = files::pin_directories(output, &context)?;
        let installation = env::current_exe().map_err(|e| e.to_string())?;
        let root = installation
            .parent()
            .ok_or("missing HTDemucs installation")?;
        let _package_directories = files::pin_directories(root, &context)?;
        let (directory, manifest_sha, package_sha) = if is_cuda {
            ("cuda", cuda::ORT_MANIFEST_SHA, cuda::ORT_PACKAGE_SHA)
        } else {
            ("ort", ORT_MANIFEST, ORT_PACKAGE)
        };
        let ort_dir = root.join(directory);
        let _runtime_directory = files::pin_directories(&ort_dir, &context)?;
        let package = ort_dir.join("step23-package.json");
        let manifest = ort_dir.join("step23-runtime.json");
        let package_request = env::var_os(runtime::PACKAGE_ENV)
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("missing absolute HTDemucs ORT package path")?;
        let package = package.canonicalize().map_err(|e| e.to_string())?;
        if package_request.canonicalize().map_err(|e| e.to_string())? != package
            || env::var(runtime::PACKAGE_DIGEST_ENV).as_deref() != Ok(package_sha)
            || env::var(runtime::LOAD_POLICY_ENV).as_deref() != Ok("package-system32")
        {
            return Err("HTDemucs requires its pinned ORT package".into());
        }
        let is_load = !is_cuda && matches!(args, [mode, _] if mode == "load");
        let track = (!is_load)
            .then(|| prepare_track(args, output, &context))
            .transpose()?;
        let _graph = files::verified_file(
            &root.join(graph_name),
            Some(release.graph_bytes),
            &release.graph_sha256,
            &context,
        )?;
        let _crt = runtime::load_worker_crt(root, &context)?;
        let (runtime, environment) = runtime::configure(Some((&manifest, manifest_sha)), &context)?;
        let admission = runtime.ok_or("missing HTDemucs ORT admission")?;
        runtime::commit(environment)?;
        let mut startup_error = None;
        let device = is_cuda
            .then(|| cuda::device(&context, &mut startup_error))
            .transpose();
        let device = qualify_startup(device, startup_error, &context, cuda_failure)?;
        if let Some(device) = &device {
            device.verified_uuid()?;
        }
        let memory = device
            .as_ref()
            .and_then(cuda::Device::luid_and_node_mask)
            .and_then(|(luid, node_mask)| cuda_memory::Measurement::start(luid, node_mask));
        let profile_path = is_cuda
            .then(|| own.profile(&output.join("ort-profile")))
            .transpose()?;
        let mut startup_error = None;
        let core = CpuModel::load_scan64(
            &root.join(graph_name),
            &release,
            &context,
            profile_path.as_deref(),
            || Ok(()),
            |stage, code, message| {
                if is_cuda {
                    startup_error = cuda::model_startup_error(stage, code, message);
                }
            },
        );
        let mut core = qualify_startup(core, startup_error, &context, cuda_failure)?;

        if is_load {
            let runtime = admission.receipt()?;
            drop(core);
            return Ok(serde_json::json!({
                "schema_version": 3, "mode": "load", "device": "cpu",
                "graph_sha256": release.graph_sha256,
                "trace_sha256": release.trace_sha256,
                "approval_sha256": format!("{:x}", Sha256::digest(release_bytes.as_bytes())),
                "job": "windows-owned", "runtime": runtime,
            }));
        }

        let PreparedTrack {
            rate,
            channels,
            frames,
            overlap,
            offsets,
            digest,
            mixture,
            output_frames,
            output_len,
            cpu_capability,
            profile,
        } = track.ok_or("missing validated HTDemucs track")?;
        let model = Model {
            sources: &SOURCES,
            channels: 2,
            samplerate: RATE,
            segment: FRAMES,
            overlap,
            transition_power: 1.0,
            valid_frames: FRAMES,
            training_frames: FRAMES,
            split: true,
            offsets: &offsets,
            weights: &[1.0; SOURCES.len()],
        };
        let mut output_pcm = buffer(output_len)?;
        let mut result = core.run_offline(&mixture, &mut output_pcm, &model, profile, true);
        let device_error = core
            .run_failure()
            .and_then(|(code, message)| cuda::device_error(code, message));
        let group_norm_device_error = core.group_norm_device_failure();
        let profile_result = profile_path
            .as_ref()
            .and_then(|_| own.finish_profile_with(&mut result, || core.finish_profile()));
        let memory = memory.and_then(cuda_memory::Measurement::finish);
        drop(core);
        retain_first(&mut result, context.cleanup());
        drop(mixture);
        if result.is_err() && (device_error.is_some() || group_norm_device_error.is_some()) {
            context.checkpoint("demucs.cuda.failure")?;
            context.cleanup()?;
            let receipt = admission.receipt()?;
            cuda::runtime_evidence(
                device.as_ref().ok_or("missing HTDemucs CUDA device")?,
                &receipt,
            )?;
            *cuda_failure = true;
        }
        result?;
        let runtime = admission.receipt()?;
        let (cuda, kernel_events) = if let Some(device) = &device {
            let mut evidence = cuda::runtime_evidence(device, &runtime)?;
            evidence["device_uuid"] = device.verified_uuid()?.into();
            let prefix = profile_path
                .as_ref()
                .ok_or("missing HTDemucs CUDA profile")?;
            let count = files::kernel_events(prefix, profile_result.as_deref(), &context)?;
            (evidence, count)
        } else {
            (serde_json::Value::Null, 0)
        };
        let mut outputs = serde_json::Map::new();
        for (name, pcm) in SOURCES
            .iter()
            .zip(output_pcm.chunks_exact(output_frames * 2))
        {
            let (left, right) = pcm.split_at(output_frames);
            let digest = write_pcm(own, &output.join(format!("{name}.f32")), left, right)?;
            outputs.insert((*name).into(), digest.into());
        }
        let provider = if is_cuda {
            "CUDAExecutionProvider"
        } else {
            "CPUExecutionProvider"
        };
        Ok(serde_json::json!({
            "schema_version": 3,
            "device": if is_cuda { "cuda" } else { "cpu" },
            "provider": provider, "cuda": cuda, "cuda_kernel_events": kernel_events, "cuda_memory": memory,
            "cpu_capability": cpu_capability,
            "graph_sha256": release.graph_sha256,
            "trace_sha256": release.trace_sha256, "pcm_sha256": digest,
            "approval_sha256": format!("{:x}", Sha256::digest(release_bytes.as_bytes())),
            "source": {"rate": rate, "channels": channels, "frames": frames},
            "frames": output_frames, "overlap": overlap, "offsets": offsets,
            "job": "windows-owned", "outputs": outputs, "runtime": runtime,
        }))
    }
}
