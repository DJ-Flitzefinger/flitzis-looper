//! Target-clock reads without changing the sealed legacy PCM16 originals.
use super::wav::Pcm16Wav;
use crate::audio_engine::sample_loader::fft_stream::{self, FftPcmStream};
use std::fs::File;
use std::io;

fn io_error(error: crate::audio_engine::errors::SampleLoadError) -> io::Error {
    match error {
        crate::audio_engine::errors::SampleLoadError::Io(error) => error,
        other => io::Error::other(other),
    }
}

/// Original decoder clock captured from the matching immutable cold assignment.
#[derive(Clone, Copy)]
pub(crate) struct SourceClock {
    pub(crate) frames: usize,
    pub(crate) rate: u32,
}

impl SourceClock {
    pub(crate) fn from_lease(
        lease: &crate::audio_engine::cold_store::CommittedColdLease,
        reference: &crate::messages::SampleBuffer,
    ) -> Result<Self, String> {
        lease
            .verify_reference(reference)
            .map_err(|e| e.to_string())?;
        let descriptor = &lease.manifest.descriptor;
        let decoder = &descriptor["decoder"]["pcm"];
        let transform = &descriptor["playback"]["transform"];
        let frames = decoder["full_frames"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("original decoder extent missing")?;
        let rate = decoder["rate_hz"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("original decoder rate missing")?;
        let clock = Self { frames, rate };
        let view = reference
            .residency
            .as_ref()
            .ok_or("bound source descriptor missing")?;
        if frames == 0
            || transform["source_frames"].as_u64() != Some(frames as u64)
            || transform["source_rate_hz"].as_u64() != Some(u64::from(rate))
            || transform["output_frames"].as_u64() != Some(reference.frame_count() as u64)
            || transform["output_rate_hz"].as_u64() != Some(u64::from(view.source.sample_rate_hz))
            || transform["origin_frames"].as_u64() != Some(0)
            || transform["phase"].as_str() != Some("source-frame-zero-after-single-delay-trim-v1")
            || transform["length"].as_str()
                != Some("ceil(input-frames*output-rate/source-rate)-integer-v1")
            || clock.frames_at(view.source.sample_rate_hz)? != reference.frame_count()
        {
            return Err("bound original decoder and playback clocks disagree".into());
        }
        Ok(clock)
    }

    pub(crate) fn frames_at(self, rate: u32) -> Result<usize, String> {
        fft_stream::output_frames(self.frames, self.rate, rate).map_err(|e| e.to_string())
    }
}

pub(super) struct StemPcmReader<'a> {
    file: &'a File,
    layout: Pcm16Wav,
    converter: Option<FftPcmStream>,
}

impl<'a> StemPcmReader<'a> {
    pub(super) fn new(file: &'a File, layout: Pcm16Wav, rate: u32) -> io::Result<Self> {
        let converter = if layout.format.sample_rate_hz == rate {
            None
        } else {
            Some(
                FftPcmStream::new(
                    layout.frames,
                    layout.format.channels,
                    layout.format.sample_rate_hz,
                    rate,
                    crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES,
                )
                .map_err(io_error)?,
            )
        };
        Ok(Self {
            file,
            layout,
            converter,
        })
    }

    pub(super) fn read(
        &mut self,
        first: usize,
        values: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<()> {
        let Self {
            file,
            layout,
            converter,
        } = self;
        match converter {
            None => layout.read_frames(file, first, values, cancelled),
            Some(converter) => converter
                .read(first, values, cancelled, |frame, samples| {
                    layout
                        .read_frames(file, frame, samples, cancelled)
                        .map_err(Into::into)
                })
                .map_err(io_error),
        }
    }

    pub(super) fn finish(&mut self, cancelled: &dyn Fn() -> bool) -> io::Result<()> {
        let Self {
            file,
            layout,
            converter,
        } = self;
        if let Some(converter) = converter {
            converter
                .finish(cancelled, |frame, samples| {
                    layout
                        .read_frames(file, frame, samples, cancelled)
                        .map_err(Into::into)
                })
                .map_err(io_error)?;
        }
        Ok(())
    }
}

pub(super) fn scratch(rate: u32, layout: Pcm16Wav) -> Result<usize, String> {
    if rate == layout.format.sample_rate_hz {
        return Ok(0);
    }
    fft_stream::working_bytes(layout.format.sample_rate_hz, rate, layout.format.channels)
        .map_err(|e| e.to_string())?
        .checked_mul(4)
        .ok_or_else(|| "four legacy FFT cursors exceed scratch arithmetic".into())
}
