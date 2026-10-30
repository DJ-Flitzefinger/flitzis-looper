# Design

Use the existing immutable StemGenerationRequest/Result and StemGenerationBackend
protocol. Capture the selected bounded model id at job admission and route it to
an adapter on the background path. Changing Settings affects future jobs only;
existing prepared sets and model-free cache restoration do not depend on selection.
Old projects retain Demucs. The selector is independent of Beat This acceptance.

Pin ZFTurbo Music-Source-Separation-Training release v1.0.12 MUSDB18HQ asset config
and checkpoint, and the corresponding network implementation with attribution.
Explicit installation checks SHA256 before atomically exposing assets. Generation
checks installed identities before deserialization and never accesses the network.
Inference uses bounded overlap-add chunks, not one whole-track GPU tensor. The
same model may use CPU when CUDA is unavailable or fails; never substitute a model.
All five aligned artifacts are produced in the job's private generation directory;
the existing controller alone promotes the complete set and waits for native ACK.

CUDA-enabled Torch wheels include their runtime; driver compatibility and an actual
kernel/model separation must be checked. Two worker/32 queued admission bounds
separator work. BS-RoFormer/shared alignment explicit PCM and existing native
preparation retain 1 GiB/job caps, distinct from model/activations/process RSS.
Unchanged Demucs whole-track neural PCM has no new aggregate memory claim.
Native startup/preparation/assignment limits remain unchanged. Existing old voices,
history, FIFO, filtering, seams and accepted timing remain Rust-owned.
