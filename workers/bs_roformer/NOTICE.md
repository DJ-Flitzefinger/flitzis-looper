# BS-RoFormer MUSDB18HQ source and model notice

The ONNX graph topology and native inference adapter implement the
BS-RoFormer MUSDB18HQ network from ZFTurbo's
[Music-Source-Separation-Training v1.0.12 release](https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/tag/v1.0.12),
upstream commit `aef04b2e52fb3beaf25e333199f5a7236e628e7b`. The
[original BS-RoFormer implementation](https://github.com/lucidrains/BS-RoFormer)
is by Phil Wang (lucidrains). The included [MIT licenses](LICENSE) credit Roman
Solovyev (2024) and Phil Wang (2023).

The upstream network was defined in `models/bs_roformer/bs_roformer.py`
(SHA-256 `93408c7254c60c48e47be0657a64745065396b0b1c6da4e02c75aca57eb62bf3`)
and `models/bs_roformer/attend.py`
(SHA-256 `0459d799ade55541df2994b0becf7aec12214491360c5a06e346f6d615eaed15`).
The local track adapter follows that revision's `utils.demix_track` geometry
and applies linear fade adjustments to the actual first and last chunks.

The upstream release supplies the
[separately pinned model inputs](../../src/flitzis_looper/controller/bs_roformer_assets.py).
The release has no separate explicit checkpoint license. The tracked ONNX
files carry topology and code-defined constants. Setup obtains the pinned
configuration and checkpoint directly from upstream and derives the learned
weights locally. See the [worker README](README.md) for graph and initializer
identities.
