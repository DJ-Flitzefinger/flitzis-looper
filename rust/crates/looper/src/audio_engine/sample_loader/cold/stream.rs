//! Complete decoder and playback files with bounded packet/FFT/IO storage.
use super::*;
use crate::audio_engine::cold_store::{CompletedPcmArtifact, PcmArtifactWriter};

pub(crate) struct DecodedDiskAudio {
    pub(crate) artifact: CompletedPcmArtifact,
    pub(crate) channels: usize,
    pub(crate) rate_hz: u32,
    pub(crate) frames: usize,
    pub(crate) scratch_reserved_bytes: usize,
}

/// Read the captured original once, preserving decoder packet chronology. The
/// complete decoder PCM is durable on disk; only one packet is owned at a time.
pub(crate) fn decode_audio_snapshot_streamed(
    file: File,
    hint_path: &Path,
    decoder_path: &Path,
    output_rate_hz: u32,
    maximum: usize,
    cancelled: &impl Fn() -> bool,
    mut progress: impl FnMut(SampleLoadProgress),
) -> Result<DecodedDiskAudio, SampleLoadError> {
    check_cancelled(cancelled)?;
    let (mut format, params, mut descriptor) = probe_snapshot(file, hint_path)?;
    let track_id = descriptor.default_track_id;
    let (max_packet_frames, max_packet_channels) = decoder_dimensions(&params)?;
    if max_packet_channels > MAX_CHANNELS {
        return Err(SampleLoadError::InvalidInput(
            "decoder declared channels exceed bound",
        ));
    }
    let workspace = max_packet_frames
        .checked_mul(max_packet_channels)
        .and_then(|n| n.checked_mul(DECODER_WORKING_PLANES + 3))
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(64 * 1024 + COPY_FRAMES * MAX_CHANNELS * 4))
        .ok_or(SampleLoadError::Limit("streaming decoder workspace"))?;
    check_bytes(workspace, maximum)?;
    let mut decoder = get_codecs().make(&params, &DecoderOptions { verify: false })?;
    let mut rate = None;
    let mut channels = None;
    let mut frames = 0_usize;
    let mut writer: Option<PcmArtifactWriter> = None;
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
        if let Some(packet_channels) =
            channels.or_else(|| params.channels.map(|value| value.count()))
        {
            let packet_frames =
                usize::try_from(packet.dur).map_err(|_| SampleLoadError::Limit("packet frames"))?;
            let packet_bytes = packet_frames
                .checked_mul(packet_channels)
                .and_then(|n| n.checked_mul(12))
                .ok_or(SampleLoadError::Limit("packet workspace"))?;
            // The packet can carry invalid declared duration; reject before the
            // decoder uses it as an allocation dimension, independently of EOF.
            check_bytes(sum_bytes(workspace, packet_bytes)?, maximum)?;
        }
        check_cancelled(cancelled)?;
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(DecoderError::DecodeError(_)) => {
                if let Some(known_channels) = channels.filter(|n| *n > 0) {
                    let count = usize::try_from(packet.dur)
                        .map_err(|_| SampleLoadError::Limit("silence frames"))?;
                    let silence = zeroed(COPY_FRAMES * known_channels)?;
                    let mut left = count;
                    while left > 0 {
                        check_cancelled(cancelled)?;
                        let chunk = left.min(COPY_FRAMES);
                        writer
                            .as_mut()
                            .ok_or(SampleLoadError::MissingChannels)?
                            .append(&silence[..chunk * known_channels], cancelled)?;
                        left -= chunk;
                    }
                    frames = frames
                        .checked_add(count)
                        .ok_or(SampleLoadError::Limit("decoded frame extent"))?;
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
        if audio.capacity() > max_packet_frames || channel_count > max_packet_channels {
            return Err(SampleLoadError::Limit(
                "decoder packet exceeds admitted workspace",
            ));
        }
        if writer.is_none() {
            writer = Some(PcmArtifactWriter::create(
                decoder_path,
                spec.rate,
                channel_count,
            )?);
        }
        let count = audio.frames();
        let mut buffer = DecoderBuffer::<f32>::new(audio.capacity() as u64, spec);
        buffer.copy_interleaved_ref(audio);
        writer
            .as_mut()
            .expect("decoder writer")
            .append(buffer.samples(), cancelled)?;
        frames = frames
            .checked_add(count)
            .ok_or(SampleLoadError::Limit("decoded frame extent"))?;
        if let Some(total) = params.n_frames.filter(|n| *n > 0) {
            progress(SampleLoadProgress {
                subtask: SampleLoadSubtask::Decoding,
                resampling_required: spec.rate != output_rate_hz,
                percent: clamp_progress(frames as f32 / total as f32),
            });
        }
    }
    check_cancelled(cancelled)?;
    if frames == 0 {
        return Err(SampleLoadError::NoDecodedFrames);
    }
    let rate_hz = rate.ok_or(SampleLoadError::MissingSampleRate)?;
    let channels = channels.ok_or(SampleLoadError::MissingChannels)?;
    let artifact = writer
        .ok_or(SampleLoadError::NoDecodedFrames)?
        .finish(descriptor.to_json(), cancelled)?;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::Decoding,
        resampling_required: rate_hz != output_rate_hz,
        percent: 1.0,
    });
    Ok(DecodedDiskAudio {
        artifact,
        channels,
        rate_hz,
        frames,
        scratch_reserved_bytes: workspace,
    })
}

fn zeroed(count: usize) -> Result<Vec<f32>, SampleLoadError> {
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(count)
        .map_err(|_| SampleLoadError::Limit("streaming PCM allocation failed"))?;
    samples.resize(count, 0.0);
    Ok(samples)
}

fn read_samples(
    reader: &mut File,
    samples: &mut [f32],
    cancelled: &impl Fn() -> bool,
) -> Result<(), SampleLoadError> {
    let mut bytes = [0_u8; 64 * 1024];
    for chunk in samples.chunks_mut(bytes.len() / 4) {
        check_cancelled(cancelled)?;
        reader.read_exact(&mut bytes[..chunk.len() * 4])?;
        for (destination, encoded) in chunk.iter_mut().zip(bytes.chunks_exact(4)) {
            *destination = f32::from_le_bytes(encoded.try_into().expect("sample bytes"));
            if !destination.is_finite() {
                return Err(SampleLoadError::InvalidInput(
                    "nonfinite streamed resampler input",
                ));
            }
        }
    }
    check_cancelled(cancelled)
}

fn append_mapped(
    writer: &mut PcmArtifactWriter,
    input: &[f32],
    source_channels: usize,
    output_channels: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<(), SampleLoadError> {
    if source_channels == output_channels {
        writer.append(input, cancelled)?;
    } else {
        for chunk in input.chunks(COPY_FRAMES * source_channels) {
            check_cancelled(cancelled)?;
            let mapped = map_channels(chunk.to_vec(), source_channels, output_channels)?;
            writer.append(&mapped, cancelled)?;
        }
    }
    Ok(())
}

/// One continuous pinned FFT state, exact single delay trim and finite tail.
/// Independent full-zero-padded small-fixture oracles remain in cold/tests.rs.
pub(crate) fn prepare_playback_streamed(
    decoded: &mut DecodedDiskAudio,
    path: &Path,
    output_channels: usize,
    output_rate: u32,
    maximum: usize,
    cancelled: &impl Fn() -> bool,
    mut progress: impl FnMut(SampleLoadProgress),
) -> Result<(CompletedPcmArtifact, PlaybackTransform), SampleLoadError> {
    validate_rate_channels(decoded.rate_hz, decoded.channels)?;
    validate_rate_channels(output_rate, output_channels)?;
    if decoded.channels != output_channels
        && !matches!((decoded.channels, output_channels), (1, 2) | (2, 1))
    {
        return Err(SampleLoadError::UnsupportedChannels {
            file_channels: decoded.channels,
            output_channels,
        });
    }
    check_cancelled(cancelled)?;
    let expected = usize::try_from(
        (decoded.frames as u128 * u128::from(output_rate)).div_ceil(u128::from(decoded.rate_hz)),
    )
    .map_err(|_| SampleLoadError::Limit("complete playback frame extent"))?;
    let mut reader = decoded.artifact.reader.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let resampling = decoded.rate_hz != output_rate;
    let mut writer = PcmArtifactWriter::create(path, output_rate, output_channels)?;
    let delay;
    if !resampling {
        let scratch = COPY_FRAMES
            .checked_mul(decoded.channels + output_channels)
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(128 * 1024))
            .ok_or(SampleLoadError::Limit("identity streaming workspace"))?;
        check_bytes(scratch, maximum)?;
        decoded.scratch_reserved_bytes = decoded.scratch_reserved_bytes.max(scratch);
        let mut buffer = zeroed(COPY_FRAMES * decoded.channels)?;
        let mut left = decoded.frames;
        while left > 0 {
            let frames = left.min(COPY_FRAMES);
            read_samples(
                &mut reader,
                &mut buffer[..frames * decoded.channels],
                cancelled,
            )?;
            append_mapped(
                &mut writer,
                &buffer[..frames * decoded.channels],
                decoded.channels,
                output_channels,
                cancelled,
            )?;
            left -= frames;
            progress(SampleLoadProgress {
                subtask: SampleLoadSubtask::Resampling,
                resampling_required: false,
                percent: clamp_progress((decoded.frames - left) as f32 / decoded.frames as f32),
            });
        }
        delay = 0;
    } else {
        let (fft_input, fft_output) = fft_dimensions(decoded.rate_hz, output_rate);
        let workspace = fft_input
            .checked_add(fft_output)
            .and_then(|n| n.checked_mul(decoded.channels))
            .and_then(|n| n.checked_mul(64 * 4))
            .and_then(|n| n.checked_add(128 * 1024))
            .and_then(|n| n.checked_add(COPY_FRAMES * (decoded.channels + output_channels) * 8))
            .ok_or(SampleLoadError::Limit("streaming FFT workspace"))?;
        check_bytes(workspace, maximum)?;
        decoded.scratch_reserved_bytes = decoded.scratch_reserved_bytes.max(workspace);
        let mut converter = Fft::<f32>::new(
            decoded.rate_hz as usize,
            output_rate as usize,
            RESAMPLE_CHUNK_FRAMES,
            1,
            decoded.channels,
            FixedSync::Input,
        )?;
        delay = converter.output_delay();
        let required = expected
            .checked_add(delay)
            .ok_or(SampleLoadError::Limit("delayed playback extent"))?;
        let input_frames = converter.input_frames_max();
        let output_frames = converter.output_frames_max();
        let mut input = zeroed(input_frames * decoded.channels)?;
        let mut output = zeroed(output_frames * decoded.channels)?;
        let mut left = decoded.frames;
        let mut produced_total = 0_usize;
        let mut written = 0_usize;
        let mut tail_left = None;
        progress(SampleLoadProgress {
            subtask: SampleLoadSubtask::Resampling,
            resampling_required: true,
            percent: 0.0,
        });
        while left > 0 || produced_total < required {
            check_cancelled(cancelled)?;
            let needed = converter.input_frames_next();
            let available = needed.min(left);
            if available == 0 {
                let budget = match tail_left {
                    Some(n) => n,
                    None => {
                        tail_call_budget(required - produced_total, decoded.rate_hz, output_rate)
                            .map_err(|_| SampleLoadError::Limit("finite streaming tail budget"))?
                    }
                };
                if budget == 0 {
                    return Err(SampleLoadError::InvalidInput(
                        "streaming resampler tail exceeded finite budget",
                    ));
                }
                tail_left = Some(budget - 1);
            }
            read_samples(
                &mut reader,
                &mut input[..available * decoded.channels],
                cancelled,
            )?;
            let source = InterleavedOwned::new_from(input, decoded.channels, input_frames)
                .map_err(|_| SampleLoadError::InvalidInput("streaming FFT input dimensions"))?;
            let mut destination =
                InterleavedOwned::new_from(output, decoded.channels, output_frames).map_err(
                    |_| SampleLoadError::InvalidInput("streaming FFT output dimensions"),
                )?;
            let indexing = Indexing {
                input_offset: 0,
                output_offset: 0,
                partial_len: (available < needed).then_some(available),
                active_channels_mask: None,
            };
            let (consumed, produced) =
                converter.process_into_buffer(&source, &mut destination, Some(&indexing))?;
            input = source.take_data();
            output = destination.take_data();
            if available == needed && consumed == 0 {
                return Err(SampleLoadError::InvalidInput(
                    "streaming FFT made no progress",
                ));
            }
            left -= available;
            let start = delay.saturating_sub(produced_total).min(produced);
            let count = (produced - start).min(expected - written);
            append_mapped(
                &mut writer,
                &output[start * decoded.channels..(start + count) * decoded.channels],
                decoded.channels,
                output_channels,
                cancelled,
            )?;
            written += count;
            produced_total = produced_total
                .checked_add(produced)
                .ok_or(SampleLoadError::Limit("streaming output extent"))?;
            progress(SampleLoadProgress {
                subtask: SampleLoadSubtask::Resampling,
                resampling_required: true,
                percent: clamp_progress((decoded.frames - left) as f32 / decoded.frames as f32),
            });
        }
        if written != expected {
            return Err(SampleLoadError::InvalidInput(
                "streaming playback length mismatch",
            ));
        }
    }
    check_cancelled(cancelled)?;
    if reader.read(&mut [0_u8; 1])? != 0 {
        return Err(SampleLoadError::InvalidInput(
            "streaming decoder PCM extra EOF",
        ));
    }
    let transform = PlaybackTransform {
        source_rate: decoded.rate_hz,
        output_rate,
        source_channels: decoded.channels,
        output_channels,
        source_frames: decoded.frames,
        output_frames: expected,
        delay_frames: delay,
    };
    let artifact = writer.finish(json!({"processing":"full-buffer-playback-v1"}), cancelled)?;
    progress(SampleLoadProgress {
        subtask: SampleLoadSubtask::ChannelMapping,
        resampling_required: resampling,
        percent: 1.0,
    });
    Ok((artifact, transform))
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
