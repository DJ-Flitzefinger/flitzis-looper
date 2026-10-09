# BS-RoFormer MUSDB18HQ inference subset

This distribution contains only the inference network of ZFTurbo's
Music-Source-Separation-Training release v1.0.12, "BS Roformer MUSDB18HQ":

https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/tag/v1.0.12

Upstream commit: `aef04b2e52fb3beaf25e333199f5a7236e628e7b`.

The following files are copied byte-for-byte without modification:

| Installed file | Upstream path | SHA256 |
| --- | --- | --- |
| `network/bs_roformer.py` | `models/bs_roformer/bs_roformer.py` | `93408c7254c60c48e47be0657a64745065396b0b1c6da4e02c75aca57eb62bf3` |
| `network/attend.py` | `models/bs_roformer/attend.py` | `0459d799ade55541df2994b0becf7aec12214491360c5a06e346f6d615eaed15` |

The MIT licenses of Roman Solovyev (2024) and original implementation author
Phil Wang / lucidrains (2023) accompany this distribution. The upstream README
credits https://github.com/lucidrains/BS-RoFormer as the implementation origin.
No training code, alternate network family, or model checkpoint is vendored.
The release does not provide a separate explicit checkpoint-license file.

The configuration and checkpoint are preinstalled, separately verified runtime
data. Their complete identities are pinned in `model.py`. The YAML has Python
tuple tags; the worker verifies its SHA256 and passes the exact model arguments
explicitly rather than installing the upstream training/YAML machinery.
`models.bs_roformer.attend` is temporarily resolved inside the disposable worker
to retain the network's original import unchanged. This package does not install
a generic top-level `models` package.

The local `audio.py` and `inference.py` adapt the release's `utils.demix_track`
at the same commit. The stereo 44.1 kHz input is decoded/resampled by FFmpeg;
mono is duplicated by the stereo converter. Published parameters remain
485100-frame chunks, two-chunk maximum batches, overlap factor two, CUDA AMP
and no input normalization. CPU execution uses float32 without CUDA AMP.
The network files retain their original Torch SDPA attention configuration.

The adaptation preserves reflection borders, short-tail reflection versus zero
padding, source ordering and float32 weighted overlap-add. Each chunk owns its
linear fade window, so the first/last chunk adjustment cannot mutate future
windows. This intentionally changes the released `utils.demix_track` window
behavior: that code mutates one shared window per inference batch, leaves the
first fade-in enabled for a two-chunk first batch, and removes fade-out from both
chunks in the terminal batch. With batch size one, its first fade-in adjustment
also persists into later chunks. The local worker applies boundary adjustments
only to the actual first/last chunk. Exact numerical parity with the released
overlap-add is therefore not claimed, although the network files and model
identities remain exact. A chunk-dependent prediction regression checks these
windows against independent scalar binary64 normalized overlap-add across
first/middle/last chunks, batch boundaries, short tails and reflected inputs;
the copy-input regressions separately check source origin and complete length.
A fixed-size CPU overlap-add ring writes finalized regions immediately;
neither an entire source nor an entire output set is allocated as live PCM.
The worker rejects non-finite inputs/outputs and uncovered frames instead of
upstream's `nan_to_num`. Outputs are clipped and written as four complete stereo
PCM16 WAVs for the application's common alignment/publication contract.
The application remains responsible for the atomic five-file stem-set marker,
current source version, private generation, retirement leases and native ACK.

`ChunkPlan.transient_pcm_bytes` conservatively counts simultaneous PCM copies,
including CPU/GPU inputs and outputs, ring/counter, read/copy and PCM16 write
buffers, and rejects shapes exceeding the fixed 1 GiB per-job PCM cap before
decoding. It does not describe process RSS, external decoder internals, model
weights/activations, or an aggregate allocator peak. Decode scratch is written
only below the requested private output directory and is removed on success or
failure. Model/runtime downloads are never initiated by this worker.

The small dependency set pins Einops 0.6.1 and Rotary-Embedding-Torch 0.3.5 to
upstream's network stack. Beartype 0.22.9 replaces the original 0.14.1 to support
Python 3.14. Torch is supplied by the application's single configured runtime.
