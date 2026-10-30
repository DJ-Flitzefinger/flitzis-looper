# CUDA-capable analysis with actual execution diagnostics

The selected Beat This model currently runs in a locked CPU-only worker even when
the machine has a usable CUDA device. STEM backends already attempt CUDA, but
their actual device is lost between backend completion, alignment and the UI.

Add one known CUDA-capable Beat This runtime with automatic GPU selection and a
single fresh CPU fallback, while preserving the immutable CPU reference runtime.
Carry verified execution metadata through existing source/job ownership and show
it separately from requested policy, availability and stage. Retain historical
records without relabeling them. This changes execution policy, not the estimator,
musical acceptance, map custody or audio timing authority.
