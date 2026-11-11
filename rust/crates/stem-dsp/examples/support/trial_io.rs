//! Retained diagnostic I/O. Preparation happens before allocator activation;
//! later operations borrow existing handles/buffers and never format OS text.
use flitzis_stem_dsp::Error;
use std::{
    fmt,
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

pub const BUFFER_BYTES: usize = 8192;
pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug, Clone, Copy)]
pub enum Failure {
    Message(&'static str),
    Core(Error),
    Io {
        phase: &'static str,
        os_error: Option<i32>,
    },
}

impl From<&'static str> for Failure {
    fn from(message: &'static str) -> Self {
        Self::Message(message)
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::Core(error) => error.fmt(f),
            Self::Io { phase, os_error } => {
                write!(f, "BS trial I/O failed: phase={phase} os_error=")?;
                match os_error {
                    Some(code) => code.fmt(f),
                    None => f.write_str("unavailable"),
                }
            }
        }
    }
}

pub trait IoPhase<T> {
    fn at(self, phase: &'static str) -> Result<T>;
}

impl<T> IoPhase<T> for io::Result<T> {
    fn at(self, phase: &'static str) -> Result<T> {
        self.map_err(|error| Failure::Io {
            phase,
            os_error: error.raw_os_error(),
        })
    }
}

/// Preserve the first CLI I/O failure across the core's narrow Source/Output
/// callback contract. The core still terminates immediately with its own error.
pub fn core_io<T>(
    result: io::Result<T>,
    phase: &'static str,
    first: &mut Option<Failure>,
    core_error: Error,
) -> std::result::Result<T, Error> {
    result.at(phase).map_err(|failure| {
        first.get_or_insert(failure);
        core_error
    })
}

/// Visit every completion even after failure; the first error stays authoritative.
pub fn retain_first<T>(result: &mut Result<T>, completion: Result<()>) {
    if let Err(error) = completion
        && result.is_ok()
    {
        *result = Err(error);
    }
}

/// Consumes Rust ownership before the sole native close attempt. A failed close
/// is unresolved OS ownership, never proof of settlement and never retried.
pub(super) fn close_file(file: File) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::{ffi::c_void, os::windows::io::IntoRawHandle};
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: *mut c_void) -> i32;
            fn GetLastError() -> u32;
        }
        let handle = file.into_raw_handle();
        // SAFETY: this is the unique owned File handle, after all readers end.
        // No File remains to close it again, including on native failure.
        if unsafe { CloseHandle(handle) } == 0 {
            let code = unsafe { GetLastError() };
            Err(io::Error::from_raw_os_error(code as i32))
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        drop(file);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
pub(super) fn observe_closed_control(handle: std::os::windows::io::RawHandle) -> Result<()> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
        fn GetLastError() -> u32;
    }
    let mut flags = 0;
    // SAFETY: observation only, immediately after the control's owned close;
    // no intervening handle creation. It neither uses nor closes the old handle.
    let found = unsafe { GetHandleInformation(handle, &mut flags) };
    let error = if found == 0 {
        unsafe { GetLastError() }
    } else {
        0
    };
    if found != 0 || error != 6 {
        return Err("control did not observe an invalid closed handle".into());
    }
    Ok(())
}

#[derive(Default)]
pub struct Inputs {
    pub source: Option<BufReader<File>>,
    pub weights: Option<BufReader<File>>,
    pub original: Option<BufReader<File>>,
}

impl Inputs {
    pub fn prepare(
        &mut self,
        source: &Path,
        source_bytes: u64,
        weights: &Path,
        weight_bytes: u64,
        original: Option<(&Path, u64)>,
    ) -> Result<()> {
        self.source = Some(BufReader::with_capacity(
            BUFFER_BYTES,
            File::open(source).at("source.open")?,
        ));
        if self
            .source
            .as_ref()
            .unwrap()
            .get_ref()
            .metadata()
            .at("source.metadata")?
            .len()
            != source_bytes
        {
            return Err("incomplete source".into());
        }
        if let Some((path, bytes)) = original {
            self.original = Some(BufReader::with_capacity(
                BUFFER_BYTES,
                File::open(path).at("original-features.open")?,
            ));
            if self
                .original
                .as_ref()
                .unwrap()
                .get_ref()
                .metadata()
                .at("original-features.metadata")?
                .len()
                != bytes
            {
                return Err("incomplete original features".into());
            }
        }
        self.weights = Some(BufReader::with_capacity(
            BUFFER_BYTES,
            File::open(weights).at("weights.open")?,
        ));
        if self
            .weights
            .as_ref()
            .unwrap()
            .get_ref()
            .metadata()
            .at("weights.metadata")?
            .len()
            != weight_bytes
        {
            return Err("incomplete fixed-schema weights".into());
        }
        Ok(())
    }

    pub fn complete<T>(self, result: &mut Result<T>) {
        for (reader, phase) in [
            (self.source, "source.close"),
            (self.weights, "weights.close"),
            (self.original, "original-features.close"),
        ] {
            if let Some(reader) = reader {
                retain_first(result, close_file(reader.into_inner()).at(phase));
            }
        }
    }
}

/// Only explicit writes/flushes may reach the sink. Abandoning private output,
/// including unwinding after a failure, discards buffered bytes without retrying.
pub struct ExplicitBuffer<W: Write> {
    writer: Option<BufWriter<W>>,
}

impl<W: Write> ExplicitBuffer<W> {
    fn new(writer: W) -> Self {
        Self {
            writer: Some(BufWriter::with_capacity(BUFFER_BYTES, writer)),
        }
    }

    fn buffer(&mut self) -> &mut BufWriter<W> {
        self.writer.as_mut().expect("buffer retained until drop")
    }

    fn into_sink(mut self) -> W {
        let (sink, _pending) = self
            .writer
            .take()
            .expect("buffer retained until completion")
            .into_parts();
        sink
    }
}

// A diagnostic sink seam, also used by deterministic sync/close failure controls.
trait CompleteSink: Write {
    fn sync(&mut self) -> io::Result<()>;
    fn close(self) -> io::Result<()>;
}

impl CompleteSink for File {
    fn sync(&mut self) -> io::Result<()> {
        self.sync_all()
    }

    fn close(self) -> io::Result<()> {
        close_file(self)
    }
}

fn complete_buffer<W: CompleteSink>(mut writer: ExplicitBuffer<W>, successful: bool) -> Result<()> {
    let mut result = Ok(());
    if successful {
        retain_first(&mut result, writer.flush().at("output.flush"));
    }
    // into_parts never flushes, including after a partial write or writer panic.
    let mut sink = writer.into_sink();
    if successful && result.is_ok() {
        retain_first(&mut result, sink.sync().at("output.sync"));
    }
    retain_first(&mut result, sink.close().at("output.close"));
    result
}

impl<W: Write> Write for ExplicitBuffer<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer().write(bytes)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.buffer().write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.buffer().flush()
    }
}

impl<W: Write> Drop for ExplicitBuffer<W> {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            // into_parts performs no flush, even after an inner writer panic.
            // The sink still closes normally; this does not bound native close.
            let (_sink, _pending) = writer.into_parts();
        }
    }
}

pub struct Output {
    pub writer: ExplicitBuffer<File>,
    phase: &'static str,
    // Only create_new output paths are owned. Never infer cleanup from a name.
    path: Option<PathBuf>,
}

impl Output {
    fn prepare(directory: &Path, name: &str, phase: &'static str) -> Result<Self> {
        let path = directory.join(name);
        let file = File::create_new(&path).at(phase)?;
        Ok(Self {
            writer: ExplicitBuffer::new(file),
            phase,
            path: Some(path),
        })
    }

    pub fn write_values(&mut self, values: &[f32]) -> Result<()> {
        self.write_values_checked(values, &|| Ok(()))
    }

    pub fn write_values_checked(
        &mut self,
        values: &[f32],
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        let mut bytes = [0u8; BUFFER_BYTES];
        for block in values.chunks(BUFFER_BYTES / 4) {
            check()?;
            for (raw, value) in bytes.chunks_exact_mut(4).zip(block) {
                raw.copy_from_slice(&value.to_le_bytes());
            }
            self.writer
                .write_all(&bytes[..block.len() * 4])
                .at(self.phase)?;
        }
        check()
    }

    pub fn complete(self, successful: bool) -> Result<()> {
        let mut result = complete_buffer(self.writer, successful);
        if !successful && let Some(path) = self.path {
            // All numerical borrows and the sole checked close precede deletion.
            // No recursive deletion or destructor retry, even on close failure.
            retain_first(
                &mut result,
                std::fs::remove_file(path).at("output.invalidate"),
            );
        }
        result
    }
}

/// Primary outcome and cleanup qualification are independent. In particular a
/// Cancelled primary cannot conceal a later close/invalidation failure from ACK.
pub fn complete<T>(
    result: Result<T>,
    inputs: Inputs,
    outputs: Option<Outputs>,
    cleanup: &mut Result<()>,
    final_check: &dyn Fn() -> Result<()>,
) -> Result<T> {
    complete_retained(
        result,
        inputs,
        outputs,
        cleanup,
        final_check,
        &mut PrivateOutputs::default(),
    )
}

/// Exact exclusive creations remain owned through terminal writes and closes.
/// No destructor deletes files or retries a failed native operation on unwind.
#[derive(Default)]
pub struct PrivateOutputs {
    paths: [Option<PathBuf>; 4],
}

impl PrivateOutputs {
    pub fn finish<T>(self, successful: bool, cleanup: &mut Result<T>) {
        if !successful {
            for path in self.paths.into_iter().flatten() {
                retain_first(cleanup, std::fs::remove_file(path).at("output.invalidate"));
            }
        }
    }
}

pub fn complete_retained<T>(
    mut result: Result<T>,
    inputs: Inputs,
    mut outputs: Option<Outputs>,
    cleanup: &mut Result<()>,
    final_check: &dyn Fn() -> Result<()>,
    private: &mut PrivateOutputs,
) -> Result<T> {
    // Move path custody, without allocating, beyond the last flush/sync/close.
    let paths = outputs.as_mut().map(Outputs::take_paths);
    let successful = result.is_ok();
    inputs.complete(cleanup);
    if let Some(outputs) = outputs {
        outputs.complete(successful, cleanup);
    }
    retain_first(&mut result, *cleanup);
    if result.is_ok() {
        retain_first(&mut result, final_check());
    }
    if let Some(paths) = paths {
        let owned = PrivateOutputs { paths };
        if result.is_ok() {
            *private = owned;
        } else {
            owned.finish(false, cleanup);
        }
    }
    result
}

pub struct ChunkOutputs {
    pub features: Output,
    pub masks: Output,
    pub prediction: Output,
    pub original_masks: Option<Output>,
}

pub enum Outputs {
    // Prepared before seal together with handles, paths and buffered storage.
    Chunk(Box<ChunkOutputs>),
    Track(Output),
}

impl Outputs {
    fn take_paths(&mut self) -> [Option<PathBuf>; 4] {
        match self {
            Self::Chunk(output) => [
                output.features.path.take(),
                output.masks.path.take(),
                output.prediction.path.take(),
                output.original_masks.as_mut().and_then(|o| o.path.take()),
            ],
            Self::Track(output) => [output.path.take(), None, None, None],
        }
    }

    /// Only a fresh private directory is accepted. A preparation or later error
    /// invalidates ALL its empty/partial files; no cleanup or success is implied.
    pub fn prepare(directory: &Path, chunk: bool, original: bool) -> Result<Self> {
        std::fs::create_dir(directory).at("output.directory")?;
        let mut staged: [Option<Output>; 4] = [None, None, None, None];
        let result = (|| {
            if chunk {
                for (slot, name, phase) in [
                    (0, "features.f32", "output.features"),
                    (1, "masks.f32", "output.masks"),
                    (2, "output.f32", "output.pcm"),
                ] {
                    staged[slot] = Some(Output::prepare(directory, name, phase)?);
                }
                if original {
                    staged[3] = Some(Output::prepare(
                        directory,
                        "original_features_masks.f32",
                        "output.original-masks",
                    )?);
                }
            } else {
                staged[0] = Some(Output::prepare(directory, "output.f32", "output.pcm")?);
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut failed = Err(error);
            for output in staged.into_iter().flatten() {
                retain_first(&mut failed, output.complete(false));
            }
            return failed;
        }
        if chunk {
            Ok(Self::Chunk(Box::new(ChunkOutputs {
                features: staged[0].take().unwrap(),
                masks: staged[1].take().unwrap(),
                prediction: staged[2].take().unwrap(),
                original_masks: staged[3].take(),
            })))
        } else {
            Ok(Self::Track(staged[0].take().unwrap()))
        }
    }

    pub fn complete<T>(self, successful: bool, result: &mut Result<T>) {
        match self {
            Self::Chunk(outputs) => {
                for output in [
                    Some(outputs.features),
                    Some(outputs.masks),
                    Some(outputs.prediction),
                    outputs.original_masks,
                ]
                .into_iter()
                .flatten()
                {
                    retain_first(result, output.complete(successful));
                }
            }
            Self::Track(output) => retain_first(result, output.complete(successful)),
        }
    }
}

/// Check both borrowed sinks without closing their runtime-owned handles.
/// Diagnostic failure never replaces the original error or triggers recursion.
pub fn report_terminal(
    mut result: Result<()>,
    status: &mut impl Write,
    errors: &mut impl Write,
) -> Result<()> {
    retain_first(&mut result, status.flush().at("status.flush"));
    if let Err(error) = &result {
        let mut report = writeln!(errors, "{error}").at("error.write");
        retain_first(&mut report, errors.flush().at("error.flush"));
        // Both operations were checked once. Their failure cannot erase the
        // original failure, and this path never reports its own report failure.
        retain_first(&mut result, report);
    }
    result
}

/// Real retained-handle writes and a real read-only-handle failure, without a
/// model or admission claim. The fixture extent is never a full-model budget.
pub fn control(
    directory: &Path,
    workspace: &crate::workspace::WorkspaceAllocator,
    status: &mut impl Write,
    private: &mut PrivateOutputs,
) -> Result<()> {
    let mut prepared = Outputs::prepare(directory, true, true)?;
    let mut inputs = Inputs::default();
    let mut rejected = None;
    let mut baseline = None;
    let mut result = (|| {
        let Outputs::Chunk(outputs) = &mut prepared else {
            return Err("invalid I/O control setup".into());
        };
        // Tiny retained inputs only: these are initially empty control files,
        // not weights/model data. All readers and buffers exist before seal.
        inputs.prepare(
            &directory.join("features.f32"),
            0,
            &directory.join("masks.f32"),
            0,
            Some((&directory.join("output.f32"), 0)),
        )?;
        rejected = Some(ExplicitBuffer::new(
            File::open(directory.join("features.f32")).at("control.open")?,
        ));
        workspace.seal(1 << 20)?;
        baseline = Some(workspace.stats());
        let values = [0.25f32, -0.5, 1.0, -0.0];
        outputs.features.write_values(&values)?;
        outputs.masks.write_values(&values)?;
        outputs.prediction.write_values(&values)?;
        outputs
            .original_masks
            .as_mut()
            .ok_or("missing I/O control output")?
            .write_values(&values)?;
        let rejected = rejected.as_mut().unwrap();
        let error = rejected
            .write_all(&[1, 2, 3, 4])
            .and_then(|()| rejected.flush())
            .at("control.read-only")
            .err()
            .ok_or("read-only output unexpectedly succeeded")?;
        // The expected rejected bytes are abandoned, never retried during completion.
        if !matches!(
            error,
            Failure::Io {
                os_error: Some(_),
                ..
            }
        ) {
            return Err("read-only output lost its numeric OS error".into());
        }
        Ok(error)
    })();
    let successful = result.is_ok();
    let paths = PrivateOutputs {
        paths: prepared.take_paths(),
    };
    prepared.complete(successful, &mut result);
    if let Some(reader) = &mut inputs.source {
        let read_result = (|| {
            if result.is_ok() {
                use std::io::{Read, Seek, SeekFrom};
                let mut bytes = [0; 16];
                reader.read_exact(&mut bytes).at("control.read")?;
                for (raw, expected) in bytes.chunks_exact(4).zip([0.25f32, -0.5, 1.0, -0.0]) {
                    if raw != expected.to_le_bytes() {
                        return Err("retained I/O control bytes differ".into());
                    }
                }
                reader.seek(SeekFrom::Start(0)).at("control.seek")?;
                let mut too_long = [0; 17];
                match reader.read_exact(&mut too_long) {
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
                    _ => return Err("retained I/O control lost EOF".into()),
                }
            }
            Ok(())
        })();
        retain_first(&mut result, read_result);
    }
    #[cfg(windows)]
    let handle = inputs
        .source
        .as_ref()
        .map(|reader| std::os::windows::io::AsRawHandle::as_raw_handle(reader.get_ref()));
    inputs.complete(&mut result);
    #[cfg(windows)]
    if result.is_ok()
        && let Some(handle) = handle
    {
        retain_first(&mut result, observe_closed_control(handle));
    }
    if let Some(rejected) = rejected {
        retain_first(&mut result, complete_buffer(rejected, false));
    }
    if result.is_err() {
        paths.finish(false, &mut result);
    } else {
        *private = paths;
    }
    let error = result?;
    if Some(workspace.stats()) != baseline {
        return Err("retained I/O allocated after seal".into());
    }
    writeln!(
        status,
        "BS retained I/O control: outputs=4 writes=4 closed=8 expected_error=({error})"
    )
    .at("status.write")?;
    Ok(())
}

#[cfg(test)]
#[path = "trial_io_tests.rs"]
mod tests;
