//! Complete signals with bounded reads; file storage never enters realtime.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};

pub(crate) enum Signal<'a> {
    Slice(&'a [f64]),
    Owned(Vec<f64>),
    File {
        file: File,
        frames: usize,
        width: usize,
    },
}

#[cfg(test)]
pub(crate) fn test_file() -> File {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "key-signal-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x04000000); // FILE_FLAG_DELETE_ON_CLOSE, like caller tempfile.
    }
    let file = options.open(&path).unwrap();
    #[cfg(not(windows))]
    std::fs::remove_file(path).unwrap();
    file
}

pub(crate) fn check_cancelled(cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Key analysis cancelled",
        ))
    } else {
        Ok(())
    }
}

impl Signal<'_> {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Slice(samples) => samples.len(),
            Self::Owned(samples) => samples.len(),
            Self::File { frames, .. } => *frames,
        }
    }

    pub(crate) fn file(file: File, frames: usize, width: usize) -> io::Result<Self> {
        let bytes = frames
            .checked_mul(width)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| io::Error::other("Key signal extent overflow"))?;
        if !matches!(width, 4 | 8) || file.metadata()?.len() != bytes {
            return Err(io::Error::other("Key signal extent differs"));
        }
        Ok(Self::File {
            file,
            frames,
            width,
        })
    }

    pub(crate) fn read_range(&mut self, start: usize, output: &mut [f64]) -> io::Result<()> {
        let end = start
            .checked_add(output.len())
            .filter(|end| *end <= self.len())
            .ok_or_else(|| io::Error::other("Key signal range outside extent"))?;
        match self {
            Self::Slice(samples) => output.copy_from_slice(&samples[start..end]),
            Self::Owned(samples) => output.copy_from_slice(&samples[start..end]),
            Self::File { file, width, .. } => {
                let offset = start
                    .checked_mul(*width)
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(|| io::Error::other("Key signal offset overflow"))?;
                file.seek(SeekFrom::Start(offset))?;
                let mut bytes = [0_u8; 8192];
                for chunk in output.chunks_mut(bytes.len() / *width) {
                    file.read_exact(&mut bytes[..chunk.len() * *width])?;
                    for (sample, encoded) in chunk.iter_mut().zip(bytes.chunks_exact(*width)) {
                        *sample = if *width == 4 {
                            f64::from(f32::from_le_bytes(encoded.try_into().unwrap()))
                        } else {
                            f64::from_le_bytes(encoded.try_into().unwrap())
                        };
                    }
                }
            }
        }
        if output.iter().any(|sample| !sample.is_finite()) {
            return Err(io::Error::other("Non-finite Key signal"));
        }
        Ok(())
    }

    /// Validate every sample, including samples between STFT reads, before inference.
    pub(crate) fn is_silent(&mut self, cancelled: &dyn Fn() -> bool) -> io::Result<bool> {
        let mut samples = [0.0; 1024];
        let mut silent = true;
        for start in (0..self.len()).step_by(samples.len()) {
            check_cancelled(cancelled)?;
            let count = samples.len().min(self.len() - start);
            self.read_range(start, &mut samples[..count])?;
            silent &= samples[..count]
                .iter()
                .all(|sample| sample.abs() < f64::from(1e-6_f32));
        }
        check_cancelled(cancelled)?;
        Ok(silent)
    }
}

pub(crate) enum Scratch<'a> {
    Memory,
    Files(&'a mut dyn FnMut() -> io::Result<File>),
}

pub(crate) enum Sink {
    Memory(Vec<f64>),
    File(File, usize),
}

impl Scratch<'_> {
    pub(crate) fn sink(&mut self, frames: usize) -> io::Result<Sink> {
        frames
            .checked_mul(8)
            .ok_or_else(|| io::Error::other("Key scratch overflow"))?;
        match self {
            Self::Memory => {
                let mut samples = Vec::new();
                samples
                    .try_reserve_exact(frames)
                    .map_err(io::Error::other)?;
                if samples.capacity() != frames {
                    return Err(io::Error::other("Unexpected Key scratch capacity"));
                }
                Ok(Sink::Memory(samples))
            }
            Self::Files(factory) => {
                let file = factory()?;
                if file.metadata()?.len() != 0 {
                    return Err(io::Error::other("Key scratch must be empty"));
                }
                Ok(Sink::File(file, 0))
            }
        }
    }
}

impl Sink {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Memory(samples) => samples.len(),
            Self::File(_, frames) => *frames,
        }
    }

    pub(crate) fn append(&mut self, samples: &[f64]) -> io::Result<()> {
        match self {
            Self::Memory(output) => output.extend_from_slice(samples),
            Self::File(file, frames) => {
                let mut bytes = [0_u8; 8192];
                for chunk in samples.chunks(1024) {
                    for (value, destination) in chunk.iter().zip(bytes.chunks_exact_mut(8)) {
                        destination.copy_from_slice(&value.to_le_bytes());
                    }
                    file.write_all(&bytes[..chunk.len() * 8])?;
                }
                *frames = frames
                    .checked_add(samples.len())
                    .ok_or_else(|| io::Error::other("Key scratch overflow"))?;
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self, frames: usize) -> io::Result<Signal<'static>> {
        let zeros = [0.0; 1024];
        while self.len() < frames {
            self.append(&zeros[..zeros.len().min(frames - self.len())])?;
        }
        match self {
            Self::Memory(samples) => Ok(Signal::Owned(samples)),
            Self::File(mut file, _) => {
                file.flush()?;
                Signal::file(file, frames, 8)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_reads_preserve_bits_and_reject_extent_changes_nonfinite_and_cancel() {
        let values = [-0.0_f32, 0.25, -0.9375];
        let mut file = test_file();
        for value in values {
            file.write_all(&value.to_le_bytes()).unwrap();
        }
        assert!(Signal::file(file.try_clone().unwrap(), 2, 4).is_err());
        let mut signal = Signal::file(file.try_clone().unwrap(), 3, 4).unwrap();
        let mut actual = [0.0; 3];
        signal.read_range(0, &mut actual).unwrap();
        assert!(
            actual
                .iter()
                .zip(values)
                .all(|(a, b)| a.to_bits() == f64::from(b).to_bits())
        );
        assert!(!signal.is_silent(&|| false).unwrap());
        assert_eq!(
            signal.is_silent(&|| true).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert!(signal.read_range(usize::MAX, &mut actual).is_err());
        file.set_len(4).unwrap();
        assert_eq!(
            signal.read_range(0, &mut actual).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&f32::NAN.to_le_bytes()).unwrap();
        assert!(
            Signal::file(file, 1, 4)
                .unwrap()
                .is_silent(&|| false)
                .is_err()
        );
    }
}
