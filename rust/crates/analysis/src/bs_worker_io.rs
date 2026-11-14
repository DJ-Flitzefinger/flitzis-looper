//! Complete, private input and output custody for the standalone BS worker.
use flitzis_looper_analysis::trial_lifetime::{Context, OwnedFile};
use flitzis_stem_dsp::{CHANNELS, RATE};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const BLOCK_BYTES: usize = 65536;
const FRAME_BYTES: usize = CHANNELS * size_of::<f32>();
pub const NAMES: [&str; 4] = ["drums", "bass", "other", "vocals"];

#[derive(Deserialize)]
struct ProfileEvent {
    cat: Option<String>,
    ph: Option<String>,
    name: Option<String>,
    args: Option<ProfileArgs>,
}

#[derive(Deserialize)]
struct ProfileArgs {
    provider: Option<String>,
    op_name: Option<String>,
}

pub fn kernel_events(
    prefix: &Path,
    profile: Option<&str>,
    context: &Context,
) -> Result<usize, String> {
    let path = Path::new(profile.ok_or("missing finalized BS CUDA execution profile")?);
    if path.parent() != prefix.parent() {
        return Err("BS CUDA execution profile is outside the owned directory".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| error.to_string())?;
    let file = context.file(file, "bs.profile.close");
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("BS CUDA execution profile is not an ordinary file".into());
    }
    let mut result = count_kernels(&file, context);
    flitzis_looper_analysis::trial_lifetime::retain_first(&mut result, file.complete());
    result
}

fn count_kernels(file: &OwnedFile, context: &Context) -> Result<usize, String> {
    use serde::de::{Deserializer, SeqAccess, Visitor};
    struct Kernels<'a>(&'a Context);
    impl<'de> Visitor<'de> for Kernels<'_> {
        type Value = usize;
        fn expecting(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            fmt.write_str("a complete ORT event array")
        }
        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<usize, S::Error> {
            let mut kernels = 0;
            while let Some(event) = seq.next_element::<ProfileEvent>()? {
                self.0.check().map_err(serde::de::Error::custom)?;
                if event.cat.as_deref() == Some("Node")
                    && event.ph.as_deref() == Some("X")
                    && event
                        .name
                        .is_some_and(|name| name.ends_with("_kernel_time"))
                    && event.args.is_some_and(|args| {
                        args.provider.as_deref() == Some("CUDAExecutionProvider")
                            && matches!(
                                args.op_name.as_deref(),
                                Some(
                                    "MatMul"
                                        | "FusedMatMul"
                                        | "Gemm"
                                        | "Attention"
                                        | "MultiHeadAttention"
                                )
                            )
                    })
                {
                    kernels += 1;
                }
            }
            Ok(kernels)
        }
    }
    let mut profile = serde_json::Deserializer::from_reader(std::io::BufReader::new(&**file));
    let count = profile
        .deserialize_seq(Kernels(context))
        .map_err(|error| error.to_string())?;
    profile.end().map_err(|error| error.to_string())?;
    if count == 0 {
        Err("no BS CUDA math kernel was observed in the completed model run".into())
    } else {
        Ok(count)
    }
}

pub fn pin_directories(path: &Path, context: &Context) -> Result<Vec<OwnedFile>, String> {
    let mut pins = Vec::new();
    let mut ancestors: Vec<_> = path
        .ancestors()
        .filter(|leaf| leaf.parent().is_some())
        .collect();
    for directory in ancestors.drain(..).rev() {
        context.check()?;
        let handle = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)
            .map_err(|error| format!("pin BS directory {}: {error}", directory.display()))?;
        let file = context.file(handle, "bs.directory.close");
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("BS directory requires an ordinary local path".into());
        }
        pins.push(file);
    }
    Ok(pins)
}

pub fn digest_arg(value: &str) -> Result<&str, String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        Ok(value)
    } else {
        Err("BS SHA-256 must be 64 lowercase hexadecimal digits".into())
    }
}

pub fn verified_file(
    path: &Path,
    expected_extent: Option<usize>,
    expected_sha: &str,
    context: &Context,
) -> Result<OwnedFile, String> {
    context.checkpoint("bs.input.verify")?;
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("open BS input {}: {error}", path.display()))?;
    let mut file = context.file(handle, "bs.input.close");
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let extent = metadata.len();
    if !metadata.is_file()
        || extent == 0
        || expected_extent.is_some_and(|expected| expected as u64 != extent)
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(format!(
            "BS input type or extent differs: {}",
            path.display()
        ));
    }
    if hash_file(&mut file, extent, context)? != expected_sha {
        return Err(format!("BS input SHA-256 differs: {}", path.display()));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    Ok(file)
}

fn hash_file(file: &mut OwnedFile, extent: u64, context: &Context) -> Result<String, String> {
    let mut remaining = extent;
    let mut digest = Sha256::new();
    let mut buffer = [0; BLOCK_BYTES];
    while remaining != 0 {
        context.check()?;
        let count = remaining.min(buffer.len() as u64) as usize;
        let bytes = &mut buffer[..count];
        file.read_exact(bytes).map_err(|error| error.to_string())?;
        digest.update(bytes);
        remaining -= count as u64;
    }
    if file
        .read(&mut buffer[..1])
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("unexpected bytes after BS input/output extent".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn f32_buffer(count: usize, phase: &str) -> Result<Vec<f32>, String> {
    let bytes = count.checked_mul(4).ok_or("BS float32 extent overflow")?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|error| format!("BS host allocation failed: {phase}, {bytes} bytes: {error}"))?;
    values.resize(count, 0.0);
    Ok(values)
}

pub fn read_f32(file: &mut OwnedFile, values: &mut [f32], context: &Context) -> Result<(), String> {
    let mut raw = [0; BLOCK_BYTES];
    for block in values.chunks_mut(raw.len() / 4) {
        context.check()?;
        let bytes = &mut raw[..block.len() * 4];
        file.read_exact(bytes).map_err(|error| error.to_string())?;
        for (dst, bytes) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *dst = f32::from_le_bytes(bytes.try_into().unwrap());
        }
    }
    Ok(())
}

pub fn read_planar(
    file: &mut OwnedFile,
    start: usize,
    frames: usize,
    planar: &mut [f32],
    context: &Context,
) -> Result<usize, String> {
    let position = start
        .checked_mul(FRAME_BYTES)
        .ok_or("BS source offset overflow")?;
    file.seek(SeekFrom::Start(position as u64))
        .map_err(|error| error.to_string())?;
    let mut raw = [0; BLOCK_BYTES];
    let (left, right) = planar.split_at_mut(frames);
    for (left, right) in left
        .chunks_mut(raw.len() / FRAME_BYTES)
        .zip(right.chunks_mut(raw.len() / FRAME_BYTES))
    {
        context.check()?;
        let bytes = &mut raw[..left.len() * FRAME_BYTES];
        file.read_exact(bytes).map_err(|error| error.to_string())?;
        for ((l, r), frame) in left
            .iter_mut()
            .zip(right)
            .zip(bytes.chunks_exact(FRAME_BYTES))
        {
            *l = f32::from_le_bytes(frame[..4].try_into().unwrap());
            *r = f32::from_le_bytes(frame[4..].try_into().unwrap());
        }
    }
    Ok(frames)
}

pub struct TrackOutputs {
    files: Vec<Output>,
    frames: usize,
    written: usize,
}

struct Output {
    file: OwnedFile,
    digest: Sha256,
    extent: u64,
    raw: bool,
}

impl TrackOutputs {
    pub fn new(
        mut create: impl FnMut(&str) -> Result<OwnedFile, String>,
        frames: usize,
        raw: bool,
    ) -> Result<Self, String> {
        let header = wav_header(frames)?;
        let wav_extent = (frames as u64)
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(header.len() as u64))
            .ok_or("BS WAV extent overflow")?;
        let mut files = Vec::with_capacity(NAMES.len() * if raw { 2 } else { 1 });
        for name in NAMES {
            let mut file = create(&format!("{name}.wav"))?;
            file.write_all(&header).map_err(|error| error.to_string())?;
            files.push(Output {
                file,
                digest: Sha256::new_with_prefix(&header),
                extent: wav_extent,
                raw: false,
            });
        }
        if raw {
            for name in NAMES {
                files.push(Output {
                    file: create(&format!("{name}.f32"))?,
                    digest: Sha256::new(),
                    extent: (frames as u64)
                        .checked_mul(8)
                        .ok_or("BS raw extent overflow")?,
                    raw: true,
                });
            }
        }
        Ok(Self {
            files,
            frames,
            written: 0,
        })
    }

    pub fn write(
        &mut self,
        start: usize,
        frames: usize,
        planar: &[f32],
        context: &Context,
    ) -> Result<(), String> {
        if start != self.written || frames > self.frames - self.written {
            return Err("BS output block does not extend the complete track".into());
        }
        for (index, file) in self.files.iter_mut().enumerate() {
            let offset = (index % NAMES.len()) * CHANNELS * frames;
            let (left, right) = planar[offset..offset + CHANNELS * frames].split_at(frames);
            file.write(left, right, context)?;
        }
        self.written += frames;
        Ok(())
    }

    pub fn finish(
        self,
        context: &Context,
    ) -> Result<(serde_json::Value, serde_json::Value), String> {
        if self.written != self.frames {
            return Err("incomplete BS output frame extent".into());
        }
        let mut wav = serde_json::Map::new();
        let mut raw = serde_json::Map::new();
        for (index, file) in self.files.into_iter().enumerate() {
            let destination = if file.raw { &mut raw } else { &mut wav };
            let sha = file.finish(context)?;
            destination.insert(NAMES[index % NAMES.len()].into(), sha.into());
        }
        Ok((wav.into(), raw.into()))
    }

    pub fn required_bytes(frames: usize, raw: bool) -> Result<u64, String> {
        let bytes_per_frame = NAMES.len() as u64 * if raw { 12 } else { 4 };
        (frames as u64)
            .checked_mul(bytes_per_frame)
            .and_then(|bytes| bytes.checked_add((NAMES.len() * 80) as u64))
            .ok_or_else(|| "BS output disk extent overflow".into())
    }
}

impl Output {
    fn write(&mut self, left: &[f32], right: &[f32], context: &Context) -> Result<(), String> {
        let align = if self.raw { 8 } else { 4 };
        let mut raw = [0u8; BLOCK_BYTES];
        for (left, right) in left
            .chunks(raw.len() / align)
            .zip(right.chunks(raw.len() / align))
        {
            context.check()?;
            let bytes = &mut raw[..left.len() * align];
            for ((&l, &r), dst) in left.iter().zip(right).zip(bytes.chunks_exact_mut(align)) {
                if self.raw {
                    dst[..4].copy_from_slice(&l.to_le_bytes());
                    dst[4..].copy_from_slice(&r.to_le_bytes());
                } else {
                    dst[..2].copy_from_slice(&pcm16(l).to_le_bytes());
                    dst[2..].copy_from_slice(&pcm16(r).to_le_bytes());
                }
            }
            self.file
                .write_all(bytes)
                .map_err(|error| error.to_string())?;
            self.digest.update(bytes);
        }
        Ok(())
    }

    fn finish(mut self, context: &Context) -> Result<String, String> {
        context.checkpoint("bs.output.verify")?;
        self.file.sync_all().map_err(|error| error.to_string())?;
        let metadata = self.file.metadata().map_err(|error| error.to_string())?;
        if metadata.len() != self.extent {
            return Err("BS output file extent differs".into());
        }
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        let sha = hash_file(&mut self.file, self.extent, context)?;
        if sha != format!("{:x}", self.digest.finalize()) {
            return Err("BS output contents differ from the complete write".into());
        }
        self.file.complete()?;
        Ok(sha)
    }
}

fn pcm16(value: f32) -> i16 {
    if value <= -1.0 {
        i16::MIN
    } else {
        (value.min(1.0) * 32767.0).round_ties_even() as i16
    }
}

fn wav_header(frames: usize) -> Result<Vec<u8>, String> {
    let bytes = (frames as u64)
        .checked_mul(4)
        .ok_or("BS WAV data extent overflow")?;
    let mut header = Vec::with_capacity(80);
    let riff = (bytes <= u64::from(u32::MAX) - 36).then_some(bytes as u32);
    if let Some(bytes) = riff {
        header.extend(b"RIFF");
        header.extend((bytes + 36).to_le_bytes());
        header.extend(b"WAVE");
    } else {
        let riff_bytes = bytes.checked_add(72).ok_or("BS RF64 extent overflow")?;
        header.extend(b"RF64\xff\xff\xff\xffWAVEds64");
        header.extend(28u32.to_le_bytes());
        header.extend(riff_bytes.to_le_bytes());
        header.extend(bytes.to_le_bytes());
        header.extend((frames as u64).to_le_bytes());
        header.extend(0u32.to_le_bytes());
    }
    header.extend(b"fmt ");
    header.extend(16u32.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(2u16.to_le_bytes());
    header.extend(RATE.to_le_bytes());
    header.extend((RATE * 4).to_le_bytes());
    header.extend(4u16.to_le_bytes());
    header.extend(16u16.to_le_bytes());
    header.extend(b"data");
    header.extend(riff.unwrap_or(u32::MAX).to_le_bytes());
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_is_ties_even_and_preserves_negative_saturation() {
        assert_eq!(pcm16(-2.0), -32768);
        assert_eq!(pcm16(-1.0), -32768);
        assert_eq!(pcm16(1.0), 32767);
        assert_eq!(pcm16(2.0), 32767);
        assert_eq!(pcm16(0.5 / 32767.0), 0);
        assert_eq!(pcm16(-0.5 / 32767.0), 0);
        assert_eq!(pcm16(1.5 / 32767.0), 2);
        assert_eq!(pcm16(-1.5 / 32767.0), -2);
    }

    #[test]
    fn wav_header_switches_at_the_exact_riff_extent() {
        let final_riff = (u32::MAX as usize - 36) / 4;
        let riff = wav_header(final_riff).unwrap();
        assert_eq!(riff.len(), 44);
        assert_eq!(&riff[..4], b"RIFF");
        let first_rf64 = wav_header(final_riff + 1).unwrap();
        assert_eq!(first_rf64.len(), 80);
        assert_eq!(&first_rf64[..4], b"RF64");
        assert_eq!(
            u64::from_le_bytes(first_rf64[36..44].try_into().unwrap()),
            (final_riff + 1) as u64
        );
    }
}
