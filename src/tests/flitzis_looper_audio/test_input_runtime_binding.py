"""Hardware-free public MIDI ownership boundary checks.

Loaded-source and scheduled callback races exercise the native in-memory production
fixtures; these checks ensure caller metadata cannot synthesize native authority.
"""

import pytest

from flitzis_looper_audio import AudioEngine, InputRuntimePadBinding


def test_input_runtime_binding_cannot_be_constructed_from_python() -> None:
    with pytest.raises(TypeError):
        InputRuntimePadBinding()


def test_input_runtime_binding_is_unavailable_without_a_loaded_native_source() -> None:
    engine = AudioEngine()
    assert engine.current_input_runtime_pad_binding(0) is None
    with pytest.raises(ValueError, match="out of range"):
        engine.current_input_runtime_pad_binding(216)
    with pytest.raises(OverflowError):
        engine.current_input_runtime_pad_binding(-1)


def test_input_runtime_rejects_caller_metadata_as_a_binding() -> None:
    engine = AudioEngine()
    multi_loop = False
    with pytest.raises(TypeError):
        engine.set_input_runtime_state(
            multi_loop,
            [True] * 216,
            [0.0] * 216,
            [None] * 216,
            [{"pad_id": 0, "authority_revision": 1}] * 216,  # type: ignore[list-item]
        )


def test_input_runtime_fallback_requires_initialized_native_runtime() -> None:
    engine = AudioEngine()
    with pytest.raises(RuntimeError, match="not initialized"):
        engine.trigger_input_runtime_pad(0)
    with pytest.raises(OverflowError):
        engine.trigger_input_runtime_pad(-1)
