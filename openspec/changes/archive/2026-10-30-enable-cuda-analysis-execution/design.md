# Design

The legacy CPU project and protocol-one serializers remain byte-identical. A
second known source project pins Python3.12.13, Torch/Torchaudio2.8.0+cu128 and the
same Beat This1.1.0/final0 checkpoint and CPU logmel frontend. Conditional lock
dependencies are evaluated for Windows/AMD64/Python3.12.13, independently of the
App interpreter. The manifest must match this known source project and lock;
an arbitrary self-described manifest cannot authorize a runtime.

Protocol two separates requested `auto` from actual `cpu` or `cuda:0`. The worker
verifies model parameters/buffers and inference input placement and float32,
disables TF32, runs the existing inference and returns CPU logits to the unchanged
minimal postprocessor. A CUDA probe, availability, placement or inference failure
permits at most one fresh CPU model/checkpoint attempt after failed tensor/model
references retire. Frontend, hash, source, protocol, postprocess, cancellation and
parent-timeout failures do not trigger this fallback.

The Python source-map bridge resolves its installed configuration once and
returns the actual encoded request with its bounded protocol-two result. Native
reconstructs the expected request using its own original source geometry,
identity, path and one closed model profile, then validates the echo and actual
execution receipt. Existing cold admission, source lease and actual child/reader
retirement remain the sole pipeline. No audio callback or clock changes occur.

App tempo schema4 gains an optional diagnostic execution union. New analyses
under the proven legacy CPU profile derive a strict CPU-only receipt without
altering protocol-one raw bytes. Existing saved methods without a receipt stay
unknown. CUDA-runtime CPU fallback remains distinguishable from that old profile.
Existing source/request guards govern completion, restore and Copy; receipt data
cannot authorize a map, BPM edit, Native timing or phase ACK.

STEM execution follows the existing typed progress/result and subscriber routes.
Attempted and completed device evidence is separate from stage and survives
Aligning. Completed cache metadata may retain the historical receipt; cached
content without a receipt stays unknown. Move, shared subscribers and Copy use
their original job/content custody, rather than looking up a potentially moved
origin PAD. Device receipts do not alter artifact completion markers or source
fingerprints.

Installation stages and verifies before atomic pointer publication. Delivery is
coordinated with the actual Native capability/hash under the closed-App rule.
Rollback restores the pointer and Native pair together. One active CUDA-capable
worker serves GPU and CPU fallback; the previous CPU installation is temporary
paired rollback, not a second permanent active inference service.
