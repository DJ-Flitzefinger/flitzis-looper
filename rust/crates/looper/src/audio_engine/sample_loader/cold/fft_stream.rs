//! Bounded, continuous, delay-trimmed FFT conversion shared by cold PCM readers.
use super::stream::zeroed;
use super::*;

pub(crate) fn output_frames(
    frames: usize,
    source: u32,
    target: u32,
) -> Result<usize, SampleLoadError> {
    validate_rate_channels(source, 1)?;
    validate_rate_channels(target, 1)?;
    usize::try_from((frames as u128 * u128::from(target)).div_ceil(u128::from(source)))
        .map_err(|_| SampleLoadError::Limit("complete FFT frame extent"))
}

/// Conservative allowance for the pinned FFT plans, interleaved buffers and
/// their process-call adapters. It is independent of the complete file extent.
pub(crate) fn working_bytes(
    source: u32,
    target: u32,
    channels: usize,
) -> Result<usize, SampleLoadError> {
    validate_rate_channels(source, channels)?;
    validate_rate_channels(target, channels)?;
    let (input, output) = fft_dimensions(source, target);
    input
        .checked_add(output)
        .and_then(|n| n.checked_mul(channels))
        .and_then(|n| n.checked_mul(64 * 4))
        .and_then(|n| n.checked_add(128 * 1024))
        .ok_or(SampleLoadError::Limit("streaming FFT workspace"))
}

/// Executed counters, available only after complete input and finite tail flush.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FftCompletion {
    source_frames: usize,
    generated_frames: usize,
    delay_frames: usize,
}

impl FftCompletion {
    pub(crate) fn source_frames(self) -> usize {
        self.source_frames
    }
    pub(crate) fn generated_frames(self) -> usize {
        self.generated_frames
    }
    pub(crate) fn delay_frames(self) -> usize {
        self.delay_frames
    }
}

pub(crate) struct FftPcmStream {
    converter: Fft<f32>,
    input: Vec<f32>,
    output: Vec<f32>,
    source_rate: u32,
    target_rate: u32,
    channels: usize,
    source_frames: usize,
    source_position: usize,
    expected: usize,
    delay: usize,
    required: usize,
    produced: usize,
    generated: usize,
    position: usize,
    available: std::ops::Range<usize>,
    tail_left: Option<usize>,
}

impl FftPcmStream {
    pub(crate) fn new(
        source_frames: usize,
        channels: usize,
        source_rate: u32,
        target_rate: u32,
        maximum: usize,
    ) -> Result<Self, SampleLoadError> {
        check_bytes(working_bytes(source_rate, target_rate, channels)?, maximum)?;
        let expected = output_frames(source_frames, source_rate, target_rate)?;
        let converter = Fft::<f32>::new(
            source_rate as usize,
            target_rate as usize,
            RESAMPLE_CHUNK_FRAMES,
            1,
            channels,
            FixedSync::Input,
        )?;
        let delay = converter.output_delay();
        let required = expected
            .checked_add(delay)
            .ok_or(SampleLoadError::Limit("delayed playback extent"))?;
        let input = zeroed(converter.input_frames_max() * channels)?;
        let output = zeroed(converter.output_frames_max() * channels)?;
        Ok(Self {
            converter,
            input,
            output,
            source_rate,
            target_rate,
            channels,
            source_frames,
            source_position: 0,
            expected,
            delay,
            required,
            produced: 0,
            generated: 0,
            position: 0,
            available: 0..0,
            tail_left: None,
        })
    }

    pub(crate) fn delay(&self) -> usize {
        self.delay
    }

    fn finished(&self) -> bool {
        self.source_position == self.source_frames && self.produced >= self.required
    }

    fn refill(
        &mut self,
        cancelled: &dyn Fn() -> bool,
        read: &mut impl FnMut(usize, &mut [f32]) -> Result<(), SampleLoadError>,
    ) -> Result<(), SampleLoadError> {
        check_cancelled(cancelled)?;
        let needed = self.converter.input_frames_next();
        let count = needed.min(self.source_frames - self.source_position);
        if count == 0 {
            let budget = match self.tail_left {
                Some(value) => value,
                None => tail_call_budget(
                    self.required.saturating_sub(self.produced),
                    self.source_rate,
                    self.target_rate,
                )
                .map_err(|_| SampleLoadError::Limit("finite streaming tail budget"))?,
            };
            if budget == 0 {
                return Err(SampleLoadError::InvalidInput(
                    "streaming resampler tail exceeded finite budget",
                ));
            }
            self.tail_left = Some(budget - 1);
        }
        read(
            self.source_position,
            &mut self.input[..count * self.channels],
        )?;
        if self.input[..count * self.channels]
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(SampleLoadError::InvalidInput(
                "non-finite streaming FFT input",
            ));
        }
        check_cancelled(cancelled)?;
        let input_frames = self.converter.input_frames_max();
        let output_frames = self.converter.output_frames_max();
        let source = InterleavedOwned::new_from(
            std::mem::take(&mut self.input),
            self.channels,
            input_frames,
        )
        .map_err(|_| SampleLoadError::InvalidInput("streaming FFT input dimensions"))?;
        let mut target = InterleavedOwned::new_from(
            std::mem::take(&mut self.output),
            self.channels,
            output_frames,
        )
        .map_err(|_| SampleLoadError::InvalidInput("streaming FFT output dimensions"))?;
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: (count < needed).then_some(count),
            active_channels_mask: None,
        };
        let (consumed, produced) =
            self.converter
                .process_into_buffer(&source, &mut target, Some(&indexing))?;
        self.input = source.take_data();
        self.output = target.take_data();
        if count == needed && consumed == 0 {
            return Err(SampleLoadError::InvalidInput(
                "streaming FFT made no progress",
            ));
        }
        self.source_position += count;
        let start = self.delay.saturating_sub(self.produced).min(produced);
        let length = (produced - start).min(self.expected - self.generated);
        self.available = start * self.channels..(start + length) * self.channels;
        self.generated += length;
        self.produced = self
            .produced
            .checked_add(produced)
            .ok_or(SampleLoadError::Limit("streaming output extent"))?;
        check_cancelled(cancelled)
    }

    /// Requests advance in target frames. Prefixes are discarded after a single
    /// continuous conversion from source zero, never by restarting at a window.
    pub(crate) fn read(
        &mut self,
        first: usize,
        destination: &mut [f32],
        cancelled: &dyn Fn() -> bool,
        mut read: impl FnMut(usize, &mut [f32]) -> Result<(), SampleLoadError>,
    ) -> Result<(), SampleLoadError> {
        check_cancelled(cancelled)?;
        if !destination.len().is_multiple_of(self.channels)
            || first < self.position
            || first
                .checked_add(destination.len() / self.channels)
                .is_none_or(|end| end > self.expected)
        {
            return Err(SampleLoadError::InvalidInput("streaming FFT target region"));
        }
        let mut skip = (first - self.position) * self.channels;
        let mut copied = 0;
        while skip != 0 || copied < destination.len() {
            check_cancelled(cancelled)?;
            if self.available.is_empty() {
                if self.finished() {
                    return Err(SampleLoadError::InvalidInput(
                        "streaming playback length mismatch",
                    ));
                }
                self.refill(cancelled, &mut read)?;
                continue;
            }
            let discarded = skip.min(self.available.len());
            self.available.start += discarded;
            skip -= discarded;
            self.position += discarded / self.channels;
            let count = (destination.len() - copied).min(self.available.len());
            destination[copied..copied + count]
                .copy_from_slice(&self.output[self.available.start..self.available.start + count]);
            self.available.start += count;
            copied += count;
            self.position += count / self.channels;
        }
        check_cancelled(cancelled)
    }

    /// Consume complete input and finite tail even when alignment omits an edge.
    pub(crate) fn finish(
        &mut self,
        cancelled: &dyn Fn() -> bool,
        mut read: impl FnMut(usize, &mut [f32]) -> Result<(), SampleLoadError>,
    ) -> Result<(), SampleLoadError> {
        while !self.finished() {
            self.refill(cancelled, &mut read)?;
        }
        check_cancelled(cancelled)?;
        if self.generated != self.expected {
            return Err(SampleLoadError::InvalidInput(
                "streaming playback length mismatch",
            ));
        }
        Ok(())
    }

    pub(crate) fn finish_completed(
        &mut self,
        cancelled: &dyn Fn() -> bool,
        read: impl FnMut(usize, &mut [f32]) -> Result<(), SampleLoadError>,
    ) -> Result<FftCompletion, SampleLoadError> {
        self.finish(cancelled, read)?;
        Ok(FftCompletion {
            source_frames: self.source_position,
            generated_frames: self.generated,
            delay_frames: self.delay,
        })
    }
}
