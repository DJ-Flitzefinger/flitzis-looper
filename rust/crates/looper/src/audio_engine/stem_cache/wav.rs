//! Bounded PCM16 RIFF/RF64 inspection and positioned conversion on sealed files.
use super::{WavFormat, parse_pcm16_wav_format, pcm16_to_float};
use std::fs::File;
use std::io;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pcm16Wav {
    pub(crate) format: WavFormat,
    pub(crate) frames: usize,
    data_offset: u64,
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}

fn check(cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(invalid("complete stem WAV read cancelled"))
    } else {
        Ok(())
    }
}

/// Read without sharing the seek cursor of a retained/cloned immutable reader.
pub(crate) fn read_at(file: &File, mut bytes: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !bytes.is_empty() {
        #[cfg(windows)]
        let count = {
            use std::os::windows::fs::FileExt;
            file.seek_read(bytes, offset)?
        };
        #[cfg(unix)]
        let count = {
            use std::os::unix::fs::FileExt;
            file.read_at(bytes, offset)?
        };
        #[cfg(not(any(windows, unix)))]
        return Err(invalid("positioned WAV reader unavailable"));
        if count == 0 {
            return Err(invalid("stem WAV EOF mismatch"));
        }
        offset = offset
            .checked_add(count as u64)
            .ok_or_else(|| invalid("stem WAV position overflow"))?;
        bytes = &mut bytes[count..];
    }
    Ok(())
}

impl Pcm16Wav {
    /// Header sizes are disk extents, never allocation sizes. Unknown chunks are
    /// skipped with checked u64 offsets; only the fixed fmt/ds64 prefix is read.
    pub(crate) fn inspect(file: &File, cancelled: &dyn Fn() -> bool) -> io::Result<Self> {
        check(cancelled)?;
        let extent = file.metadata()?.len();
        let mut header = [0; 12];
        read_at(file, &mut header, 0)?;
        let rf64 = &header[..4] == b"RF64";
        if (!rf64 && &header[..4] != b"RIFF") || &header[8..] != b"WAVE" {
            return Err(invalid("expected RIFF/RF64 WAVE header"));
        }
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap());
        if rf64 && size != u32::MAX {
            return Err(invalid("RF64 requires sentinel RIFF extent"));
        }
        if !rf64 && u64::from(size).checked_add(8) != Some(extent) {
            return Err(invalid("RIFF declared extent/EOF mismatch"));
        }
        let mut offset = 12_u64;
        let mut format = None;
        let mut data = None;
        let mut ds64: Option<(u64, u64, u64)> = None;
        while offset < extent {
            check(cancelled)?;
            if extent - offset < 8 {
                return Err(invalid("trailing partial WAV chunk"));
            }
            let mut chunk = [0; 8];
            read_at(file, &mut chunk, offset)?;
            let id = &chunk[..4];
            let size32 = u32::from_le_bytes(chunk[4..].try_into().unwrap());
            let start = offset
                .checked_add(8)
                .ok_or_else(|| invalid("WAV offset overflow"))?;
            let size = if size32 == u32::MAX {
                if rf64 && id == b"data" {
                    ds64.ok_or_else(|| invalid("RF64 data precedes ds64"))?.1
                } else {
                    return Err(invalid("unsupported sentinel WAV chunk extent"));
                }
            } else {
                u64::from(size32)
            };
            let end = start
                .checked_add(size)
                .ok_or_else(|| invalid("WAV extent overflow"))?;
            let next = end
                .checked_add(size % 2)
                .ok_or_else(|| invalid("WAV pad overflow"))?;
            if next > extent {
                return Err(invalid("WAV chunk extends past end of file"));
            }
            match id {
                b"ds64" => {
                    if !rf64 || offset != 12 || ds64.is_some() || size < 28 {
                        return Err(invalid("invalid or duplicate RF64 ds64 chunk"));
                    }
                    let mut values = [0; 28];
                    read_at(file, &mut values, start)?;
                    let riff = u64::from_le_bytes(values[..8].try_into().unwrap());
                    let payload = u64::from_le_bytes(values[8..16].try_into().unwrap());
                    let frames = u64::from_le_bytes(values[16..24].try_into().unwrap());
                    let table = u32::from_le_bytes(values[24..].try_into().unwrap());
                    // This format has one data chunk. A table is only needed for
                    // further sentinel-sized chunks, which we reject explicitly.
                    if table != 0 || size != 28 || riff.checked_add(8) != Some(extent) {
                        return Err(invalid(
                            "unsupported RF64 table or declared extent/EOF mismatch",
                        ));
                    }
                    ds64 = Some((riff, payload, frames));
                }
                b"fmt " => {
                    if format.is_some() || size < 16 {
                        return Err(invalid("missing or duplicate WAV format"));
                    }
                    let mut bytes = [0; 40];
                    let length = size.min(bytes.len() as u64) as usize;
                    read_at(file, &mut bytes[..length], start)?;
                    format = Some(parse_pcm16_wav_format(&bytes[..length]).map_err(invalid)?);
                }
                b"data" => {
                    if data.is_some() || (rf64 && size32 != u32::MAX) {
                        return Err(invalid("duplicate data or nonsentinel RF64 data chunk"));
                    }
                    data = Some((start, size));
                }
                _ => (),
            }
            offset = next;
        }
        let format = format.ok_or_else(|| invalid("missing fmt chunk"))?;
        let (data_offset, data_bytes) = data.ok_or_else(|| invalid("missing data chunk"))?;
        if data_bytes == 0 || data_bytes % format.block_align as u64 != 0 {
            return Err(invalid("data length does not align to WAV block size"));
        }
        let frames64 = data_bytes / format.block_align as u64;
        if rf64 && ds64.is_none_or(|(_, bytes, frames)| bytes != data_bytes || frames != frames64) {
            return Err(invalid("RF64 complete sample/data geometry mismatch"));
        }
        let frames = usize::try_from(frames64).map_err(invalid)?;
        check(cancelled)?;
        Ok(Self {
            format,
            frames,
            data_offset,
        })
    }

    pub(crate) fn validate_geometry(
        &self,
        rate: u32,
        channels: usize,
        frames: usize,
    ) -> Result<(), String> {
        if self.format.sample_rate_hz != rate {
            return Err(format!(
                "sample rate mismatch: expected {rate}, got {}",
                self.format.sample_rate_hz
            ));
        }
        if self.format.channels != channels {
            return Err(format!(
                "channel count mismatch: expected {channels}, got {}",
                self.format.channels
            ));
        }
        if self.frames != frames {
            return Err(format!(
                "frame count mismatch: expected {frames}, got {}",
                self.frames
            ));
        }
        Ok(())
    }

    pub(crate) fn read_frames(
        &self,
        file: &File,
        first: usize,
        values: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        let channels = self.format.channels;
        if !values.len().is_multiple_of(channels)
            || first
                .checked_add(values.len() / channels)
                .is_none_or(|end| end > self.frames)
        {
            return Err(invalid("stem WAV range geometry mismatch"));
        }
        let sample = first
            .checked_mul(channels)
            .ok_or_else(|| invalid("WAV range overflow"))?;
        let byte = (sample as u64)
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.data_offset))
            .ok_or_else(|| invalid("WAV byte position overflow"))?;
        let mut encoded = [0_u8; 8192];
        for (index, destination) in values.chunks_mut(encoded.len() / 2).enumerate() {
            check(cancelled)?;
            let offset = byte
                .checked_add((index * encoded.len()) as u64)
                .ok_or_else(|| invalid("WAV block position overflow"))?;
            read_at(file, &mut encoded[..destination.len() * 2], offset)?;
            for (target, word) in destination.iter_mut().zip(encoded.chunks_exact(2)) {
                *target = pcm16_to_float(i16::from_le_bytes(word.try_into().unwrap()));
            }
        }
        check(cancelled)
    }
}
