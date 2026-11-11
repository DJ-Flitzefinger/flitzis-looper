//! Shared caller, file, ORT-helper and terminal lifetime for both standalone CLIs.
use flitzis_looper_analysis::{
    beat_inference::{BeatModel, Logits},
    trial_lifetime::{CANCELLED, Context, retain_first, terminate_during},
};
use ort::session::RunOptions;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

pub struct Invocation {
    pub context: Context,
    paths: Vec<PathBuf>,
    profile_directory: Option<PathBuf>,
}

impl Invocation {
    pub fn new() -> Self {
        Self {
            context: Context::from_environment(),
            paths: Vec::new(),
            profile_directory: None,
        }
    }

    pub fn read(&self, path: &Path, limit: usize) -> Result<(Vec<u8>, String), String> {
        self.read_checked(path, limit, |_| Ok(()))
    }

    pub fn read_checked(
        &self,
        path: &Path,
        limit: usize,
        validate: impl FnOnce(u64) -> Result<(), String>,
    ) -> Result<(Vec<u8>, String), String> {
        self.context.checkpoint("input.read")?;
        let mut file = self
            .context
            .file(File::open(path).map_err(|e| e.to_string())?, "input.close");
        let mut result = (|| {
            let extent = file.metadata().map_err(|e| e.to_string())?.len();
            if extent > limit as u64 {
                return Err("input exceeds complete trial byte bound".into());
            }
            validate(extent)?;
            let mut raw = Vec::new();
            raw.try_reserve_exact(extent as usize)
                .map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                self.context.check()?;
                let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                if raw.len() + count > limit {
                    return Err("input changed beyond trial byte bound".into());
                }
                hash.update(&buffer[..count]);
                raw.extend_from_slice(&buffer[..count]);
            }
            if raw.len() as u64 != extent {
                return Err("complete input changed during read".into());
            }
            self.context.checkpoint("input.hash")?;
            Ok((raw, format!("{:x}", hash.finalize())))
        })();
        retain_first(&mut result, file.complete());
        result
    }

    pub fn floats(&self, raw: &[u8]) -> Result<Vec<f32>, String> {
        if !raw.len().is_multiple_of(4) {
            return Err("invalid float32 byte extent".into());
        }
        let mut values = Vec::with_capacity(raw.len() / 4);
        for block in raw.chunks(65536) {
            self.context.check()?;
            for bytes in block.chunks_exact(4) {
                let value = f32::from_le_bytes(bytes.try_into().unwrap());
                if !value.is_finite() {
                    return Err("nonfinite complete float32 input".into());
                }
                values.push(value);
            }
        }
        Ok(values)
    }

    pub fn write(
        &mut self,
        path: &Path,
        write: impl FnOnce(&mut File, &Context) -> Result<(), String>,
    ) -> Result<(), String> {
        self.context.checkpoint("output.write")?;
        let path = path.to_owned();
        self.paths.try_reserve(1).map_err(|e| e.to_string())?;
        let file = File::create_new(&path).map_err(|e| e.to_string())?;
        // Custody begins only after successful exclusive creation.
        self.paths.push(path);
        let mut file = self.context.file(file, "output.close");
        let mut result = write(&mut file, &self.context);
        if result.is_ok() {
            retain_first(&mut result, file.flush().map_err(|e| e.to_string()));
        }
        if result.is_ok() {
            retain_first(&mut result, file.sync_all().map_err(|e| e.to_string()));
        }
        retain_first(&mut result, file.complete());
        result
    }

    pub fn write_floats(&mut self, path: &Path, values: &[f32]) -> Result<(), String> {
        self.write(path, |file, context| {
            let mut bytes = [0u8; 8192];
            for block in values.chunks(bytes.len() / 4) {
                context.check()?;
                for (raw, value) in bytes.chunks_exact_mut(4).zip(block) {
                    raw.copy_from_slice(&value.to_le_bytes());
                }
                file.write_all(&bytes[..block.len() * 4])
                    .map_err(|e| e.to_string())?;
            }
            context.check()
        })
    }

    /// ORT chooses a suffixed filename. A new exclusive private directory makes
    /// its generated files attributable without deleting any caller prefix match.
    pub fn profile(&mut self, prefix: &Path) -> Result<PathBuf, String> {
        self.context.checkpoint("profile.prepare")?;
        let mut name = prefix.as_os_str().to_owned();
        name.push(format!(
            ".native-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        let directory = PathBuf::from(name);
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        self.profile_directory = Some(directory.clone());
        let directory = directory.canonicalize().map_err(|e| e.to_string())?;
        let profile = directory.join("profile");
        self.profile_directory = Some(directory);
        Ok(profile)
    }

    pub fn finish_profile<T>(
        &self,
        model: &mut BeatModel,
        result: &mut Result<T, String>,
    ) -> Option<String> {
        // Runs and their helpers have already joined. Always attempt profiling,
        // even on cancellation or an earlier run failure.
        self.finish_profile_with(result, || model.finish_profile())
    }

    fn finish_profile_with<T>(
        &self,
        result: &mut Result<T, String>,
        finish: impl FnOnce() -> Result<String, String>,
    ) -> Option<String> {
        let profile = finish();
        if let Err(error) = &profile {
            self.context.record(Err(format!("profile.finish: {error}")));
        }
        retain_first(result, profile.as_ref().map(|_| ()).map_err(Clone::clone));
        profile.ok()
    }

    fn collect_profiles(&mut self) -> Result<(), String> {
        if let Some(directory) = &self.profile_directory {
            // No session remains. Only actual regular files in the freshly
            // created invocation directory are owned; never follow links/globs.
            for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                    return Err("unexpected entry in private profile directory".into());
                }
                self.paths.push(entry.path());
            }
        }
        Ok(())
    }

    fn invalidate(&mut self) {
        for path in self.paths.drain(..) {
            self.context
                .record(std::fs::remove_file(path).map_err(|e| format!("output.invalidate: {e}")));
        }
        if let Some(path) = self.profile_directory.take() {
            self.context
                .record(std::fs::remove_dir(path).map_err(|e| format!("profile.invalidate: {e}")));
        }
    }

    pub fn finish(
        mut self,
        mut result: Result<serde_json::Value, String>,
        diagnostic: Option<serde_json::Value>,
        status: &mut impl Write,
        errors: &mut impl Write,
    ) -> ExitCode {
        let profiles = self.collect_profiles();
        self.context.record(profiles);
        retain_first(&mut result, self.context.cleanup());
        let mut terminal = status.flush().map_err(|e| e.to_string());
        retain_first(&mut terminal, errors.flush().map_err(|e| e.to_string()));
        if result.is_ok() {
            retain_first(&mut result, self.context.checkpoint("completion"));
        }
        if result.is_err() {
            self.invalidate();
        }
        let cleanup = self.context.cleanup();
        let cancelled = result.as_ref().err().is_some_and(|e| {
            e == CANCELLED || e.starts_with(&format!("{CANCELLED}; native run: "))
        });
        let code = if terminal.is_ok() && cleanup.is_ok() && cancelled {
            retain_first(
                &mut terminal,
                writeln!(
                    status,
                    "FLITZIS_STEM_CANCELLED_V1:{{\"stage\":\"{}\"}}",
                    self.context.stage()
                )
                .map_err(|e| e.to_string()),
            );
            ExitCode::from(130)
        } else if terminal.is_ok() && cleanup.is_ok() && result.is_ok() {
            retain_first(
                &mut terminal,
                writeln!(status, "{}", result.as_ref().unwrap()).map_err(|e| e.to_string()),
            );
            ExitCode::SUCCESS
        } else if terminal.is_ok()
            && cleanup.is_ok()
            && let Some(diagnostic) = diagnostic
        {
            retain_first(
                &mut terminal,
                writeln!(status, "{diagnostic}").map_err(|e| e.to_string()),
            );
            ExitCode::from(70)
        } else {
            if let Err(error) = result {
                retain_first(
                    &mut terminal,
                    writeln!(errors, "Error: {error}").map_err(|e| e.to_string()),
                );
            }
            if let Err(error) = cleanup {
                retain_first(
                    &mut terminal,
                    writeln!(errors, "Cleanup: {error}").map_err(|e| e.to_string()),
                );
            }
            ExitCode::FAILURE
        };
        // Both runtime-owned sinks are borrowed, checked, and never closed here.
        retain_first(&mut terminal, status.flush().map_err(|e| e.to_string()));
        retain_first(&mut terminal, errors.flush().map_err(|e| e.to_string()));
        if terminal.is_err() {
            self.invalidate();
            ExitCode::FAILURE
        } else {
            code
        }
    }
}

/// A helper exists only for this run/options borrow. Its condition-variable wait
/// is woken on run completion; it always joins before profile/session release.
pub fn run(
    model: &mut BeatModel,
    chunk: &[f32],
    context: &Context,
    diagnostic_ms: Option<u64>,
) -> Result<Logits, String> {
    run_observed(model, chunk, context, diagnostic_ms, |_, _| {})
}

pub fn run_observed(
    model: &mut BeatModel,
    chunk: &[f32],
    context: &Context,
    diagnostic_ms: Option<u64>,
    observe_failure: impl FnOnce(ort::ErrorCode, &str),
) -> Result<Logits, String> {
    context.checkpoint("model.run")?;
    let options = RunOptions::new().map_err(|e| e.to_string())?;
    let mut native_terminated = false;
    let (result, requested) = terminate_during(
        context,
        diagnostic_ms,
        || options.terminate().map_err(|e| e.to_string()),
        || {
            let (result, terminated) = model.run_observing_failure(
                chunk,
                &options,
                &|| context.cancelled(),
                observe_failure,
            );
            native_terminated = terminated;
            result
        },
    );
    let result = qualify_run(result, native_terminated, requested, context);

    if result.is_ok() && diagnostic_ms.is_some() {
        return Err("diagnostic run finished before termination".into());
    }
    result
}

fn qualify_run<T>(
    result: Result<T, String>,
    native_terminated: bool,
    terminated: bool,
    context: &Context,
) -> Result<T, String> {
    if native_terminated && terminated && context.cancelled() {
        result.map_err(|primary| format!("{CANCELLED}; native run: {primary}"))
    } else {
        result
    }
}

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod tests;
