//! Experimental CPU learned BS network; no Python, Torch, ORT or device runtime.
//!
//! Fixed release schema, all eight axial blocks and all four learned masks. A
//! caller must admit weights, spectral/track buffers and process overhead BEFORE
//! reading/allocating, retain immutable custody through return, and invalidate
//! every private output on any later error. This is not productive provenance,
//! CUDA/AMP, audio-quality, installation or platform acceptance.
//! Derived from the pinned vendored BS-RoFormer (Phil Wang 2023, Roman Solovyev
//! 2024); retain vendor/bs_roformer/LICENSE-LUCIDRAINS and LICENSE-ZFTURBO.
use crate::{
    BINS, Error, Geometry, RATE, STEMS, SpectralWorkspace, Spectrum, checkpoint, finite, reserved,
    zeros,
};
use sha2::{Digest, Sha256};
use std::io::Read;

pub const DIM: usize = 384;
pub const BANDS: usize = 62;
pub const DEPTH: usize = 8;
pub const FEATURES: usize = BINS * 4;
pub const MAX_STEPS: usize = crate::MAX_CHUNK_FRAMES / crate::HOP + 1;
pub const MAGIC: &[u8; 8] = b"FLBSNN01";
const HEADS: usize = 8;
const HEAD: usize = 64;
const INNER: usize = HEADS * HEAD;
const QUERY_TILE: usize = 16;
const LINEAR_TILE: usize = 32;

pub fn band_width(band: usize) -> usize {
    match band {
        0..24 => 8,
        24..36 => 16,
        36..44 => 48,
        44..52 => 96,
        52..60 => 192,
        60 => 512,
        61 => 516,
        _ => 0,
    }
}

/// Exact fixed-schema payload size, excluding the eight-byte format header.
pub fn weight_floats() -> usize {
    let bands: usize = (0..BANDS).map(|b| band_width(b) * (DIM + 1) + DIM).sum();
    let attention = HEAD / 2 + DIM + 3 * INNER * DIM + HEADS * (DIM + 1) + DIM * INNER;
    let feedforward = DIM + 4 * DIM * DIM + 4 * DIM + DIM * 4 * DIM + DIM;
    let masks: usize = (0..BANDS)
        .map(|b| 2 * DIM * (DIM + 1) + 2 * band_width(b) * (2 * DIM + 1))
        .sum();
    bands + DEPTH * 2 * (attention + feedforward) + DIM + STEMS * masks
}

pub fn weight_bytes() -> usize {
    MAGIC.len() + weight_floats() * 4
}

/// Conservative vector-payload allowance for ONE sample's learned forward.
/// Includes both axial input/output copies, norms/projections, all masks and
/// per-head tiled attention scratch. NOT allocator/RSS, weights, spectral/PCM,
/// SGEMM internal packing/allocator, process/stack/code, admission or a
/// release-memory measurement.
pub fn activation_bytes(steps: usize) -> Result<usize, Error> {
    validate_steps(steps)?;
    Ok((3 * steps * BANDS * DIM
        + STEMS * steps * FEATURES
        + 16 * steps * DIM
        + HEADS * QUERY_TILE * steps
        + 16 * QUERY_TILE * DIM)
        * 4)
}

fn validate_steps(steps: usize) -> Result<(), Error> {
    if !(1..=MAX_STEPS).contains(&steps) {
        Err(Error::Shape)
    } else {
        Ok(())
    }
}

struct Decoder<'a, R> {
    reader: &'a mut R,
    digest: Sha256,
    cancelled: &'a dyn Fn() -> bool,
}

impl<R: Read> Decoder<'_, R> {
    fn finish(self, expected: &[u8; 32]) -> Result<(), Error> {
        checkpoint(self.cancelled)?;
        let mut tail = [0];
        if self.reader.read(&mut tail).map_err(|_| Error::Source)? != 0 {
            return Err(Error::Source);
        }
        let actual: [u8; 32] = self.digest.finalize().into();
        if actual != *expected {
            return Err(Error::Source);
        }
        checkpoint(self.cancelled)
    }
    fn values(&mut self, len: usize) -> Result<Vec<f32>, Error> {
        checkpoint(self.cancelled)?;
        let mut result = zeros(len, "weights.tensor")?;
        let mut bytes = [0u8; 16_384];
        for chunk in result.chunks_mut(bytes.len() / 4) {
            checkpoint(self.cancelled)?;
            let raw = &mut bytes[..chunk.len() * 4];
            self.reader.read_exact(raw).map_err(|_| Error::Source)?;
            self.digest.update(&*raw);
            for (value, encoded) in chunk.iter_mut().zip(raw.chunks_exact(4)) {
                *value = f32::from_le_bytes(encoded.try_into().map_err(|_| Error::Shape)?);
            }
            finite(chunk, self.cancelled)?;
        }
        Ok(result)
    }
    fn norm(&mut self, width: usize) -> Result<Norm, Error> {
        Ok(Norm(self.values(width)?))
    }
    fn linear(&mut self, input: usize, output: usize, bias: bool) -> Result<Linear, Error> {
        Ok(Linear {
            input,
            output,
            weights: self.values(input * output)?,
            bias: if bias {
                Some(self.values(output)?)
            } else {
                None
            },
        })
    }
    fn transformer(&mut self) -> Result<Transformer, Error> {
        Ok(Transformer {
            attention: Attention {
                frequencies: self.values(HEAD / 2)?,
                norm: self.norm(DIM)?,
                qkv: self.linear(DIM, INNER * 3, false)?,
                gates: self.linear(DIM, HEADS, true)?,
                output: self.linear(INNER, DIM, false)?,
            },
            norm: self.norm(DIM)?,
            expand: self.linear(DIM, 4 * DIM, true)?,
            contract: self.linear(4 * DIM, DIM, true)?,
        })
    }
}

struct Norm(Vec<f32>);
impl Norm {
    /// Only the final exclusively owned axial buffer may use this form. Residual
    /// attention/FF paths still need their original input and use `apply`.
    fn apply_in_place(&self, input: &mut [f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
        if !input.len().is_multiple_of(self.0.len()) {
            return Err(Error::Shape);
        }
        let scale = (self.0.len() as f32).sqrt();
        for row in input.chunks_mut(self.0.len()) {
            checkpoint(cancelled)?;
            // Complete row reduction BEFORE any write, with the exact old f32
            // sum/sqrt/clamp then divide -> scale -> gamma arithmetic sequence.
            let length = row.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            if !length.is_finite() {
                return Err(Error::NonFinite);
            }
            for (x, gamma) in row.iter_mut().zip(&self.0) {
                *x = ((*x / length) * scale) * gamma;
            }
            checkpoint(cancelled)?;
        }
        finite(input, cancelled)?;
        checkpoint(cancelled)
    }

    fn apply(&self, input: &[f32], cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, Error> {
        if !input.len().is_multiple_of(self.0.len()) {
            return Err(Error::Shape);
        }
        let mut output = zeros(input.len(), "learned.norm")?;
        let scale = (self.0.len() as f32).sqrt();
        for (source, target) in input
            .chunks(self.0.len())
            .zip(output.chunks_mut(self.0.len()))
        {
            checkpoint(cancelled)?;
            // F.normalize's L2 norm clamp, NOT sqrt(mean(x*x)+epsilon).
            let length = source.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            if !length.is_finite() {
                return Err(Error::NonFinite);
            }
            for ((x, y), gamma) in source.iter().zip(target).zip(&self.0) {
                *y = ((*x / length) * scale) * gamma;
            }
        }
        finite(&output, cancelled)?;
        Ok(output)
    }
}

struct Linear {
    input: usize,
    output: usize,
    weights: Vec<f32>,
    bias: Option<Vec<f32>>,
}
impl Linear {
    fn apply(
        &self,
        input: &[f32],
        cancelled: &dyn Fn() -> bool,
        phase: &'static str,
    ) -> Result<Vec<f32>, Error> {
        if !input.len().is_multiple_of(self.input) {
            return Err(Error::Shape);
        }
        let rows = input.len() / self.input;
        let mut result = zeros(rows * self.output, phase)?;
        for start in (0..rows).step_by(LINEAR_TILE) {
            checkpoint(cancelled)?;
            let count = (rows - start).min(LINEAR_TILE);
            multiply(
                &input[start * self.input..(start + count) * self.input],
                &self.weights,
                &mut result[start * self.output..(start + count) * self.output],
                count,
                self.input,
                self.output,
                true,
            )?;
        }
        if let Some(bias) = &self.bias {
            for row in result.chunks_mut(self.output) {
                checkpoint(cancelled)?;
                for (value, b) in row.iter_mut().zip(bias) {
                    *value += b;
                }
            }
        }
        finite(&result, cancelled)?;
        Ok(result)
    }
}

/// Only unsafe boundary: exact contiguous extents, positive bounded strides,
/// nonaliasing immutable inputs and exclusive initialized output are checked here.
fn multiply(
    a: &[f32],
    b: &[f32],
    c: &mut [f32],
    rows: usize,
    inner: usize,
    columns: usize,
    transpose_b: bool,
) -> Result<(), Error> {
    if rows == 0
        || inner == 0
        || columns == 0
        || a.len() != rows.checked_mul(inner).ok_or(Error::Shape)?
        || b.len() != inner.checked_mul(columns).ok_or(Error::Shape)?
        || c.len() != rows.checked_mul(columns).ok_or(Error::Shape)?
        || [rows, inner, columns]
            .iter()
            .any(|&n| n > isize::MAX as usize)
    {
        return Err(Error::Shape);
    }
    let (row_b, col_b) = if transpose_b {
        (1, inner)
    } else {
        (columns, 1)
    };
    // SAFETY: each address is inside the validated slices, C does not overlap
    // either immutable input, beta=0 and all output elements are initialized.
    unsafe {
        matrixmultiply::sgemm(
            rows,
            inner,
            columns,
            1.0,
            a.as_ptr(),
            inner as isize,
            1,
            b.as_ptr(),
            row_b as isize,
            col_b as isize,
            0.0,
            c.as_mut_ptr(),
            columns as isize,
            1,
        );
    }
    Ok(())
}

struct Attention {
    frequencies: Vec<f32>,
    norm: Norm,
    qkv: Linear,
    gates: Linear,
    output: Linear,
}
impl Attention {
    fn apply(&self, input: &[f32], cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, Error> {
        let normalized = self.norm.apply(input, cancelled)?;
        let mut qkv = self.qkv.apply(&normalized, cancelled, "attention.qkv")?;
        let gates = self
            .gates
            .apply(&normalized, cancelled, "attention.gates")?;
        drop(normalized);
        rotate(&mut qkv, &self.frequencies, cancelled)?;
        let attended = attend_heads(&qkv, &gates, cancelled)?;
        // All synchronous score/value readers have returned before projection.
        drop(qkv);
        drop(gates);
        self.output.apply(&attended, cancelled, "attention.output")
    }
}

fn rotate(qkv: &mut [f32], frequencies: &[f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if !qkv.len().is_multiple_of(INNER * 3) || frequencies.len() != HEAD / 2 {
        return Err(Error::Shape);
    }
    // rotary-embedding-torch0.3.5: adjacent even/odd pairs, float32 positions
    // times actual checkpoint frequencies; no cache or invented frequency grid.
    for (position, row) in qkv.chunks_mut(INNER * 3).enumerate() {
        checkpoint(cancelled)?;
        for head in 0..HEADS {
            for (pair, frequency) in frequencies.iter().enumerate() {
                let angle = position as f32 * frequency;
                let (sin, cos) = angle.sin_cos();
                for offset in [0, INNER] {
                    let index = offset + head * HEAD + 2 * pair;
                    let (even, odd) = (row[index], row[index + 1]);
                    row[index] = even * cos - odd * sin;
                    row[index + 1] = odd * cos + even * sin;
                }
            }
        }
    }
    finite(qkv, cancelled)
}

fn attend_heads(
    qkv: &[f32],
    gates: &[f32],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<f32>, Error> {
    if !qkv.len().is_multiple_of(INNER * 3) {
        return Err(Error::Shape);
    }
    let length = qkv.len() / (INNER * 3);
    validate_steps(length)?;
    if gates.len() != length * HEADS {
        return Err(Error::Shape);
    }
    finite(qkv, cancelled)?;
    finite(gates, cancelled)?;
    let mut attended = zeros(length * INNER, "attention.attended")?;
    let mut q = zeros(length * HEAD, "attention.query")?;
    let mut k = zeros(length * HEAD, "attention.key")?;
    let mut v = zeros(length * HEAD, "attention.value")?;
    let mut scores = zeros(QUERY_TILE * length, "attention.scores")?;
    let mut tile_output = zeros(QUERY_TILE * HEAD, "attention.tile-output")?;
    for head in 0..HEADS {
        checkpoint(cancelled)?;
        for position in 0..length {
            for (part, target) in [&mut q, &mut k, &mut v].into_iter().enumerate() {
                let start = position * INNER * 3 + part * INNER + head * HEAD;
                target[position * HEAD..(position + 1) * HEAD]
                    .copy_from_slice(&qkv[start..start + HEAD]);
            }
        }
        for start in (0..length).step_by(QUERY_TILE) {
            checkpoint(cancelled)?;
            let count = (length - start).min(QUERY_TILE);
            let score = &mut scores[..count * length];
            multiply(
                &q[start * HEAD..(start + count) * HEAD],
                &k,
                score,
                count,
                HEAD,
                length,
                true,
            )?;
            for row in score.chunks_mut(length) {
                checkpoint(cancelled)?;
                for value in &mut *row {
                    *value *= 0.125;
                }
                finite(row, cancelled)?;
                let maximum = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let mut total = 0.0f32;
                for value in &mut *row {
                    *value = (*value - maximum).exp();
                    total += *value;
                }
                if !total.is_finite() || total <= 0.0 {
                    return Err(Error::NonFinite);
                }
                for value in row {
                    *value /= total;
                }
            }
            let tile = &mut tile_output[..count * HEAD];
            multiply(score, &v, tile, count, length, HEAD, false)?;
            for row in 0..count {
                let gate = sigmoid(gates[(start + row) * HEADS + head]);
                for d in 0..HEAD {
                    attended[(start + row) * INNER + head * HEAD + d] = tile[row * HEAD + d] * gate;
                }
            }
        }
    }
    finite(&attended, cancelled)?;
    Ok(attended)
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn activate(
    values: &mut [f32],
    width: usize,
    function: fn(f32) -> f32,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Error> {
    for row in values.chunks_mut(width) {
        checkpoint(cancelled)?;
        for value in row {
            *value = function(*value);
        }
    }
    finite(values, cancelled)
}

fn gelu(x: f32) -> f32 {
    // Original nn.GELU(), approximate='none', not tanh approximation.
    0.5 * x * (1.0 + libm::erff(x * std::f32::consts::FRAC_1_SQRT_2))
}

struct Transformer {
    attention: Attention,
    norm: Norm,
    expand: Linear,
    contract: Linear,
}
impl Transformer {
    fn apply(&self, input: &mut [f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
        let attention = self.attention.apply(input, cancelled)?;
        add(input, &attention, cancelled)?;
        drop(attention);
        let norm = self.norm.apply(input, cancelled)?;
        let mut expanded = self.expand.apply(&norm, cancelled, "feedforward.expand")?;
        drop(norm);
        activate(&mut expanded, 4 * DIM, gelu, cancelled)?;
        let feedforward = self
            .contract
            .apply(&expanded, cancelled, "feedforward.contract")?;
        drop(expanded);
        add(input, &feedforward, cancelled)
    }
}

fn add(target: &mut [f32], value: &[f32], cancelled: &dyn Fn() -> bool) -> Result<(), Error> {
    if target.len() != value.len() {
        return Err(Error::Shape);
    }
    for (a, b) in target
        .chunks_mut(crate::BLOCK)
        .zip(value.chunks(crate::BLOCK))
    {
        checkpoint(cancelled)?;
        for (x, y) in a.iter_mut().zip(b) {
            *x += y;
        }
    }
    finite(target, cancelled)
}

struct Band {
    norm: Norm,
    projection: Linear,
}
struct Mask {
    expand: Linear,
    contract: Linear,
}
impl Mask {
    fn project(&self, selected: &[f32], cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, Error> {
        let mut hidden = self.expand.apply(selected, cancelled, "mask.expand")?;
        activate(&mut hidden, 2 * DIM, f32::tanh, cancelled)?;
        let projected = self.contract.apply(&hidden, cancelled, "mask.contract")?;
        drop(hidden);
        Ok(projected)
    }
}

/// Complete fixed release learned CPU network, with immutable owned weights.
pub struct Model {
    bands: Vec<Band>,
    blocks: Vec<(Transformer, Transformer)>,
    norm: Norm,
    masks: Vec<Vec<Mask>>,
}

impl Model {
    /// Load exact fixed-shape little-endian float32 export, authenticate ALL bytes
    /// from this borrowed reader, reject finite/shape/hash/short/trailing failures.
    /// The externally supplied digest binds the trial export, not full provenance.
    pub fn load<R: Read>(
        reader: &mut R,
        expected_sha: &[u8; 32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, Error> {
        checkpoint(cancelled)?;
        let mut magic = [0; 8];
        reader.read_exact(&mut magic).map_err(|_| Error::Source)?;
        if &magic != MAGIC {
            return Err(Error::Shape);
        }
        let mut decoder = Decoder {
            reader,
            digest: Sha256::new(),
            cancelled,
        };
        decoder.digest.update(magic);
        let mut bands = reserved(BANDS, "weights.band-metadata")?;
        for b in 0..BANDS {
            bands.push(Band {
                norm: decoder.norm(band_width(b))?,
                projection: decoder.linear(band_width(b), DIM, true)?,
            });
        }
        let mut blocks = reserved(DEPTH, "weights.block-metadata")?;
        for _ in 0..DEPTH {
            blocks.push((decoder.transformer()?, decoder.transformer()?));
        }
        let norm = decoder.norm(DIM)?;
        let mut masks = reserved(STEMS, "weights.mask-metadata")?;
        for _ in 0..STEMS {
            let mut stem = reserved(BANDS, "weights.mask-band-metadata")?;
            for b in 0..BANDS {
                stem.push(Mask {
                    expand: decoder.linear(DIM, DIM * 2, true)?,
                    contract: decoder.linear(DIM * 2, band_width(b) * 2, true)?,
                });
            }
            masks.push(stem);
        }
        decoder.finish(expected_sha)?;
        checkpoint(cancelled)?;
        Ok(Self {
            bands,
            blocks,
            norm,
            masks,
        })
    }

    /// Predict all four complex masks for ONE complete spectral sample.
    /// Feature/mask order is [time,frequency,stereo,real/imag], stem outermost.
    pub fn masks(
        &self,
        features: &[f32],
        steps: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, Error> {
        let x = self.latent(features, steps, cancelled)?;
        let mut result = zeros(STEMS * steps * FEATURES, "learned.masks")?;
        self.project_masks(&x, steps, 0, STEMS, &mut result, cancelled)?;
        Ok(result)
    }

    fn latent(
        &self,
        features: &[f32],
        steps: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, Error> {
        validate_steps(steps)?;
        if features.len() != steps * FEATURES {
            return Err(Error::Shape);
        }
        finite(features, cancelled)?;
        let mut x = zeros(steps * BANDS * DIM, "learned.axial")?;
        let mut offset = 0;
        for (b, band) in self.bands.iter().enumerate() {
            let width = band_width(b);
            let mut selected = zeros(steps * width, "learned.band-input")?;
            for t in 0..steps {
                checkpoint(cancelled)?;
                selected[t * width..(t + 1) * width].copy_from_slice(
                    &features[t * FEATURES + offset..t * FEATURES + offset + width],
                );
            }
            let normalized = band.norm.apply(&selected, cancelled)?;
            drop(selected);
            let projected =
                band.projection
                    .apply(&normalized, cancelled, "learned.band-projection")?;
            drop(normalized);
            for t in 0..steps {
                x[(t * BANDS + b) * DIM..(t * BANDS + b + 1) * DIM]
                    .copy_from_slice(&projected[t * DIM..(t + 1) * DIM]);
            }
            offset += width;
        }
        for (time, frequency) in &self.blocks {
            for band in 0..BANDS {
                let mut selected = zeros(steps * DIM, "learned.time-input")?;
                for t in 0..steps {
                    selected[t * DIM..(t + 1) * DIM].copy_from_slice(
                        &x[(t * BANDS + band) * DIM..(t * BANDS + band + 1) * DIM],
                    );
                }
                time.apply(&mut selected, cancelled)?;
                for t in 0..steps {
                    x[(t * BANDS + band) * DIM..(t * BANDS + band + 1) * DIM]
                        .copy_from_slice(&selected[t * DIM..(t + 1) * DIM]);
                }
            }
            for row in x.chunks_mut(BANDS * DIM) {
                frequency.apply(row, cancelled)?;
            }
        }
        // All synchronous transformer readers have returned. This private owned
        // buffer has no residual/external reader; avoid a second full axial Vec.
        self.norm.apply_in_place(&mut x, cancelled)?;
        Ok(x)
    }

    fn project_masks(
        &self,
        x: &[f32],
        steps: usize,
        first_stem: usize,
        count: usize,
        result: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        validate_steps(steps)?;
        if count == 0
            || first_stem >= STEMS
            || count > STEMS - first_stem
            || x.len() != steps * BANDS * DIM
            || result.len() != count * steps * FEATURES
        {
            return Err(Error::Shape);
        }
        let mut offset = 0;
        for band in 0..BANDS {
            let width = band_width(band);
            let mut selected = zeros(steps * DIM, "learned.mask-input")?;
            for t in 0..steps {
                selected[t * DIM..(t + 1) * DIM]
                    .copy_from_slice(&x[(t * BANDS + band) * DIM..(t * BANDS + band + 1) * DIM]);
            }
            for stem in 0..count {
                let mask = &self.masks[first_stem + stem][band];
                let projected = mask.project(&selected, cancelled)?;
                for t in 0..steps {
                    checkpoint(cancelled)?;
                    for i in 0..width {
                        result[(stem * steps + t) * FEATURES + offset + i] = projected
                            [t * 2 * width + i]
                            * sigmoid(projected[t * 2 * width + width + i]);
                    }
                }
            }
            offset += width;
        }
        finite(result, cancelled)?;
        checkpoint(cancelled)?;
        Ok(())
    }

    /// Entire native STFT -> learned eight-block network -> complex mask/ISTFT.
    pub fn chunk(&self, pcm: &[f32], cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, Error> {
        if !pcm.len().is_multiple_of(crate::CHANNELS) {
            return Err(Error::Shape);
        }
        let g = Geometry::new(pcm.len() / crate::CHANNELS, RATE)?;
        // Retain the original analyze-before-output cancellation/finite gate.
        finite(pcm, cancelled)?;
        let mut output = zeros(STEMS * crate::CHANNELS * g.frames(), "learned.chunk-output")?;
        self.chunk_into(pcm, &mut output, cancelled)?;
        Ok(output)
    }

    /// Complete chunk into private exact planar [stem, stereo, PCM] storage.
    /// One mask buffer is reused only after each inverse reader returns. Axial state
    /// and ONE inverse context remain live throughout; include both in admission.
    /// Any error/cancellation invalidates the ENTIRE destination, even earlier stems.
    pub fn chunk_into(
        &self,
        pcm: &[f32],
        output: &mut [f32],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        self.chunk_into_using(pcm, output, None, cancelled)
    }

    /// Same complete learned path, using the caller's retained spectral state.
    /// The workspace stays exclusively borrowed through the last inverse reader.
    pub fn chunk_into_with_workspace(
        &self,
        pcm: &[f32],
        output: &mut [f32],
        workspace: &mut SpectralWorkspace,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        self.chunk_into_using(pcm, output, Some(workspace), cancelled)
    }

    fn chunk_into_using(
        &self,
        pcm: &[f32],
        output: &mut [f32],
        mut workspace: Option<&mut SpectralWorkspace>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), Error> {
        if !pcm.len().is_multiple_of(crate::CHANNELS) {
            return Err(Error::Shape);
        }
        let g = Geometry::new(pcm.len() / crate::CHANNELS, RATE)?;
        if output.len() != STEMS * crate::CHANNELS * g.frames() {
            return Err(Error::Shape);
        }
        let spectrum = match workspace.as_deref_mut() {
            Some(workspace) => workspace.analyze(pcm, RATE, cancelled)?,
            None => Spectrum::analyze(pcm, RATE, cancelled)?,
        };
        let x = self.latent(spectrum.features(), g.steps(), cancelled)?;
        let mut inverse = match workspace {
            Some(workspace) => workspace.reconstruction(&spectrum, cancelled)?,
            None => spectrum.reconstruction(cancelled)?,
        };
        let mut masks = zeros(g.feature_len(), "learned.stem-mask")?;
        for (stem, target) in output
            .chunks_exact_mut(crate::CHANNELS * g.frames())
            .enumerate()
        {
            checkpoint(cancelled)?;
            self.project_masks(&x, g.steps(), stem, 1, &mut masks, cancelled)?;
            inverse.write_stem(&masks, target, cancelled)?;
            // The final synchronous reader returned before the next overwrite.
        }
        finite(output, cancelled)?;
        checkpoint(cancelled)
    }

    /// Synchronous release batch2 predictor for the existing track core. Each
    /// independent sample runs serially without changing chunk size or attention
    /// extent; original batch order/complete result and last-reader gates remain.
    pub fn track_batch(
        &self,
        pcm: &[f32],
        batch: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, Error> {
        self.track_batch_using(pcm, batch, None, cancelled)
    }

    /// Original complete batch2 with one retained workspace across serial samples.
    pub fn track_batch_with_workspace(
        &self,
        pcm: &[f32],
        batch: usize,
        workspace: &mut SpectralWorkspace,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, Error> {
        self.track_batch_using(pcm, batch, Some(workspace), cancelled)
    }

    fn track_batch_using(
        &self,
        pcm: &[f32],
        batch: usize,
        mut workspace: Option<&mut SpectralWorkspace>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, Error> {
        let frames = crate::track::CHUNK;
        if !(1..=crate::track::BATCH).contains(&batch) || pcm.len() != batch * 2 * frames {
            return Err(Error::Shape);
        }
        Geometry::new(frames, RATE)?;
        finite(pcm, cancelled)?;
        let mut output = zeros(batch * STEMS * 2 * frames, "learned.batch-output")?;
        for (sample, target) in pcm
            .chunks_exact(2 * frames)
            .zip(output.chunks_exact_mut(STEMS * 2 * frames))
        {
            checkpoint(cancelled)?;
            self.chunk_into_using(sample, target, workspace.as_deref_mut(), cancelled)?;
        }
        finite(&output, cancelled)?;
        checkpoint(cancelled)?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn independent_stem_projection_matches_original_band_outer_layout_bits() {
        // Deliberately small synthetic operator weights, NOT a release network
        // execution. Cover all62 band offsets, four distinct heads and tile tail.
        let steps = 33;
        let masks = (0..STEMS)
            .map(|stem| {
                (0..BANDS)
                    .map(|band| {
                        let width = band_width(band);
                        Mask {
                            expand: Linear {
                                input: DIM,
                                output: 2,
                                weights: (0..2 * DIM)
                                    .map(|i| ((i + stem + band) % 11) as f32 * 0.001 - 0.005)
                                    .collect(),
                                bias: Some(vec![0.1 * stem as f32, -0.02 * band as f32]),
                            },
                            contract: Linear {
                                input: 2,
                                output: 2 * width,
                                weights: (0..4 * width)
                                    .map(|i| ((i + 3 * stem + band) % 7) as f32 * 0.01 - 0.03)
                                    .collect(),
                                bias: Some(
                                    (0..2 * width)
                                        .map(|i| (i % 13) as f32 * 0.01 - 0.06)
                                        .collect(),
                                ),
                            },
                        }
                    })
                    .collect()
            })
            .collect();
        let model = Model {
            bands: vec![],
            blocks: vec![],
            norm: Norm(vec![]),
            masks,
        };
        let x: Vec<_> = (0..steps * BANDS * DIM)
            .map(|i| (i % 43) as f32 * 0.013 - 0.27)
            .collect();
        // Original pre-factoring band-outer/stem-inner algorithm, including
        // independent expand/activation/contract calls and complete GLU copy.
        let mut expected = vec![0.; STEMS * steps * FEATURES];
        let mut offset = 0;
        for band in 0..BANDS {
            let width = band_width(band);
            let mut selected = vec![0.; steps * DIM];
            for t in 0..steps {
                selected[t * DIM..(t + 1) * DIM]
                    .copy_from_slice(&x[(t * BANDS + band) * DIM..(t * BANDS + band + 1) * DIM]);
            }
            for stem in 0..STEMS {
                let mask = &model.masks[stem][band];
                let mut hidden = mask
                    .expand
                    .apply(&selected, &|| false, "test.legacy-expand")
                    .unwrap();
                activate(&mut hidden, 2 * DIM, f32::tanh, &|| false).unwrap();
                let projected = mask
                    .contract
                    .apply(&hidden, &|| false, "test.legacy-contract")
                    .unwrap();
                for t in 0..steps {
                    for i in 0..width {
                        expected[(stem * steps + t) * FEATURES + offset + i] = projected
                            [t * 2 * width + i]
                            * sigmoid(projected[t * 2 * width + width + i]);
                    }
                }
            }
            offset += width;
        }
        for (first, count) in [(0, STEMS), (3, 1), (1, 1), (0, 1), (2, 1)] {
            let mut actual = vec![123.; count * steps * FEATURES];
            model
                .project_masks(&x, steps, first, count, &mut actual, &|| false)
                .unwrap();
            assert!(
                actual
                    .iter()
                    .zip(&expected[first * steps * FEATURES..(first + count) * steps * FEATURES])
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
        }
        let mut private = vec![123.; steps * FEATURES];
        assert_eq!(
            model.project_masks(&x, steps, STEMS, 1, &mut private, &|| false),
            Err(Error::Shape)
        );
        assert_eq!(
            model.project_masks(&x, steps, 0, 0, &mut private, &|| false),
            Err(Error::Shape)
        );
        assert_eq!(
            model.project_masks(&x, steps, 0, 1, &mut private[..1], &|| false),
            Err(Error::Shape)
        );
        assert_eq!(
            model.project_masks(&x, steps, 0, 1, &mut private, &|| true),
            Err(Error::Cancelled)
        );
        assert!(private.iter().all(|x| *x == 123.));
        // Malformed destination/PCM reject before touching this partial test model.
        assert_eq!(
            model.chunk_into(&[0.; 2051], &mut private, &|| false),
            Err(Error::Shape)
        );
        assert_eq!(
            model.chunk_into(&[0.; 2050], &mut private[..1], &|| false),
            Err(Error::Shape)
        );
        let mut pcm = vec![0.; 2050];
        pcm[0] = f32::NAN;
        assert_eq!(model.chunk(&pcm, &|| false), Err(Error::NonFinite));
        assert_eq!(model.chunk(&[0.; 2050], &|| true), Err(Error::Cancelled));
        let mut destination = vec![123.; STEMS * 2050];
        assert_eq!(
            model.chunk_into(&pcm, &mut destination, &|| false),
            Err(Error::NonFinite)
        );
        assert!(destination.iter().all(|x| *x == 123.));
    }

    #[test]
    fn matrix_transpose_and_natural_layout_preserve_distinct_rows() {
        let a = [1., 2., 3., 4., 5., 6.];
        let b = [7., 8., 9., 10., 11., 12.];
        let mut result = [0.; 4];
        multiply(&a, &b, &mut result, 2, 3, 2, true).unwrap();
        assert_eq!(result, [50., 68., 122., 167.]);
        multiply(&a, &b, &mut result, 2, 3, 2, false).unwrap();
        assert_eq!(result, [58., 64., 139., 154.]);
        assert_eq!(
            multiply(&a[..5], &b, &mut result, 2, 3, 2, true),
            Err(Error::Shape)
        );
    }
    #[test]
    fn normalization_is_l2_clamp_and_cancellable() {
        let norm = Norm(vec![2., 3.]);
        let actual = norm.apply(&[3., 4., 0., 0., 1e-15, 0.], &|| false).unwrap();
        assert!((actual[0] - 1.2 * 2f32.sqrt()).abs() < 1e-6);
        assert!((actual[1] - 2.4 * 2f32.sqrt()).abs() < 1e-6);
        assert_eq!(&actual[2..4], &[0., 0.]);
        assert!((actual[4] - 0.002 * 2f32.sqrt()).abs() < 1e-9);
        assert_eq!(norm.apply(&[1., 2.], &|| true), Err(Error::Cancelled));
        assert_eq!(norm.apply(&[f32::MAX; 2], &|| false), Err(Error::NonFinite));
    }

    #[test]
    fn final_rms_inplace_matches_owned_bits_and_cancel_is_private() {
        let norm = Norm((0..DIM).map(|i| (i % 7) as f32 * 0.2 - 0.3).collect());
        let mut source = vec![0.0; DIM * 4];
        for i in 0..DIM {
            source[i] = i as f32 * 0.01 - 2.0;
            source[2 * DIM + i] = (i % 9) as f32 * 1e-16;
            source[3 * DIM + i] = (i % 31) as f32 - 15.0;
        }
        let expected = norm.apply(&source, &|| false).unwrap();
        let mut owned = source.clone();
        norm.apply_in_place(&mut owned, &|| false).unwrap();
        for (actual, expected) in owned.iter().zip(&expected) {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        let calls = Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() > 2
        };
        let mut private = source.clone();
        assert_eq!(
            norm.apply_in_place(&mut private, &cancel),
            Err(Error::Cancelled)
        );
        assert_ne!(&private[..DIM], &source[..DIM]);
        assert_eq!(&private[DIM..], &source[DIM..]);
        assert_eq!(
            norm.apply_in_place(&mut [f32::MAX; DIM], &|| false),
            Err(Error::NonFinite)
        );
    }
    #[test]
    fn weight_schema_and_early_rejections_precede_model_payload() {
        assert_eq!((0..BANDS).map(band_width).sum::<usize>(), FEATURES);
        assert_eq!(weight_floats(), 131_704_612);
        assert_eq!(weight_bytes(), 526_818_456);
        assert!(activation_bytes(MAX_STEPS).unwrap() < 1 << 30);
        assert_eq!(activation_bytes(0), Err(Error::Shape));
        assert_eq!(activation_bytes(MAX_STEPS + 1), Err(Error::Shape));
        assert!(matches!(
            Model::load(&mut &b"badmagic"[..], &[0; 32], &|| false),
            Err(Error::Shape)
        ));
        assert!(matches!(
            Model::load(&mut &MAGIC[..], &[0; 32], &|| false),
            Err(Error::Source)
        ));
        assert!(matches!(
            Model::load(&mut &MAGIC[..], &[0; 32], &|| true),
            Err(Error::Cancelled)
        ));
    }
    #[test]
    fn dense_tile_cancel_and_finite_overflow_are_terminal() {
        let linear = Linear {
            input: 2,
            output: 1,
            weights: vec![1., 1.],
            bias: None,
        };
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() > 1
        };
        assert_eq!(
            linear.apply(&[1.; 66], &cancelled, "test.linear"),
            Err(Error::Cancelled)
        );
        assert_eq!(
            linear.apply(&[f32::MAX; 2], &|| false, "test.linear"),
            Err(Error::NonFinite)
        );
    }

    #[test]
    fn actual_decoder_rejects_bad_digest_tail_short_nonfinite_and_mid_load_cancel() {
        let bytes = [1f32.to_le_bytes(), (-2f32).to_le_bytes()].concat();
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let decode = |raw: &[u8], expected: &[u8; 32]| {
            let mut reader = raw;
            let mut decoder = Decoder {
                reader: &mut reader,
                digest: Sha256::new(),
                cancelled: &|| false,
            };
            let values = decoder.values(2)?;
            decoder.finish(expected)?;
            Ok::<_, Error>(values)
        };
        assert_eq!(decode(&bytes, &digest).unwrap(), [1., -2.]);
        assert_eq!(decode(&bytes, &[0; 32]), Err(Error::Source));
        assert_eq!(
            decode(&[bytes.as_slice(), &[0]].concat(), &digest),
            Err(Error::Source)
        );
        assert_eq!(decode(&bytes[..7], &digest), Err(Error::Source));
        assert_eq!(
            decode(&[f32::NAN.to_le_bytes(), [0; 4]].concat(), &digest),
            Err(Error::NonFinite)
        );
        let calls = Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() > 4
        };
        let data = vec![0; 80_000];
        let mut reader = &data[..];
        let mut decoder = Decoder {
            reader: &mut reader,
            digest: Sha256::new(),
            cancelled: &cancel,
        };
        assert_eq!(decoder.values(20_000), Err(Error::Cancelled));
        assert!(reader.len() < data.len());
    }

    #[test]
    fn rotary_pairs_and_all_heads_noncausal_tail_tiles_match_uniform_attention() {
        let length = 17;
        let mut qkv = vec![0.; length * INNER * 3];
        for t in 0..length {
            for h in 0..HEADS {
                for d in 0..HEAD {
                    qkv[t * INNER * 3 + 2 * INNER + h * HEAD + d] = (t * HEADS + h + d) as f32;
                }
            }
        }
        let gates = vec![0.; length * HEADS];
        let actual = attend_heads(&qkv, &gates, &|| false).unwrap();
        for t in 0..length {
            for h in 0..HEADS {
                for d in 0..HEAD {
                    let expected = (8 * HEADS + h + d) as f32 * 0.5;
                    assert!((actual[t * INNER + h * HEAD + d] - expected).abs() < 2e-5);
                }
            }
        }
        let mut pairs = vec![0.; 2 * INNER * 3];
        pairs[INNER * 3] = 1.;
        pairs[INNER * 3 + 1] = 2.;
        pairs[INNER * 3 + INNER] = 3.;
        pairs[INNER * 3 + INNER + 1] = 4.;
        pairs[INNER * 3 + 2 * INNER] = 5.;
        rotate(
            &mut pairs,
            &[std::f32::consts::FRAC_PI_2; HEAD / 2],
            &|| false,
        )
        .unwrap();
        assert!((pairs[INNER * 3] + 2.).abs() < 1e-6);
        assert!((pairs[INNER * 3 + 1] - 1.).abs() < 1e-6);
        assert!((pairs[INNER * 3 + INNER] + 4.).abs() < 1e-6);
        assert!((pairs[INNER * 3 + INNER + 1] - 3.).abs() < 1e-6);
        assert_eq!(pairs[INNER * 3 + 2 * INNER], 5.);
    }

    #[test]
    fn cancellation_inside_attention_gelu_and_mask_activations_is_terminal() {
        let length = 33;
        let qkv = vec![0.; length * INNER * 3];
        let gates = vec![0.; length * HEADS];
        let calls = Cell::new(0);
        // Validation consumes14 checkpoints, then head/tile/16 score rows.
        // This crosses one actual score/softmax/value tile before cancellation.
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() > 34
        };
        assert_eq!(attend_heads(&qkv, &gates, &cancel), Err(Error::Cancelled));
        for (width, function) in [(4 * DIM, gelu as fn(f32) -> f32), (2 * DIM, f32::tanh)] {
            let calls = Cell::new(0);
            let cancel = || {
                calls.set(calls.get() + 1);
                calls.get() > 1
            };
            let mut values = vec![1.; width * 2];
            assert_eq!(
                activate(&mut values, width, function, &cancel),
                Err(Error::Cancelled)
            );
            assert!(values[0] != 1. && values[width] == 1.);
        }
        let mut residual = vec![1.; DIM];
        add(&mut residual, &vec![gelu(1.); DIM], &|| false).unwrap();
        assert!((residual[0] - 1.8413447).abs() < 1e-6);
    }
}
