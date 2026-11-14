# BS-RoFormer MUSDB18HQ ONNX

These two graph files contain model topology and code-defined constants. All
learned initializers are external ranges of a single locally derived file,
`weights.bsnn`. The initializer directory starts at byte 8, follows the
pinned effective tensor layout, and ends at byte 526,818,456. Neither the
checkpoint nor the effective weights are distributed here. The upstream
release contains the original configuration and checkpoint. See the
[vendored model notice](../../vendor/bs_roformer/NOTICE.md).

| Graph | Bytes | SHA-256 |
| --- | ---: | --- |
| CPU FP32 `bs-mask.onnx` | 516,510 | `1bbb2bfd27e4cb71b6fce2c1cb455afca44b98335ab8de38540e0f750d4a15c9` |
| CUDA AMP `bs-mask-amp.onnx` | 841,582 | `19a9be3bc2e836d06918c7d9877f4def4f8822de3fb69134b0695ebd7afef3d5` |

Both graphs take `features[B,1101,4100]` and write
`masks[B,4,1101,4100]` as float32, for B=1 or B=2. The CUDA graph preserves
mixed-precision boundaries of the pinned Torch CUDA reference. Time attention
splits independent queries into tiles and evaluates each against all keys.
The worker verifies the full graph, external weight file, ORT runtime and
input geometry before inference.

Recreate the two files with [the exporter](../../scripts/export_bs_roformer_onnx.py)
and the [hash-locked tooling](../../scripts/native-inference-requirements.lock).
The `graph` mode emits topology without opening the checkpoint. The `check`
mode verifies each complete graph beside locally derived, pinned weights.
