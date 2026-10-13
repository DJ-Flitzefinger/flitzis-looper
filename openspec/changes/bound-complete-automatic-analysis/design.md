# Complete analysis storage

The immutable complete f32 source stays pinned. At 44100 Hz, channel means are
computed directly into the shared f64 input using the existing f32 stereo arithmetic
before widening. At other loaded rates the same f32 mono and Rubato conversion run
once; those preparation buffers retire before the concurrent pipelines start.
The key frontend accepts the already widened signal without another full conversion.

CQT keeps only one windowed FFT input/output/scratch and magnitude frame. Its
compact octave responses remain complete. Centered zero padding is addressed per
frame, and the first octave borrows the shared input. Decimated octave buffers use
bounded capacities including actual resampler output allowance.

Admission accounts by stage lifetime: preparation and concurrent inference are
separate only where lexical ownership proves that their allocations cannot overlap.
Within each stage all coexisting source, mono, decimation, filter/basis, FFT, compact
response, BPM/downbeat and rate-output allocations are summed. Pinned finite views
are additional. Overflow and excessive FFT geometry reject before allocations.
The analysis bound is distinct from total process RSS and model/ORT workspace;
measured process peaks are reported separately without claiming an ORT memory cap.

Existing source admission, cancellation, stale request fencing and own import ACK
remain authoritative. No failed or superseded analysis publishes invented metadata.

The [allocation and real-import report](../../../docs/automatic-analysis-memory.md)
records the executed bounds, exact input extent, detector outputs and separate
process peaks. Broader musical/model/Human acceptance remains open.
