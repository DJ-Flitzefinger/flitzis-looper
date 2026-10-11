//! Actual RF64 demuxer registered with Symphonia's probe. PCM decoding remains
//! in its pinned codec registry; original headers and u64 extents are preserved.
use std::io::{Read, Seek, SeekFrom};
use symphonia::core::audio::Channels;
use symphonia::core::codecs::*;
use symphonia::core::errors::{Error, Result, SeekErrorKind, end_of_stream_error};
use symphonia::core::formats::{
    Cue, FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo, Track,
};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::{Metadata, MetadataLog};
use symphonia::core::probe::{Descriptor, Instantiate, QueryDescriptor};
use symphonia::core::units::TimeBase;

const PACKET_FRAMES: u64 = 4096;
const MAX_CHUNKS: usize = 4096;

pub(super) struct Rf64Reader {
    source: MediaSourceStream,
    tracks: Vec<Track>,
    metadata: MetadataLog,
    start: u64,
    frames: u64,
    frame_bytes: u64,
    next_frame: u64,
}

fn invalid(message: &'static str) -> Error {
    Error::DecodeError(message)
}
fn u16le(value: &[u8]) -> u16 {
    u16::from_le_bytes(value.try_into().expect("u16 field"))
}
fn u32le(value: &[u8]) -> u32 {
    u32::from_le_bytes(value.try_into().expect("u32 field"))
}
fn u64le(value: &[u8]) -> u64 {
    u64::from_le_bytes(value.try_into().expect("u64 field"))
}

impl QueryDescriptor for Rf64Reader {
    fn query() -> &'static [Descriptor] {
        &[Descriptor {
            short_name: "rf64",
            long_name: "RF64 Waveform Audio File Format",
            extensions: &["wav", "wave", "rf64"],
            mime_types: &["audio/vnd.wave", "audio/wav"],
            markers: &[b"RF64"],
            score: Self::score,
            inst: Instantiate::Format(|source, options| {
                Ok(Box::new(Self::try_new(source, options)?))
            }),
        }]
    }
    fn score(context: &[u8]) -> u8 {
        if context.starts_with(b"RF64") { 255 } else { 0 }
    }
}

fn codec_parameters(bytes: &[u8]) -> Result<(CodecParameters, u64)> {
    if bytes.len() < 16 {
        return Err(invalid("rf64: short fmt chunk"));
    }
    let mut encoding = u16le(&bytes[..2]);
    let channels = usize::from(u16le(&bytes[2..4]));
    let rate = u32le(&bytes[4..8]);
    let byte_rate = u32le(&bytes[8..12]);
    let align = u16le(&bytes[12..14]);
    let bits = u16le(&bytes[14..16]);
    if channels == 0 || channels > 26 || rate == 0 {
        return Err(invalid("rf64: invalid rate/channels"));
    }
    let mut mask = Channels::from_bits_truncate((1_u32 << channels) - 1);
    let mut valid_bits = bits;
    if encoding == 0xfffe {
        if bytes.len() < 40
            || u16le(&bytes[16..18]) < 22
            || bytes[26..40] != [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]
        {
            return Err(invalid("rf64: unsupported extensible fmt"));
        }
        encoding = u16le(&bytes[24..26]);
        valid_bits = u16le(&bytes[18..20]);
        let declared_mask = u32le(&bytes[20..24]);
        if declared_mask != 0 {
            mask = Channels::from_bits_truncate(declared_mask);
        }
    }
    let codec = match (encoding, bits) {
        (1, 8) => CODEC_TYPE_PCM_U8,
        (1, 16) => CODEC_TYPE_PCM_S16LE,
        (1, 24) => CODEC_TYPE_PCM_S24LE,
        (1, 32) => CODEC_TYPE_PCM_S32LE,
        (3, 32) => CODEC_TYPE_PCM_F32LE,
        (3, 64) => CODEC_TYPE_PCM_F64LE,
        _ => {
            return Err(Error::Unsupported(
                "rf64: only supported PCM/IEEE encodings",
            ));
        }
    };
    let expected = (channels as u64)
        .checked_mul(u64::from(bits / 8))
        .ok_or(invalid("rf64: frame size overflow"))?;
    if u64::from(align) != expected
        || mask.count() != channels
        || valid_bits == 0
        || valid_bits > bits
        || u64::from(byte_rate) != u64::from(rate) * expected
    {
        return Err(invalid("rf64: fmt geometry mismatch"));
    }
    let mut params = CodecParameters::new();
    params
        .for_codec(codec)
        .with_sample_rate(rate)
        .with_channels(mask)
        .with_time_base(TimeBase::new(1, rate))
        .with_max_frames_per_packet(PACKET_FRAMES)
        .with_frames_per_block(1);
    // Match the pinned WAV reader's codec parameter policy, while keeping actual
    // RF64 header identity in the decoder descriptor.
    if encoding != 3 || u16le(&bytes[..2]) == 0xfffe {
        params
            .with_bits_per_sample(u32::from(bits))
            .with_bits_per_coded_sample(u32::from(valid_bits));
    }
    Ok((params, expected))
}

impl FormatReader for Rf64Reader {
    fn try_new(mut source: MediaSourceStream, _: &FormatOptions) -> Result<Self> {
        let physical = source
            .byte_len()
            .ok_or(Error::Unsupported("rf64: finite source required"))?;
        let mut header = [0_u8; 12];
        source.read_exact(&mut header)?;
        if &header[..4] != b"RF64" || u32le(&header[4..8]) != u32::MAX || &header[8..] != b"WAVE" {
            return Err(invalid("rf64: missing RF64 WAVE header"));
        }
        let mut riff_end = None;
        let mut data_bytes = None;
        let mut sample_count = None;
        let mut format = None;
        let mut data = None;
        let mut position = 12_u64;
        for _ in 0..MAX_CHUNKS {
            let end = riff_end.unwrap_or(physical);
            if position == end {
                break;
            }
            if position.checked_add(8).is_none_or(|n| n > end) {
                return Err(invalid("rf64: partial chunk header"));
            }
            let mut chunk = [0_u8; 8];
            source.read_exact(&mut chunk)?;
            position += 8;
            let declared = u32le(&chunk[4..]);
            let length = if &chunk[..4] == b"data" && declared == u32::MAX {
                data_bytes.ok_or(invalid("rf64: missing ds64 before data"))?
            } else if declared == u32::MAX {
                return Err(Error::Unsupported(
                    "rf64: sentinel non-data chunk requires ds64 table",
                ));
            } else {
                u64::from(declared)
            };
            let next = position
                .checked_add(length)
                .and_then(|n| n.checked_add(length & 1))
                .ok_or(invalid("rf64: chunk size overflow"))?;
            if next > end {
                return Err(invalid("rf64: chunk exceeds declared extent"));
            }
            match &chunk[..4] {
                b"ds64" => {
                    if riff_end.is_some() || length < 28 || length > 28 + MAX_CHUNKS as u64 * 12 {
                        return Err(invalid("rf64: invalid ds64 chunk"));
                    }
                    let mut dimensions = [0_u8; 28];
                    source.read_exact(&mut dimensions)?;
                    let declared_end = u64le(&dimensions[..8])
                        .checked_add(8)
                        .ok_or(invalid("rf64: riff size overflow"))?;
                    let table = u32le(&dimensions[24..]);
                    if declared_end != physical
                        || declared_end < next
                        || u64::from(table) * 12 + 28 != length
                        || table != 0
                    {
                        return Err(invalid(
                            "rf64: unsupported table or exact RIFF extent mismatch",
                        ));
                    }
                    riff_end = Some(declared_end);
                    data_bytes = Some(u64le(&dimensions[8..16]));
                    sample_count = Some(u64le(&dimensions[16..24]));
                }
                b"fmt " => {
                    if format.is_some() || !(16..=64).contains(&length) {
                        return Err(invalid("rf64: duplicate or oversized fmt"));
                    }
                    let mut bytes = [0_u8; 64];
                    source.read_exact(&mut bytes[..length as usize])?;
                    format = Some(codec_parameters(&bytes[..length as usize])?);
                }
                b"data" => {
                    if data.is_some() || riff_end.is_none() {
                        return Err(invalid("rf64: duplicate or unbound data"));
                    }
                    if data_bytes != Some(length) {
                        return Err(invalid("rf64: data/ds64 extent mismatch"));
                    }
                    data = Some((position, length));
                }
                _ => {}
            }
            source.seek(SeekFrom::Start(next))?;
            position = next;
        }
        if Some(position) != riff_end {
            return Err(invalid("rf64: chunk count or final EOF mismatch"));
        }
        let (mut params, frame_bytes) = format.ok_or(invalid("rf64: missing fmt"))?;
        let (start, length) = data.ok_or(invalid("rf64: missing data"))?;
        if length == 0 || length % frame_bytes != 0 {
            return Err(invalid("rf64: incomplete PCM frames"));
        }
        let frames = length / frame_bytes;
        if sample_count.is_none_or(|count| count != 0 && count != frames) {
            return Err(invalid("rf64: sample count mismatch"));
        }
        params.with_n_frames(frames);
        source.seek(SeekFrom::Start(start))?;
        Ok(Self {
            source,
            tracks: vec![Track::new(0, params)],
            metadata: MetadataLog::default(),
            start,
            frames,
            frame_bytes,
            next_frame: 0,
        })
    }
    fn cues(&self) -> &[Cue] {
        &[]
    }
    fn metadata(&mut self) -> Metadata<'_> {
        self.metadata.metadata()
    }
    fn tracks(&self) -> &[Track] {
        &self.tracks
    }
    fn seek(&mut self, _: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let required = match to {
            SeekTo::TimeStamp { ts, track_id: 0 } => ts,
            _ => return Err(Error::SeekError(SeekErrorKind::InvalidTrack)),
        };
        if required > self.frames {
            return Err(Error::SeekError(SeekErrorKind::OutOfRange));
        }
        self.source
            .seek(SeekFrom::Start(self.start + required * self.frame_bytes))?;
        self.next_frame = required;
        Ok(SeekedTo {
            track_id: 0,
            required_ts: required,
            actual_ts: required,
        })
    }
    fn next_packet(&mut self) -> Result<Packet> {
        let frames = (self.frames - self.next_frame).min(PACKET_FRAMES);
        if frames == 0 {
            return end_of_stream_error();
        }
        let count = usize::try_from(frames * self.frame_bytes)
            .map_err(|_| Error::LimitError("rf64: packet extent"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count)
            .map_err(|_| Error::LimitError("rf64: packet allocation"))?;
        bytes.resize(count, 0);
        self.source.read_exact(&mut bytes)?;
        let packet =
            Packet::new_from_boxed_slice(0, self.next_frame, frames, bytes.into_boxed_slice());
        self.next_frame += frames;
        Ok(packet)
    }
    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.source
    }
}
