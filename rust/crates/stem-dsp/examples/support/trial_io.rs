//! Retained diagnostic I/O. Preparation happens before allocator activation;
//! later operations borrow existing handles/buffers and never format OS text.
use flitzis_stem_dsp::Error;
use std::{
    fmt,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

pub const BUFFER_BYTES: usize = 8192;
pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug)]
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

    fn get_ref(&self) -> &W {
        self.writer
            .as_ref()
            .expect("buffer retained until drop")
            .get_ref()
    }
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
}

impl Output {
    fn prepare(directory: &Path, name: &str, phase: &'static str) -> Result<Self> {
        let file = File::create_new(directory.join(name)).at(phase)?;
        Ok(Self {
            writer: ExplicitBuffer::new(file),
            phase,
        })
    }

    pub fn write_values(&mut self, values: &[f32]) -> Result<()> {
        for value in values {
            self.writer.write_all(&value.to_le_bytes()).at(self.phase)?;
        }
        self.finish()
    }

    pub fn finish(&mut self) -> Result<()> {
        self.writer.flush().at(self.phase)?;
        self.writer.get_ref().sync_all().at(self.phase)
    }
}

pub struct ChunkOutputs {
    pub features: Output,
    pub masks: Output,
    pub prediction: Output,
    pub original_masks: Option<Output>,
}

pub enum Outputs {
    Chunk(ChunkOutputs),
    Track(Output),
}

impl Outputs {
    /// Only a fresh private directory is accepted. A preparation or later error
    /// invalidates ALL its empty/partial files; no cleanup or success is implied.
    pub fn prepare(directory: &Path, chunk: bool, original: bool) -> Result<Self> {
        std::fs::create_dir(directory).at("output.directory")?;
        if chunk {
            Ok(Self::Chunk(ChunkOutputs {
                features: Output::prepare(directory, "features.f32", "output.features")?,
                masks: Output::prepare(directory, "masks.f32", "output.masks")?,
                prediction: Output::prepare(directory, "output.f32", "output.pcm")?,
                original_masks: if original {
                    Some(Output::prepare(
                        directory,
                        "original_features_masks.f32",
                        "output.original-masks",
                    )?)
                } else {
                    None
                },
            }))
        } else {
            Ok(Self::Track(Output::prepare(
                directory,
                "output.f32",
                "output.pcm",
            )?))
        }
    }
}

/// Real retained-handle writes and a real read-only-handle failure, without a
/// model or admission claim. The fixture extent is never a full-model budget.
pub fn control(
    directory: &Path,
    workspace: &crate::workspace::WorkspaceAllocator,
    status: &mut impl Write,
) -> Result<()> {
    let Outputs::Chunk(mut outputs) = Outputs::prepare(directory, true, true)? else {
        return Err("invalid I/O control setup".into());
    };
    let read_only = File::open(directory.join("features.f32")).at("control.open")?;
    let mut rejected = ExplicitBuffer::new(read_only);
    workspace.seal(1 << 20)?;
    let before = workspace.stats();
    let values = [0.25f32, -0.5, 1.0, -0.0];
    outputs.features.write_values(&values)?;
    outputs.masks.write_values(&values)?;
    outputs.prediction.write_values(&values)?;
    outputs
        .original_masks
        .as_mut()
        .ok_or("missing I/O control output")?
        .write_values(&values)?;
    let error = rejected
        .write_all(&[1, 2, 3, 4])
        .and_then(|()| rejected.flush())
        .at("control.read-only")
        .err()
        .ok_or("read-only output unexpectedly succeeded")?;
    // The same nonflushing owner as real outputs retains this prepared buffer
    // and handle through the control report, then discards the rejected bytes.
    if !matches!(
        error,
        Failure::Io {
            os_error: Some(_),
            ..
        }
    ) {
        return Err("read-only output lost its numeric OS error".into());
    }
    writeln!(
        status,
        "BS retained I/O control: outputs=4 writes=4 expected_error=({error})"
    )
    .at("status.write")?;
    let after = workspace.stats();
    if after != before {
        return Err("retained I/O allocated after seal".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "trial_io_tests.rs"]
mod tests;
