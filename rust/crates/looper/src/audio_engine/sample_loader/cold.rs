//! Complete snapshot decode and bounded playback conversion, outside realtime.

use super::{
    SampleLoadProgress, SampleLoadSubtask, append_decode_error_silence, clamp_progress,
    update_decoded_stream_config,
};
use crate::audio_engine::analysis_pcm::fft::{
    RESAMPLE_CHUNK_FRAMES, fft_dimensions, tail_call_budget,
};
use crate::audio_engine::channels::map_channels;
use crate::audio_engine::errors::SampleLoadError;
use crate::messages::SampleBuffer;
use audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use symphonia::core::{
    audio::SampleBuffer as DecoderBuffer,
    codecs::{CODEC_TYPE_ALAC, CodecParameters, DecoderOptions},
    errors::Error as DecoderError,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

const COPY_FRAMES: usize = 4096;
const MAX_CHANNELS: usize = 32;
const FALLBACK_PACKET_FRAMES: usize = 65_536;
const DECODER_WORKING_PLANES: usize = 16;
const MAX_PACKET_ERRORS: usize = 64;
const DECODER_POLICY: &str = "symphonia-0.5.5-gapless-disabled-packet-silence-v1";
const PLAYBACK_POLICY: &str = "rubato-fft-1.0.0-input1024-subchunks1-delay-trim-tail-flush-v2";

pub(crate) struct DecoderDescriptor {
    codec: String,
    container: String,
    default_track_id: u32,
    declared_max_packet_frames: Option<u64>,
    codec_config_sha256: Option<String>,
    codec_block_frames: Option<usize>,
    declared_frames: Option<u64>,
    delay_frames: Option<u32>,
    padding_frames: Option<u32>,
    silenced_frames: u64,
    skipped_packets: u64,
}

impl DecoderDescriptor {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "library": "symphonia", "version": "0.5.5", "codec": self.codec,
            "container": self.container, "default_track_id": self.default_track_id,
            "declared_max_packet_frames": self.declared_max_packet_frames,
            "codec_config_sha256": self.codec_config_sha256,
            "codec_block_frames": self.codec_block_frames,
            "format_options": {"enable_gapless": false},
            "decoder_options": {"verify": false},
            "processing": DECODER_POLICY,
            "origin": "decoder-first-frame-no-gapless-trim-v1",
            "origin_frames": 0,
            "declared_frames": self.declared_frames,
            "declared_delay_frames": self.delay_frames,
            "declared_padding_frames": self.padding_frames,
            "trimmed_delay_frames": 0, "trimmed_padding_frames": 0,
            "packet_error_silence_frames": self.silenced_frames,
            "skipped_unknown_duration_packets": self.skipped_packets,
            "max_consecutive_packet_errors": MAX_PACKET_ERRORS,
        })
    }
}

/// Recognize the executing decoder's complete durable policy without granting
/// a source permit. Warm source adoption additionally probes the real original.
pub(crate) fn validate_decoder_cache_provenance(value: &Value) -> bool {
    let optional_u64 = |key: &str| -> Option<Option<u64>> {
        let field = value.get(key)?;
        if field.is_null() {
            Some(None)
        } else {
            field.as_u64().map(Some)
        }
    };
    let descriptor = (|| -> Option<DecoderDescriptor> {
        let codec = value["codec"].as_str()?;
        if !supported_decoder_codec(codec) {
            return None;
        }
        let container = value["container"].as_str()?;
        if container.is_empty() || container.len() > 8192 {
            return None;
        }
        let config = match value.get("codec_config_sha256")? {
            Value::Null => None,
            Value::String(digest)
                if digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
            {
                Some(digest.clone())
            }
            _ => return None,
        };
        Some(DecoderDescriptor {
            codec: codec.into(),
            container: container.into(),
            default_track_id: u32::try_from(value["default_track_id"].as_u64()?).ok()?,
            declared_max_packet_frames: optional_u64("declared_max_packet_frames")?,
            codec_config_sha256: config,
            codec_block_frames: optional_u64("codec_block_frames")?
                .map(usize::try_from)
                .transpose()
                .ok()?,
            declared_frames: optional_u64("declared_frames")?,
            delay_frames: optional_u64("declared_delay_frames")?
                .map(u32::try_from)
                .transpose()
                .ok()?,
            padding_frames: optional_u64("declared_padding_frames")?
                .map(u32::try_from)
                .transpose()
                .ok()?,
            silenced_frames: value["packet_error_silence_frames"].as_u64()?,
            skipped_packets: value["skipped_unknown_duration_packets"].as_u64()?,
        })
    })();
    descriptor.is_some_and(|descriptor| descriptor.to_json() == *value)
}

fn supported_decoder_codec(codec: &str) -> bool {
    use symphonia::core::codecs as types;
    // CodecType's constructor is private. Match public IDs and then the actual
    // executing registry; an unknown or disabled codec cannot become recognized.
    [
        types::CODEC_TYPE_PCM_S32LE,
        types::CODEC_TYPE_PCM_S32LE_PLANAR,
        types::CODEC_TYPE_PCM_S32BE,
        types::CODEC_TYPE_PCM_S32BE_PLANAR,
        types::CODEC_TYPE_PCM_S24LE,
        types::CODEC_TYPE_PCM_S24LE_PLANAR,
        types::CODEC_TYPE_PCM_S24BE,
        types::CODEC_TYPE_PCM_S24BE_PLANAR,
        types::CODEC_TYPE_PCM_S16LE,
        types::CODEC_TYPE_PCM_S16LE_PLANAR,
        types::CODEC_TYPE_PCM_S16BE,
        types::CODEC_TYPE_PCM_S16BE_PLANAR,
        types::CODEC_TYPE_PCM_S8,
        types::CODEC_TYPE_PCM_S8_PLANAR,
        types::CODEC_TYPE_PCM_U32LE,
        types::CODEC_TYPE_PCM_U32LE_PLANAR,
        types::CODEC_TYPE_PCM_U32BE,
        types::CODEC_TYPE_PCM_U32BE_PLANAR,
        types::CODEC_TYPE_PCM_U24LE,
        types::CODEC_TYPE_PCM_U24LE_PLANAR,
        types::CODEC_TYPE_PCM_U24BE,
        types::CODEC_TYPE_PCM_U24BE_PLANAR,
        types::CODEC_TYPE_PCM_U16LE,
        types::CODEC_TYPE_PCM_U16LE_PLANAR,
        types::CODEC_TYPE_PCM_U16BE,
        types::CODEC_TYPE_PCM_U16BE_PLANAR,
        types::CODEC_TYPE_PCM_U8,
        types::CODEC_TYPE_PCM_U8_PLANAR,
        types::CODEC_TYPE_PCM_F32LE,
        types::CODEC_TYPE_PCM_F32LE_PLANAR,
        types::CODEC_TYPE_PCM_F32BE,
        types::CODEC_TYPE_PCM_F32BE_PLANAR,
        types::CODEC_TYPE_PCM_F64LE,
        types::CODEC_TYPE_PCM_F64LE_PLANAR,
        types::CODEC_TYPE_PCM_F64BE,
        types::CODEC_TYPE_PCM_F64BE_PLANAR,
        types::CODEC_TYPE_PCM_ALAW,
        types::CODEC_TYPE_PCM_MULAW,
        types::CODEC_TYPE_ADPCM_G722,
        types::CODEC_TYPE_ADPCM_G726,
        types::CODEC_TYPE_ADPCM_G726LE,
        types::CODEC_TYPE_ADPCM_MS,
        types::CODEC_TYPE_ADPCM_IMA_WAV,
        types::CODEC_TYPE_ADPCM_IMA_QT,
        types::CODEC_TYPE_VORBIS,
        types::CODEC_TYPE_MP1,
        types::CODEC_TYPE_MP2,
        types::CODEC_TYPE_MP3,
        types::CODEC_TYPE_AAC,
        types::CODEC_TYPE_OPUS,
        types::CODEC_TYPE_SPEEX,
        types::CODEC_TYPE_MUSEPACK,
        types::CODEC_TYPE_ATRAC1,
        types::CODEC_TYPE_ATRAC3,
        types::CODEC_TYPE_ATRAC3PLUS,
        types::CODEC_TYPE_ATRAC9,
        types::CODEC_TYPE_EAC3,
        types::CODEC_TYPE_AC4,
        types::CODEC_TYPE_DCA,
        types::CODEC_TYPE_WMA,
        types::CODEC_TYPE_FLAC,
        types::CODEC_TYPE_WAVPACK,
        types::CODEC_TYPE_MONKEYS_AUDIO,
        types::CODEC_TYPE_ALAC,
        types::CODEC_TYPE_TTA,
    ]
    .into_iter()
    .any(|known| format!("{known:?}") == codec && get_codecs().get_codec(known).is_some())
}

pub(crate) struct DecodedAudio {
    pub(crate) samples: Vec<f32>,
    pub(crate) channels: usize,
    pub(crate) rate_hz: u32,
    pub(crate) decoder: DecoderDescriptor,
}

pub(crate) struct PlaybackTransform {
    source_rate: u32,
    output_rate: u32,
    source_channels: usize,
    output_channels: usize,
    source_frames: usize,
    output_frames: usize,
    delay_frames: usize,
}

impl PlaybackTransform {
    pub(crate) fn to_json(&self) -> Value {
        let mapping = match (self.source_channels, self.output_channels) {
            (left, right) if left == right => "identity-interleaved-f32-v1",
            (1, 2) => "mono-duplicate-f32-v1",
            (2, 1) => "stereo-arithmetic-mean-f32-v1",
            _ => unreachable!("mapping was validated before preparing playback"),
        };
        json!({
            "version": 1, "source_rate_hz": self.source_rate,
            "output_rate_hz": self.output_rate,
            "source_channels": self.source_channels,
            "output_channels": self.output_channels,
            "source_frames": self.source_frames, "output_frames": self.output_frames,
            "origin_frames": 0,
            "resampler": if self.source_rate == self.output_rate {"identity-f32-v1"} else {PLAYBACK_POLICY},
            "trimmed_algorithmic_delay_frames": self.delay_frames,
            "phase": "source-frame-zero-after-single-delay-trim-v1",
            "length": "ceil(input-frames*output-rate/source-rate)-integer-v1",
            "tail": "zero-pad-until-delay-plus-complete-ceiling-length-v1",
            "channel_mapping": mapping,
        })
    }
}

fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<(), SampleLoadError> {
    if cancelled() {
        return Err(SampleLoadError::Cancelled);
    }
    Ok(())
}

fn bytes(samples: usize) -> Result<usize, SampleLoadError> {
    samples
        .checked_mul(size_of::<f32>())
        .ok_or(SampleLoadError::Limit("PCM byte overflow"))
}

fn sum_bytes(left: usize, right: usize) -> Result<usize, SampleLoadError> {
    left.checked_add(right)
        .ok_or(SampleLoadError::Limit("PCM working byte overflow"))
}

fn check_bytes(requested: usize, maximum: usize) -> Result<(), SampleLoadError> {
    if requested > maximum {
        return Err(SampleLoadError::Limit("whole-job transient PCM bytes"));
    }
    Ok(())
}

fn validate_rate_channels(rate: u32, channels: usize) -> Result<(), SampleLoadError> {
    if rate == 0 || !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(SampleLoadError::InvalidInput(
            "rate/channel dimensions out of range",
        ));
    }
    Ok(())
}

fn reserve_exact(
    samples: &mut Vec<f32>,
    length: usize,
    other_bytes: usize,
    maximum: usize,
) -> Result<(), SampleLoadError> {
    let capacity = samples.capacity().max(length);
    // Growth may allocate the new storage before releasing the old Vec.
    let previous_bytes = if length > samples.capacity() {
        bytes(samples.capacity())?
    } else {
        0
    };
    check_bytes(
        sum_bytes(sum_bytes(bytes(capacity)?, previous_bytes)?, other_bytes)?,
        maximum,
    )?;
    samples
        .try_reserve_exact(length.saturating_sub(samples.len()))
        .map_err(|_| SampleLoadError::Limit("PCM allocation failed"))?;
    check_bytes(sum_bytes(bytes(samples.capacity())?, other_bytes)?, maximum)
}

fn decoder_dimensions(params: &CodecParameters) -> Result<(usize, usize), SampleLoadError> {
    let declared_frames = params
        .max_frames_per_packet
        .map(usize::try_from)
        .transpose()
        .map_err(|_| SampleLoadError::Limit("decoder declared packet frames"))?;
    let declared_channels = params
        .channels
        .map(|channels| channels.count())
        .filter(|channels| *channels > 0);
    if params.codec == CODEC_TYPE_ALAC {
        // Symphonia0.5.5 ALAC allocates from its24/48-byte magic cookie during
        // Decoder::make(), independently of max_frames_per_packet. Read these
        // allocation dimensions BEFORE construction, including missing/forged
        // container dimensions. Remaining codec validation stays in Symphonia.
        let cookie = params
            .extra_data
            .as_deref()
            .ok_or(SampleLoadError::InvalidInput(
                "missing ALAC codec configuration",
            ))?;
        if !matches!(cookie.len(), 24 | 48) {
            return Err(SampleLoadError::InvalidInput(
                "invalid ALAC codec configuration length",
            ));
        }
        let frames = u32::from_be_bytes(cookie[..4].try_into().unwrap()) as usize;
        let channels = usize::from(cookie[9]);
        let rate = u32::from_be_bytes(cookie[20..24].try_into().unwrap());
        if frames == 0 || !(1..=8).contains(&channels) || rate == 0 {
            return Err(SampleLoadError::InvalidInput(
                "invalid ALAC allocation dimensions",
            ));
        }
        return Ok((
            frames.max(declared_frames.unwrap_or(0)),
            channels.max(declared_channels.unwrap_or(0)),
        ));
    }
    Ok((
        // Container metadata may understate a corrupt packet's real codec block.
        // Keep the pinned codecs' conservative block ceiling admitted even when
        // max_frames_per_packet is present and smaller. PCM/ADPCM declarations
        // larger than that ceiling still determine their constructor allocation.
        declared_frames.unwrap_or(0).max(FALLBACK_PACKET_FRAMES),
        declared_channels.unwrap_or(MAX_CHANNELS),
    ))
}

fn probe_snapshot(
    mut file: File,
    hint_path: &Path,
) -> Result<(Box<dyn FormatReader>, CodecParameters, DecoderDescriptor), SampleLoadError> {
    file.seek(SeekFrom::Start(0))?;
    let mut header = [0_u8; 16];
    let header_length = file.metadata()?.len().min(header.len() as u64) as usize;
    file.read_exact(&mut header[..header_length])?;
    file.seek(SeekFrom::Start(0))?;
    let container = if header_length >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        "riff-wave-v1".to_string()
    } else if header_length >= 12
        && &header[..4] == b"FORM"
        && matches!(&header[8..12], b"AIFF" | b"AIFC")
    {
        "iff-aiff-v1".to_string()
    } else if header_length >= 4 && &header[..4] == b"fLaC" {
        "flac-v1".to_string()
    } else if header_length >= 4 && &header[..4] == b"OggS" {
        "ogg-v1".to_string()
    } else if header_length >= 8 && &header[4..8] == b"ftyp" {
        "iso-bmff-v1".to_string()
    } else if header_length >= 3
        && (&header[..3] == b"ID3" || (header[0] == 0xff && header[1] & 0xe0 == 0xe0))
    {
        "mpeg-audio-or-id3-v1".to_string()
    } else {
        // Unrecognized containers retain their actual bytes; no filename can
        // claim a decoder/container identity unsupported by the copied header.
        format!("header-v1:{:02x?}", &header[..header_length])
    };
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = hint_path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    // Explicitly retain the old decoder boundary: neither gapless metadata nor
    // assumed acoustic silence trims decoder samples. The descriptor records
    // container-declared delay/padding independently of the actual full extent.
    let format_options = FormatOptions {
        enable_gapless: false,
        ..Default::default()
    };
    let probed = get_probe().format(&hint, mss, &format_options, &MetadataOptions::default())?;
    let format = probed.format;
    let track = format
        .default_track()
        .ok_or(SampleLoadError::NoDefaultTrack)?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let (max_packet_frames, _) = decoder_dimensions(&params)?;
    let descriptor = DecoderDescriptor {
        codec: format!("{:?}", params.codec),
        container,
        default_track_id: track_id,
        declared_max_packet_frames: params.max_frames_per_packet,
        codec_config_sha256: params
            .extra_data
            .as_ref()
            .map(|bytes| format!("{:x}", Sha256::digest(bytes))),
        codec_block_frames: (params.codec == CODEC_TYPE_ALAC).then_some(max_packet_frames),
        declared_frames: params.n_frames,
        delay_frames: params.delay,
        padding_frames: params.padding,
        silenced_frames: 0,
        skipped_packets: 0,
    };
    Ok((format, params, descriptor))
}

/// Warm selection probes the exact immutable original with the executing
/// decoder library. Packet-result counters are checked separately; all declared
/// codec/configuration/options/origin/version fields must match this probe.
pub(crate) fn decoder_cache_selector(
    file: File,
    hint_path: &Path,
) -> Result<Value, SampleLoadError> {
    Ok(probe_snapshot(file, hint_path)?.2.to_json())
}

pub(crate) fn playback_cache_transform(
    source_rate: u32,
    source_channels: usize,
    source_frames: usize,
    output_rate: u32,
    output_channels: usize,
    max_pcm_bytes: usize,
) -> Result<Value, SampleLoadError> {
    validate_rate_channels(source_rate, source_channels)?;
    validate_rate_channels(output_rate, output_channels)?;
    if source_channels != output_channels
        && !matches!((source_channels, output_channels), (1, 2) | (2, 1))
    {
        return Err(SampleLoadError::UnsupportedChannels {
            file_channels: source_channels,
            output_channels,
        });
    }
    let output_frames = usize::try_from(
        (source_frames as u128 * u128::from(output_rate)).div_ceil(u128::from(source_rate)),
    )
    .map_err(|_| SampleLoadError::Limit("warm output frame count"))?;
    let delay_frames = if source_rate == output_rate {
        0
    } else {
        let (input, output) = fft_dimensions(source_rate, output_rate);
        let workspace = input
            .checked_add(output)
            .and_then(|n| n.checked_mul(source_channels))
            .and_then(|n| n.checked_mul(64 * size_of::<f32>()))
            .ok_or(SampleLoadError::Limit("warm FFT workspace"))?;
        check_bytes(workspace, max_pcm_bytes)?;
        Fft::<f32>::new(
            source_rate as usize,
            output_rate as usize,
            RESAMPLE_CHUNK_FRAMES,
            1,
            source_channels,
            FixedSync::Input,
        )?
        .output_delay()
    };
    Ok(PlaybackTransform {
        source_rate,
        output_rate,
        source_channels,
        output_channels,
        source_frames,
        output_frames,
        delay_frames,
    }
    .to_json())
}

/// Decode the supplied immutable snapshot handle; never reopen its source path.
pub(crate) fn decode_audio_snapshot(
    file: File,
    hint_path: &Path,
    output_rate_hz: u32,
    max_pcm_bytes: usize,
    cancelled: &impl Fn() -> bool,
    mut progress: impl FnMut(SampleLoadProgress),
) -> Result<DecodedAudio, SampleLoadError> {
    check_cancelled(cancelled)?;
    let (mut format, params, mut descriptor) = probe_snapshot(file, hint_path)?;
    let track_id = descriptor.default_track_id;
    let (max_packet_frames, max_packet_channels) = decoder_dimensions(&params)?;
    if max_packet_channels > MAX_CHANNELS {
        return Err(SampleLoadError::InvalidInput(
            "decoder declared channels exceed bound",
        ));
    }
    // Supported codecs have bounded packet work (FLAC<=65535, Vorbis<=8192,
    // AAC/MPA fixed blocks; ALAC declares its frame length). Reserve sixteen
    // f32-equivalent complete multichannel planes before decoder construction:
    // this covers decoder-owned PCM/predictor/overlap storage, independently of
    // the converted packet below. Declared huge ALAC blocks fail before make().
    let decoder_workspace = max_packet_frames
        .checked_mul(max_packet_channels)
        .and_then(|samples| samples.checked_mul(DECODER_WORKING_PLANES))
        .ok_or(SampleLoadError::Limit("decoder workspace dimensions"))?;
    let decoder_workspace = bytes(decoder_workspace)?;
    check_bytes(decoder_workspace, max_pcm_bytes)?;
    let mut decoder = get_codecs().make(&params, &DecoderOptions { verify: false })?;
    let mut rate = None;
    let mut channels = None;
    let mut samples = Vec::new();
    let mut packet_errors = 0;
    let resampling = params
        .sample_rate
        .is_some_and(|rate| rate != output_rate_hz);
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::Decoding,
        resampling_required: resampling,
        percent: 0.0,
    });
    loop {
        check_cancelled(cancelled)?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(DecoderError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(DecoderError::DecodeError(_)) => {
                packet_errors += 1;
                descriptor.skipped_packets += 1;
                if packet_errors > MAX_PACKET_ERRORS {
                    break;
                }
                continue;
            }
            Err(error) => return Err(SampleLoadError::Decode(error)),
        };
        packet_errors = 0;
        if packet.track_id() != track_id {
            continue;
        }
        // Bound packet PCM before decoder allocation where the stream supplies
        // its dimensions; recheck actual decoder capacity before f32 conversion.
        if let Some(packet_channels) =
            channels.or_else(|| params.channels.map(|value| value.count()))
        {
            let frames =
                usize::try_from(packet.dur).map_err(|_| SampleLoadError::Limit("packet frames"))?;
            let packet_samples = frames
                .checked_mul(packet_channels)
                .ok_or(SampleLoadError::Limit("packet samples"))?;
            let packet_bytes = bytes(packet_samples)?
                .checked_mul(3)
                .ok_or(SampleLoadError::Limit("packet workspace"))?;
            check_bytes(
                sum_bytes(
                    sum_bytes(bytes(samples.capacity())?, packet_bytes)?,
                    decoder_workspace,
                )?,
                max_pcm_bytes,
            )?;
        }
        check_cancelled(cancelled)?;
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(DecoderError::DecodeError(_)) => {
                if let Some(known_channels) = channels.filter(|channels| *channels > 0) {
                    let frames = usize::try_from(packet.dur)
                        .map_err(|_| SampleLoadError::Limit("silence frames"))?;
                    let count = frames
                        .checked_mul(known_channels)
                        .ok_or(SampleLoadError::Limit("silence samples"))?;
                    let length = samples
                        .len()
                        .checked_add(count)
                        .ok_or(SampleLoadError::Limit("decoded samples"))?;
                    reserve_exact(&mut samples, length, decoder_workspace, max_pcm_bytes)?;
                    for start in (samples.len()..length).step_by(COPY_FRAMES * known_channels) {
                        check_cancelled(cancelled)?;
                        let chunk_frames = ((length - start) / known_channels).min(COPY_FRAMES);
                        append_decode_error_silence(
                            &mut samples,
                            Some(known_channels),
                            chunk_frames as u64,
                        )
                        .ok_or(SampleLoadError::Limit("packet silence extension"))?;
                    }
                    descriptor.silenced_frames = descriptor
                        .silenced_frames
                        .checked_add(packet.dur)
                        .ok_or(SampleLoadError::Limit("silenced frames overflow"))?;
                } else {
                    descriptor.skipped_packets += 1;
                }
                continue;
            }
            Err(error) => return Err(SampleLoadError::Decode(error)),
        };
        let spec = *audio.spec();
        let channel_count = spec.channels.count();
        validate_rate_channels(spec.rate, channel_count)?;
        update_decoded_stream_config(&mut rate, &mut channels, spec.rate, channel_count)?;
        let capacity_samples = audio
            .capacity()
            .checked_mul(channel_count)
            .ok_or(SampleLoadError::Limit("decoded packet capacity"))?;
        if audio.capacity() > max_packet_frames || channel_count > max_packet_channels {
            return Err(SampleLoadError::Limit(
                "decoder packet exceeds admitted workspace",
            ));
        }
        // Decoder f64 planes (worst supported storage) plus converted f32 packet.
        let packet_bytes = bytes(capacity_samples)?
            .checked_mul(3)
            .ok_or(SampleLoadError::Limit("decoded packet workspace"))?;
        let count = audio
            .frames()
            .checked_mul(channel_count)
            .ok_or(SampleLoadError::Limit("decoded packet samples"))?;
        let length = samples
            .len()
            .checked_add(count)
            .ok_or(SampleLoadError::Limit("complete decoded samples"))?;
        reserve_exact(
            &mut samples,
            length,
            sum_bytes(packet_bytes, decoder_workspace)?,
            max_pcm_bytes,
        )?;
        check_cancelled(cancelled)?;
        let mut buffer = DecoderBuffer::<f32>::new(audio.capacity() as u64, spec);
        buffer.copy_interleaved_ref(audio);
        for chunk in buffer.samples().chunks(COPY_FRAMES * channel_count) {
            check_cancelled(cancelled)?;
            if chunk.iter().any(|sample| !sample.is_finite()) {
                return Err(SampleLoadError::InvalidInput("non-finite decoder sample"));
            }
            samples.extend_from_slice(chunk);
        }
        if let Some(total) = params.n_frames.filter(|frames| *frames > 0) {
            progress(SampleLoadProgress {
                subtask: SampleLoadSubtask::Decoding,
                resampling_required: spec.rate != output_rate_hz,
                percent: clamp_progress((samples.len() / channel_count) as f32 / total as f32),
            });
        }
    }
    check_cancelled(cancelled)?;
    if samples.is_empty() {
        return Err(SampleLoadError::NoDecodedFrames);
    }
    let rate_hz = rate.ok_or(SampleLoadError::MissingSampleRate)?;
    let channels = channels.ok_or(SampleLoadError::MissingChannels)?;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::Decoding,
        resampling_required: rate_hz != output_rate_hz,
        percent: 1.0,
    });
    Ok(DecodedAudio {
        samples,
        channels,
        rate_hz,
        decoder: descriptor,
    })
}

/// Preserve complete decoder PCM while deriving complete output-format playback.
pub(crate) fn prepare_playback(
    decoded: &DecodedAudio,
    output_channels: usize,
    output_rate_hz: u32,
    max_pcm_bytes: usize,
    cancelled: &impl Fn() -> bool,
    mut progress: impl FnMut(SampleLoadProgress),
) -> Result<(SampleBuffer, PlaybackTransform), SampleLoadError> {
    check_cancelled(cancelled)?;
    validate_rate_channels(decoded.rate_hz, decoded.channels)?;
    validate_rate_channels(output_rate_hz, output_channels)?;
    if !decoded.samples.len().is_multiple_of(decoded.channels) {
        return Err(SampleLoadError::InvalidInput(
            "incomplete interleaved frame",
        ));
    }
    if decoded.channels != output_channels
        && !matches!((decoded.channels, output_channels), (1, 2) | (2, 1))
    {
        return Err(SampleLoadError::UnsupportedChannels {
            file_channels: decoded.channels,
            output_channels,
        });
    }
    let source_bytes = bytes(decoded.samples.capacity())?;
    let mut input = Vec::new();
    reserve_exact(
        &mut input,
        decoded.samples.len(),
        source_bytes,
        max_pcm_bytes,
    )?;
    for chunk in decoded.samples.chunks(COPY_FRAMES * decoded.channels) {
        check_cancelled(cancelled)?;
        if chunk.iter().any(|sample| !sample.is_finite()) {
            return Err(SampleLoadError::InvalidInput("non-finite decoder sample"));
        }
        input.extend_from_slice(chunk);
    }
    let resampling = decoded.rate_hz != output_rate_hz;
    let available = max_pcm_bytes
        .checked_sub(source_bytes)
        .ok_or(SampleLoadError::Limit("retained decoder PCM"))?;
    let (converted, delay) = resample_bounded(
        input,
        decoded.channels,
        decoded.rate_hz,
        output_rate_hz,
        available,
        cancelled,
        |percent| {
            progress(SampleLoadProgress {
                subtask: SampleLoadSubtask::Resampling,
                resampling_required: resampling,
                percent,
            });
        },
    )?;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::ChannelMapping,
        resampling_required: resampling,
        percent: 0.0,
    });
    let frames = converted.len() / decoded.channels;
    let output_samples = frames
        .checked_mul(output_channels)
        .ok_or(SampleLoadError::Limit("mapped sample count"))?;
    let mut mapped = if decoded.channels == output_channels {
        converted
    } else {
        let mut output = Vec::new();
        let other = sum_bytes(source_bytes, bytes(converted.capacity())?)?;
        reserve_exact(&mut output, output_samples, other, max_pcm_bytes)?;
        for chunk in converted.chunks(COPY_FRAMES * decoded.channels) {
            check_cancelled(cancelled)?;
            // Reuse the authoritative mapping formula on bounded input chunks.
            let chunk_bytes = bytes(chunk.len())?;
            let mapped_chunk_bytes = bytes(chunk.len() / decoded.channels * output_channels)?;
            let workspace = sum_bytes(
                sum_bytes(other, bytes(output.capacity())?)?,
                sum_bytes(chunk_bytes, mapped_chunk_bytes)?,
            )?;
            check_bytes(workspace, max_pcm_bytes)?;
            let chunk = map_channels(chunk.to_vec(), decoded.channels, output_channels)?;
            if chunk.iter().any(|sample| !sample.is_finite()) {
                return Err(SampleLoadError::InvalidInput(
                    "non-finite channel mapping sample",
                ));
            }
            output.extend_from_slice(&chunk);
        }
        // Mapping no longer needs the resampled input. Release it before the
        // separately admitted Vec-to-Arc overlap below.
        drop(converted);
        output
    };
    check_cancelled(cancelled)?;
    // Vec -> boxed slice can reallocate; Arc conversion necessarily overlaps its
    // input. Admit both allocations before either complete-buffer conversion.
    let arc_workspace = sum_bytes(bytes(mapped.capacity())?, bytes(mapped.len())?)?;
    check_bytes(sum_bytes(source_bytes, arc_workspace)?, max_pcm_bytes)?;
    mapped.shrink_to_fit();
    let samples: Arc<[f32]> = Arc::from(mapped.into_boxed_slice());
    #[cfg(test)]
    super::super::c3_observation::owned_pcm(source_bytes + samples.len() * 4);
    check_cancelled(cancelled)?;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::ChannelMapping,
        resampling_required: resampling,
        percent: 1.0,
    });
    let transform = PlaybackTransform {
        source_rate: decoded.rate_hz,
        output_rate: output_rate_hz,
        source_channels: decoded.channels,
        output_channels,
        source_frames: decoded.samples.len() / decoded.channels,
        output_frames: frames,
        delay_frames: delay,
    };
    Ok((
        SampleBuffer {
            residency: None,
            samples,
            channels: output_channels,
        },
        transform,
    ))
}

fn resample_bounded(
    input_samples: Vec<f32>,
    channels: usize,
    src_rate: u32,
    target_rate: u32,
    max_pcm_bytes: usize,
    cancelled: &impl Fn() -> bool,
    mut progress: impl FnMut(f32),
) -> Result<(Vec<f32>, usize), SampleLoadError> {
    check_cancelled(cancelled)?;
    validate_rate_channels(src_rate, channels)?;
    validate_rate_channels(target_rate, channels)?;
    if !input_samples.len().is_multiple_of(channels) {
        return Err(SampleLoadError::InvalidInput("incomplete resampler frame"));
    }
    let input_bytes = bytes(input_samples.capacity())?;
    check_bytes(input_bytes, max_pcm_bytes)?;
    if src_rate == target_rate || input_samples.is_empty() {
        progress(1.0);
        return Ok((input_samples, 0));
    }
    let frames = input_samples.len() / channels;
    let expected =
        usize::try_from((frames as u128 * u128::from(target_rate)).div_ceil(u128::from(src_rate)))
            .map_err(|_| SampleLoadError::Limit("resampler complete frame count"))?;
    let expected_samples = expected
        .checked_mul(channels)
        .ok_or(SampleLoadError::Limit("resampler complete samples"))?;
    check_bytes(
        sum_bytes(input_bytes, bytes(expected_samples)?)?,
        max_pcm_bytes,
    )?;
    let (fft_input, fft_output) = fft_dimensions(src_rate, target_rate);
    // This explicit conservative PCM/workspace allowance covers the pinned FFT
    // planar buffers, complex spectra and coefficient storage before construction.
    let fft_workspace = fft_input
        .checked_add(fft_output)
        .and_then(|value| value.checked_mul(channels))
        .and_then(|value| value.checked_mul(64))
        .and_then(|value| value.checked_mul(size_of::<f32>()))
        .ok_or(SampleLoadError::Limit("FFT workspace byte count"))?;
    check_bytes(
        sum_bytes(
            sum_bytes(input_bytes, bytes(expected_samples)?)?,
            fft_workspace,
        )?,
        max_pcm_bytes,
    )?;
    let mut resampler = Fft::<f32>::new(
        src_rate as usize,
        target_rate as usize,
        RESAMPLE_CHUNK_FRAMES,
        1,
        channels,
        FixedSync::Input,
    )?;
    check_cancelled(cancelled)?;
    let delay = resampler.output_delay();
    let required = expected
        .checked_add(delay)
        .ok_or(SampleLoadError::Limit("delayed output frames"))?;
    let output_frames = required
        .checked_add(resampler.output_frames_max())
        .ok_or(SampleLoadError::Limit("padded output frames"))?;
    let count = output_frames
        .checked_mul(channels)
        .ok_or(SampleLoadError::Limit("padded output samples"))?;
    let other_bytes = sum_bytes(input_bytes, fft_workspace)?;
    let mut output_samples = Vec::new();
    reserve_exact(&mut output_samples, count, other_bytes, max_pcm_bytes)?;
    output_samples.resize(count, 0.0);
    let input = InterleavedOwned::new_from(input_samples, channels, frames)
        .map_err(|_| SampleLoadError::InvalidInput("resampler input dimensions"))?;
    let mut output = InterleavedOwned::new_from(output_samples, channels, output_frames)
        .map_err(|_| SampleLoadError::InvalidInput("resampler output dimensions"))?;
    let mut indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        partial_len: None,
        active_channels_mask: None,
    };
    let mut left = frames;
    let mut produced_total = 0;
    progress(0.0);
    while left >= resampler.input_frames_next() {
        check_cancelled(cancelled)?;
        let (consumed, produced) =
            resampler.process_into_buffer(&input, &mut output, Some(&indexing))?;
        if consumed == 0 {
            return Err(SampleLoadError::InvalidInput(
                "resampler input made no progress",
            ));
        }
        left -= consumed;
        produced_total += produced;
        indexing.input_offset += consumed;
        indexing.output_offset += produced;
        progress(clamp_progress((frames - left) as f32 / frames as f32));
    }
    if left > 0 {
        check_cancelled(cancelled)?;
        indexing.partial_len = Some(left);
        let (_, produced) = resampler.process_into_buffer(&input, &mut output, Some(&indexing))?;
        produced_total += produced;
        indexing.output_offset += produced;
    }
    indexing.partial_len = Some(0);
    let mut tail_calls = tail_call_budget(
        required.saturating_sub(produced_total),
        src_rate,
        target_rate,
    )
    .map_err(|_| SampleLoadError::Limit("finite resampler tail calls"))?;
    while produced_total < required {
        check_cancelled(cancelled)?;
        if tail_calls == 0 {
            return Err(SampleLoadError::InvalidInput(
                "resampler tail exceeded finite budget",
            ));
        }
        tail_calls -= 1;
        let (_, produced) = resampler.process_into_buffer(&input, &mut output, Some(&indexing))?;
        produced_total += produced;
        indexing.output_offset += produced;
    }
    check_cancelled(cancelled)?;
    let mut data = output.take_data();
    for start in (0..expected_samples).step_by(COPY_FRAMES * channels) {
        check_cancelled(cancelled)?;
        let end = (start + COPY_FRAMES * channels).min(expected_samples);
        data.copy_within(delay * channels + start..delay * channels + end, start);
    }
    data.truncate(expected_samples);
    progress(1.0);
    check_cancelled(cancelled)?;
    Ok((data, delay))
}

#[cfg(test)]
mod tests;
