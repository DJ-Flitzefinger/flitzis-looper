//! Standalone BS-RoFormer release worker for Windows x64.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "beat_worker_cuda.rs"]
mod cuda;
#[cfg(windows)]
#[path = "bs_cuda_memory.rs"]
mod cuda_memory;
#[cfg(windows)]
#[path = "bs_worker_io.rs"]
mod files;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../examples/beat_this_audio/invocation.rs"]
mod invocation;
#[cfg(windows)]
#[path = "bs_mkl_frontend.rs"]
mod mkl;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../examples/beat_this_audio/runtime.rs"]
mod runtime;

#[cfg(not(windows))]
fn main() -> Result<(), String> {
    Err("native BS-RoFormer requires a Windows x64 package".into())
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let mut own = invocation::Invocation::new();
    let mut device_failure = false;
    let result = windows::run(&mut own, &mut device_failure);
    own.finish_stem(
        result,
        device_failure,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

#[cfg(windows)]
mod windows {
    use super::{cuda, cuda_memory, files, invocation::Invocation, mkl, runtime};
    use flitzis_looper_analysis::{
        bs_inference::{FEATURES, MASKS, MaskModel},
        trial_lifetime::{Context, retain_first},
    };
    use flitzis_stem_dsp::{
        CHANNELS, Error, Geometry, InverseEndpoints, RATE, STEMS, SpectralWorkspace, Spectrum,
        track::{BATCH, CHUNK, TrackPlan, process_track},
    };
    use serde_json::{Value, json};
    use std::{
        env,
        ffi::OsStr,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        ptr::null_mut,
    };

    const GRAPH_NAME: &str = "bs-mask.onnx";
    const CUDA_GRAPH_NAME: &str = "bs-mask-amp.onnx";
    const GRAPH_BYTES: usize = 516_510;
    const CUDA_GRAPH_BYTES: usize = 841_582;
    const GRAPH_SHA: &str = "1bbb2bfd27e4cb71b6fce2c1cb455afca44b98335ab8de38540e0f750d4a15c9";
    const CUDA_GRAPH_SHA: &str = "19a9be3bc2e836d06918c7d9877f4def4f8822de3fb69134b0695ebd7afef3d5";
    const WEIGHTS_NAME: &str = "weights.bsnn";
    const WEIGHTS_BYTES: usize = 526_818_456;
    const WEIGHTS_SHA: &str = "51619069b73bfcbdc444a16a4ebc8cf53ea8d3466fce67b594412a02b854cbdf";
    const CPU_MANIFEST: &str = "4929eb4f6a3aebe2c117850a2e1354c48bc004bfa5ff4d805c2ce0ecbef2915d";
    const CPU_PACKAGE: &str = "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319";
    const CPU_FFT_PROFILE: &str = "onemkl-2026.0-20260401-intel-lp64-stft";
    const CUDA_FFT_PROFILE: &str = "rustfft-bs-cuda-amp-endpoints-v1";

    #[derive(Clone, Copy)]
    enum Mode<'a> {
        Load,
        Mask {
            source: &'a Path,
            batch: usize,
            sha: &'a str,
        },
        Track {
            source: &'a Path,
            plan: TrackPlan,
            sha: &'a str,
            raw: bool,
        },
    }

    struct Request<'a> {
        cuda: bool,
        mode: Mode<'a>,
        output: &'a Path,
        graph_sha: &'a str,
    }

    impl<'a> Request<'a> {
        fn parse(args: &'a [std::ffi::OsString]) -> Result<Self, String> {
            let (cuda, args) = match args {
                [mode, rest @ ..] if mode == "--cuda" => (true, rest),
                _ => (false, args),
            };
            let (mode, output, graph) = match args {
                [mode, output, graph] if mode == "load" && !cuda => (Mode::Load, output, graph),
                [mode, source, count, digest, output, graph] => {
                    let source = absolute(source)?;
                    let count = count.to_str().and_then(|n| n.parse::<usize>().ok())
                        .ok_or("invalid BS frame or batch count")?;
                    let sha = files::digest_arg(digest.to_str().unwrap_or(""))?;
                    let mode = match mode.to_str() {
                        Some("mask") if (1..=BATCH).contains(&count) => {
                            Mode::Mask { source, batch: count, sha }
                        }
                        Some("track" | "track-raw") => Mode::Track {
                            source, plan: TrackPlan::new(count, RATE).map_err(|e| e.to_string())?,
                            sha, raw: mode == "track-raw",
                        },
                        _ => return Err("invalid BS worker mode or batch count".into()),
                    };
                    (mode, output, graph)
                }
                _ => return Err("expected [--cuda] track|track-raw|mask INPUT COUNT INPUT_SHA256 OUTPUT_DIR GRAPH_SHA256, or load OUTPUT_DIR GRAPH_SHA256".into()),
            };
            Ok(Self {
                cuda,
                mode,
                output: absolute(output)?,
                graph_sha: files::digest_arg(graph.to_str().unwrap_or(""))?,
            })
        }

        fn disk_bytes(&self) -> Result<u64, String> {
            match self.mode {
                Mode::Load => Ok(0),
                Mode::Mask { batch, .. } => Ok((batch * MASKS * 4) as u64),
                Mode::Track { plan, raw, .. } => {
                    files::TrackOutputs::required_bytes(plan.source_frames(), raw)
                }
            }
        }
    }

    fn absolute(value: &OsStr) -> Result<&Path, String> {
        let path = Path::new(value);
        if path.is_absolute() && !path.as_os_str().encode_wide().any(|unit| unit == 0) {
            Ok(path)
        } else {
            Err("BS worker paths must be absolute".into())
        }
    }

    pub fn run(own: &mut Invocation, device_failure: &mut bool) -> Result<Value, String> {
        let context = own.context.clone();
        context.checkpoint("bs.worker.startup")?;
        if !context.bound() || env::consts::ARCH != "x86_64" || ort::MINOR_VERSION != 24 {
            return Err("BS worker requires its own cancellation marker and ORT API 24".into());
        }
        let args: Vec<_> = env::args_os().skip(1).collect();
        let request = Request::parse(&args)?;
        let _directory = files::pin_directories(request.output, &context)?;
        check_disk(request.output, request.disk_bytes()?)?;
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        let root = executable
            .parent()
            .ok_or("missing BS worker installation")?;
        let _package_directories = files::pin_directories(root, &context)?;
        let (graph_name, graph_bytes, graph_sha) = if request.cuda {
            (CUDA_GRAPH_NAME, CUDA_GRAPH_BYTES, CUDA_GRAPH_SHA)
        } else {
            (GRAPH_NAME, GRAPH_BYTES, GRAPH_SHA)
        };
        if request.graph_sha != graph_sha {
            return Err("BS request graph SHA-256 differs from its release pin".into());
        }
        let graph_path = root.join(graph_name);
        let _graph = files::verified_file(&graph_path, Some(graph_bytes), graph_sha, &context)?;
        let _weights = files::verified_file(
            &root.join(WEIGHTS_NAME),
            Some(WEIGHTS_BYTES),
            WEIGHTS_SHA,
            &context,
        )?;
        let (directory, manifest_sha, package_sha) = if request.cuda {
            ("cuda", cuda::ORT_MANIFEST_SHA, cuda::ORT_PACKAGE_SHA)
        } else {
            ("ort", CPU_MANIFEST, CPU_PACKAGE)
        };
        let ort_directory = root.join(directory);
        let _runtime_directory = files::pin_directories(&ort_directory, &context)?;
        let manifest = ort_directory.join("step23-runtime.json");
        let package = ort_directory.join("step23-package.json");
        if env::var_os(runtime::PACKAGE_ENV).as_deref() != Some(package.as_os_str())
            || env::var(runtime::PACKAGE_DIGEST_ENV).as_deref() != Ok(package_sha)
            || env::var(runtime::LOAD_POLICY_ENV).as_deref() != Ok("package-system32")
        {
            return Err("BS worker requires its pinned ORT package".into());
        }
        let _crt = runtime::load_worker_crt(root, &context)?;
        let (admission, environment) =
            runtime::configure(Some((&manifest, manifest_sha)), &context)?;
        let admission = admission.ok_or("missing pinned BS ORT admission")?;
        runtime::commit(environment)?;
        let frontend = (!request.cuda && !matches!(request.mode, Mode::Mask { .. }))
            .then(|| mkl::Frontend::load(root, &context))
            .transpose()?;
        let mut startup_error = None;
        let device = request
            .cuda
            .then(|| cuda::device(&context, &mut startup_error))
            .transpose();
        let device = qualify_startup(device, startup_error, &context, device_failure)?;
        if let Some(device) = &device {
            device.verified_uuid()?;
        }
        let memory = device
            .as_ref()
            .and_then(cuda::Device::luid_and_node_mask)
            .and_then(|(luid, node_mask)| cuda_memory::Measurement::start(luid, node_mask));
        let profile = request
            .cuda
            .then(|| own.profile(&request.output.join("ort-profile")))
            .transpose()?;
        let mut startup_error = None;
        let stage_entered = std::time::Instant::now();
        let model = MaskModel::load(
            &graph_path,
            profile.as_deref(),
            request.cuda,
            &context,
            |stage, code, message| {
                if request.cuda {
                    startup_error = cuda::model_startup_error(stage, code, message);
                }
            },
        );
        let model = qualify_startup(model, startup_error, &context, device_failure)?;
        report_phase("model_load", stage_entered);
        let mut prediction = Prediction {
            model,
            frontend,
            profile,
            profile_full_track: env::var_os("FLITZIS_BS_ONNX_WARMUP").as_deref()
                == Some(OsStr::new("1")),
            kernels: 0,
            cuda_error: None,
            cuda: request.cuda,
        };
        let mut result = run_request(own, &request, &mut prediction);
        prediction.finish_profile(own, &mut result);
        let memory = memory.and_then(cuda_memory::Measurement::finish);
        let arena = if request.cuda
            && result.is_ok()
            && prediction.kernels > 0
            && matches!(request.mode, Mode::Track { .. })
        {
            prediction.model.cuda_arena_stats()
        } else {
            None
        };
        let cuda_error = prediction.cuda_error;
        let kernel_events = prediction.kernels;
        drop(prediction);
        if result.is_err() && cuda_error.is_some() {
            context.checkpoint("bs.cuda.failure")?;
            context.cleanup()?;
            // Re-observe the actually loaded package before claiming a provider fault.
            let receipt = admission.receipt()?;
            cuda::runtime_evidence(device.as_ref().ok_or("missing BS CUDA device")?, &receipt)?;
            *device_failure = true;
        }
        let mut result = result?;
        let receipt = admission.receipt()?;
        let cuda = if let Some(device) = &device {
            let mut evidence = cuda::runtime_evidence(device, &receipt)?;
            evidence["device_uuid"] = device.verified_uuid()?.into();
            if kernel_events == 0 {
                return Err("no actual BS CUDA compute was observed".into());
            }
            evidence
        } else {
            Value::Null
        };
        result["schema_version"] = 1.into();
        result["graph_sha256"] = request.graph_sha.into();
        result["weights_sha256"] = WEIGHTS_SHA.into();
        result["ort_version"] = receipt["runtime_version"].clone();
        result["ort_api"] = receipt["ort_api"].clone();
        result["ort_package_sha256"] = package_sha.into();
        result["device"] = if request.cuda { "cuda" } else { "cpu" }.into();
        result["provider"] = if request.cuda {
            "CUDAExecutionProvider"
        } else {
            "CPUExecutionProvider"
        }
        .into();
        result["cuda_kernel_events"] = kernel_events.into();
        result["cuda"] = cuda;
        if let Some(observation) = memory {
            result["cuda_memory"] = json!(observation);
        }
        if let Some(observation) = arena {
            result["cuda_arena"] = json!(observation);
        }
        if !matches!(request.mode, Mode::Mask { .. }) {
            result["fft_profile"] = if request.cuda {
                CUDA_FFT_PROFILE
            } else {
                CPU_FFT_PROFILE
            }
            .into();
        }
        Ok(result)
    }

    fn report_phase(name: &str, started: std::time::Instant) {
        use std::io::Write;
        if env::var_os("FLITZIS_BS_MODEL_TIMING").as_deref() != Some(OsStr::new("1")) {
            return;
        }
        let _ = writeln!(
            std::io::stderr().lock(),
            "bs_roformer_phase {name} {:.6}",
            started.elapsed().as_secs_f64()
        );
    }

    fn qualify_startup<T>(
        result: Result<T, String>,
        fault: Option<cuda::StartupError>,
        context: &Context,
        device_failure: &mut bool,
    ) -> Result<T, String> {
        if result.is_err() && fault.is_some() {
            context.checkpoint("bs.cuda.startup_failure")?;
            context.cleanup()?;
            *device_failure = true;
        }
        result
    }

    fn run_request(
        own: &mut Invocation,
        request: &Request<'_>,
        prediction: &mut Prediction,
    ) -> Result<Value, String> {
        let context = own.context.clone();
        match request.mode {
            Mode::Load => Ok(json!({ "mode": "load", "inference_executed": false })),
            Mode::Mask { source, batch, sha } => {
                let _source_directories = files::pin_directories(
                    source.parent().ok_or("missing BS input directory")?,
                    &context,
                )?;
                let mut file =
                    files::verified_file(source, Some(batch * FEATURES * 4), sha, &context)?;
                let mut features = files::f32_buffer(batch * FEATURES, "mask.features")?;
                let mut read = files::read_f32(&mut file, &mut features, &context);
                retain_first(&mut read, file.complete());
                read?;
                let path = request.output.join("masks.f32");
                let result = prediction.run_masks(batch, &features, &context, |masks| {
                    own.write_floats(&path, masks)
                });
                result.map(|()| json!({ "mode": "mask", "batch": batch, "features_sha256": sha }))
            }
            Mode::Track {
                source,
                plan,
                sha,
                raw,
            } => {
                let _source_directories = files::pin_directories(
                    source.parent().ok_or("missing BS input directory")?,
                    &context,
                )?;
                let frames = plan.source_frames();
                let mut file =
                    files::verified_file(source, Some(frames * CHANNELS * 4), sha, &context)?;
                let mut output = files::TrackOutputs::new(
                    |name| own.create_readable_output(&request.output.join(name)),
                    frames,
                    raw,
                )?;
                let mut spectral = SpectralWorkspace::new(CHUNK, RATE, &|| context.cancelled())
                    .map_err(|e| dsp_error(e, &context))?;
                let mut io_error = None;
                let mut model_error = None;
                let mut output_error = None;
                let track = process_track(
                    plan,
                    |start, count, planar| {
                        files::read_planar(&mut file, start, count, planar, &context).map_err(
                            |error| {
                                io_error = Some(error);
                                Error::Source
                            },
                        )
                    },
                    |planar, batch| {
                        prediction
                            .track_batch(own, &mut spectral, planar, batch, &context)
                            .map_err(|error| {
                                model_error = Some(error);
                                Error::Prediction
                            })
                    },
                    |start, count, planar| {
                        output
                            .write(start, count, planar, &context)
                            .map_err(|error| {
                                output_error = Some(error);
                                Error::Output
                            })
                    },
                    &|| context.cancelled(),
                );
                let mut result = track.map_err(|error| {
                    io_error
                        .or(model_error)
                        .or(output_error)
                        .unwrap_or_else(|| dsp_error(error, &context))
                });
                retain_first(&mut result, file.complete());
                result?;
                let (outputs, raw_outputs) = output.finish(&context)?;
                let mut receipt = json!({
                    "mode": if raw { "track-raw" } else { "track" },
                    "frames": frames,
                    "sample_rate_hz": RATE,
                    "channels": CHANNELS,
                    "pcm_sha256": sha,
                    "outputs": outputs,
                });
                if raw {
                    receipt["raw_outputs"] = raw_outputs;
                }
                Ok(receipt)
            }
        }
    }

    struct Prediction {
        model: MaskModel,
        frontend: Option<mkl::Frontend>,
        profile: Option<PathBuf>,
        profile_full_track: bool,
        kernels: usize,
        cuda_error: Option<u32>,
        cuda: bool,
    }

    impl Prediction {
        fn run_masks<T>(
            &mut self,
            batch: usize,
            features: &[f32],
            context: &Context,
            consume: impl FnOnce(&[f32]) -> Result<T, String>,
        ) -> Result<T, String> {
            let on_cuda = self.cuda;
            let fault = &mut self.cuda_error;
            self.model.run(
                batch,
                features,
                context,
                |code, message| {
                    if on_cuda {
                        *fault = cuda::device_error(code, message);
                    }
                },
                consume,
            )
        }

        fn finish_profile<T>(&mut self, own: &Invocation, result: &mut Result<T, String>) {
            if let Some(prefix) = self.profile.take() {
                let path = own.finish_profile_with(result, || self.model.finish_profile());
                if result.is_ok() {
                    retain_first(
                        result,
                        files::kernel_events(&prefix, path.as_deref(), &own.context)
                            .map(|count| self.kernels = count),
                    );
                }
            }
        }

        fn track_batch(
            &mut self,
            own: &Invocation,
            spectral: &mut SpectralWorkspace,
            planar: &[f32],
            batch: usize,
            context: &Context,
        ) -> Result<Vec<f32>, String> {
            let features_entered = std::time::Instant::now();
            context.checkpoint("bs.track.features")?;
            let mut spectra: [Option<Spectrum>; BATCH] = Default::default();
            let mut features = files::f32_buffer(batch * FEATURES, "batch.features")?;
            if let Some(frontend) = &self.frontend {
                frontend.analyze(planar, batch, &mut features, context)?;
                let geometry = Geometry::new(CHUNK, RATE).map_err(|e| dsp_error(e, context))?;
                for (slot, values) in spectra.iter_mut().zip(features.chunks_exact(FEATURES)) {
                    let mut stored = files::f32_buffer(FEATURES, "batch.spectrum")?;
                    stored.copy_from_slice(values);
                    *slot = Some(
                        Spectrum::from_features(geometry, stored, &|| context.cancelled())
                            .map_err(|e| dsp_error(e, context))?,
                    );
                }
            } else {
                for (index, chunk) in planar.chunks_exact(CHANNELS * CHUNK).enumerate() {
                    let spectrum = spectral
                        .analyze(chunk, RATE, &|| context.cancelled())
                        .map_err(|e| dsp_error(e, context))?;
                    features[index * FEATURES..(index + 1) * FEATURES]
                        .copy_from_slice(spectrum.features());
                    spectra[index] = Some(spectrum);
                }
            }
            report_phase("features", features_entered);
            let mut rows = files::f32_buffer(batch * STEMS * CHANNELS * CHUNK, "batch.prediction")?;
            let inference_entered = std::time::Instant::now();
            let endpoints = if self.cuda {
                InverseEndpoints::BsCudaAmpReference
            } else {
                InverseEndpoints::Standard
            };
            let mut result = self.run_masks(batch, &features, context, |masks| {
                report_phase("inference", inference_entered);
                let inverse_entered = std::time::Instant::now();
                context.checkpoint("bs.track.reconstruct")?;
                for (index, masks) in masks.chunks_exact(MASKS).enumerate() {
                    let spectrum = spectra[index].as_ref().ok_or("missing BS spectrum")?;
                    let mut inverse = spectral
                        .reconstruction(spectrum, &|| context.cancelled())
                        .map_err(|e| dsp_error(e, context))?;
                    for (stem, mask) in masks.chunks_exact(FEATURES).enumerate() {
                        let offset = (index * STEMS + stem) * CHANNELS * CHUNK;
                        inverse
                            .write_stem_with_endpoints(
                                mask,
                                &mut rows[offset..offset + CHANNELS * CHUNK],
                                endpoints,
                                &|| context.cancelled(),
                            )
                            .map_err(|e| dsp_error(e, context))?;
                    }
                }
                report_phase("reconstruction", inverse_entered);
                Ok(())
            });
            if !self.profile_full_track {
                self.finish_profile(own, &mut result);
            }
            result?;
            Ok(rows)
        }
    }

    fn dsp_error(error: Error, context: &Context) -> String {
        if error == Error::Cancelled
            && let Err(cancelled) = context.check()
        {
            return cancelled;
        }
        error.to_string()
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            available: *mut u64,
            capacity: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }

    fn check_disk(directory: &Path, required: u64) -> Result<(), String> {
        if required == 0 {
            return Ok(());
        }
        let directory: Vec<u16> = directory.as_os_str().encode_wide().chain([0]).collect();
        let mut available = 0;
        // SAFETY: directory is NUL-terminated and available is a live u64 output.
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
                "BS output disk query: {}",
                std::io::Error::last_os_error()
            ));
        }
        if available < required {
            return Err(format!(
                "BS output needs {required} disk bytes, {available} available"
            ));
        }
        Ok(())
    }
}
