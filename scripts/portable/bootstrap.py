"""Run the private portable runtime; import checks never start the GUI/device."""

# Standalone entrypoint copied into each runtime.
import argparse
import ctypes
import importlib
import inspect
import json
import os
import subprocess
import sys
from pathlib import Path


def physical_path(path: Path) -> Path:
    """Compare private origins without Windows extended-path spelling differences."""
    value = str(path.resolve())
    if os.name == "nt":
        if value.startswith("\\\\?\\UNC\\"):
            value = "\\\\" + value[8:]
        elif value.startswith("\\\\?\\"):
            value = value[4:]
    return Path(value)


def configure(root: Path, checkout: Path, runtime: Path | None = None) -> list[object]:
    """Select one checkout and its private interpreter before importing the app."""
    for name in tuple(os.environ):
        if name.startswith("FLITZIS_WORKSPACE") or name in {
            "FLITZIS_NATIVE_EXTENSION_DIR",
            "FLITZIS_BEAT_THIS_ROOT",
            "FLITZIS_RUNTIME_ACTIVE",
            "FLITZIS_RUNTIME_ARGS",
            "FLITZIS_APPLICATION_SOURCE_STAGE",
            "PYTHONSTARTUP",
            "PYTHONUSERBASE",
        }:
            os.environ.pop(name, None)
    runtime = runtime or Path(sys.executable).absolute().parent.parent
    sys.path.insert(0, str(checkout / "src"))
    os.environ["PYTHONPATH"] = str(checkout / "src")
    os.chdir(checkout)
    os.environ["FLITZIS_MODEL_CACHE_DIR"] = str(root / "models/torch/hub/checkpoints")
    os.environ["TORCH_HOME"] = str(root / "models/torch")
    os.environ["HF_HOME"] = str(root / "models/huggingface")
    os.environ["HF_HUB_CACHE"] = str(root / "models/huggingface/hub")
    os.environ["XDG_CACHE_HOME"] = str(root / "cache")
    os.environ["DORA_DIR"] = str(root / "cache/dora")
    os.environ["TMP"] = os.environ["TEMP"] = str(root / "cache/temp")
    for name, relative in {
        "PIP_CACHE_DIR": "cache/pip",
        "UV_CACHE_DIR": "cache/uv",
        "HF_ASSETS_CACHE": "models/huggingface/assets",
        "HUGGINGFACE_HUB_CACHE": "models/huggingface/hub",
        "NUMBA_CACHE_DIR": "cache/numba",
        "CUDA_CACHE_PATH": "cache/cuda",
        "TRITON_CACHE_DIR": "cache/triton",
        "TORCHINDUCTOR_CACHE_DIR": "cache/torchinductor",
        "TORCH_EXTENSIONS_DIR": "cache/torch-extensions",
        "MPLCONFIGDIR": "cache/matplotlib",
        "PYTHONPYCACHEPREFIX": "cache/pycache",
    }.items():
        os.environ[name] = str(root / relative)
    ffmpeg = runtime / "runtime/ffmpeg"
    os.environ["FLITZIS_FFMPEG_DIR"] = str(ffmpeg)
    os.environ["PATH"] = os.pathsep.join(
        [str(ffmpeg), str(runtime / "python"), os.environ.get("PATH", "")],
    )
    # Keep loader directory handles alive until the GUI and workers terminate.
    return [
        os.add_dll_directory(str(path)) for path in [checkout / "src/flitzis_looper_audio", ffmpeg]
    ]


def private_dlls(runtime: Path, checkout: Path) -> dict[str, str]:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetModuleHandleW.argtypes = [ctypes.c_wchar_p]
    kernel.GetModuleHandleW.restype = ctypes.c_void_p
    kernel.GetModuleFileNameW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint]
    dlls = {}
    for name in [
        "rubberband-3.dll",
        "samplerate.dll",
        "sleef.dll",
        "sleefdft.dll",
        "DirectML.dll",
        "MSVCP140.dll",
        "MSVCP140_1.dll",
        "VCOMP140.dll",
        "VCRUNTIME140.dll",
        "VCRUNTIME140_1.dll",
    ]:
        handle = kernel.GetModuleHandleW(name)
        assert handle, f"Runtime DLL not loaded: {name}"
        buffer = ctypes.create_unicode_buffer(32768)
        assert kernel.GetModuleFileNameW(handle, buffer, len(buffer))
        dll = physical_path(Path(buffer.value))
        assert any(
            dll.is_relative_to(physical_path(directory))
            for directory in [runtime, checkout / "src/flitzis_looper_audio"]
        ), f"Nonportable DLL resolution: {dll}"
        dlls[name] = str(dll)
    return dlls


def check_resident_api(engine_type: type) -> None:
    """Reject an incompatible Native before activation without constructing an engine."""
    assert getattr(engine_type, "RESIDENT_PCM_ADMISSION_VERSION", 0) >= 2, (
        "Native automatic residency admission protocol missing; rebuild the Release extension"
    )
    for method in ("set_resident_pcm_budget_mib", "get_resident_pcm_budget_status"):
        assert callable(getattr(engine_type, method, None)), (
            f"Native residency API missing: {method}"
        )
    assert (
        "interest"
        in inspect.signature(
            inspect.getattr_static(engine_type, "request_committed_stem_residency")
        ).parameters
    ), "Native residency API missing independent preload interest"


def check_pad_copy_api(engine_type: type) -> None:
    """Reject stale Copy interfaces before activating the matching Python controls."""
    for method in (
        "capture_pad_copy",
        "prepare_pad_copy",
        "publish_pad_copy",
        "reconcile_pad_copy",
        "finish_pad_copy",
        "cancel_pad_copy",
    ):
        assert callable(getattr(engine_type, method, None)), f"Native Copy API missing: {method}"


def check_pad_bank_api(native_module: object) -> None:
    """Require complete Bank mutation and fresh restart verification before activation."""
    engine = getattr(native_module, "AudioEngine", None)
    for method in (
        "capture_bank_participants",
        "capture_bank_mutation",
        "prepare_bank_mutation",
        "publish_bank_mutation",
        "validate_bank_mutation",
        "reconcile_bank_mutation",
        "finish_bank_mutation",
        "cancel_bank_mutation",
        "capture_recovered_bank",
        "validate_recovered_bank",
        "retry_recovered_bank",
        "release_recovered_bank",
    ):
        assert callable(inspect.getattr_static(engine, method, None)), (
            f"Native Bank API missing: {method}"
        )
    for name, methods in (
        ("BankParticipants", ()),
        ("CapturedBankMutation", ("publication_status",)),
        ("BankMutationAcknowledgement", ()),
        (
            "BankMutationTicket",
            ("publication_status", "acknowledgement", "metadata_json", "committed_stem_tickets"),
        ),
        ("RecoveredBankAcceptance", ("metadata_json",)),
    ):
        native_type = getattr(native_module, name, None)
        assert isinstance(native_type, type), f"Native Bank type missing: {name}"
        for method in methods:
            assert callable(inspect.getattr_static(native_type, method, None)), (
                f"Native Bank API missing: {name}.{method}"
            )
    parameters = inspect.signature(
        inspect.getattr_static(engine, "capture_bank_mutation")
    ).parameters
    assert list(parameters) == ["self", "source_bank", "target_bank", "intents", "participants"], (
        "Native Bank API invalid signature: capture_bank_mutation"
    )
    participant = parameters["participants"]
    assert participant.kind is inspect.Parameter.KEYWORD_ONLY, (
        "Native Bank API invalid original participant keyword"
    )
    assert participant.default is None, "Native Bank API invalid original participant keyword"
    assert list(
        inspect.signature(inspect.getattr_static(engine, "capture_recovered_bank")).parameters
    ) == [
        "self",
        "participants",
        "intents",
    ], "Native Bank API invalid signature: capture_recovered_bank"


def check_pad_pair_hold_api(engine_type: type) -> None:
    """Require the complete opaque Pair/Hold/input interface without running Native work."""
    for method in (
        "_capture_pad_pair",
        "_prepare_pad_pair",
        "_publish_pad_pair",
        "_reconcile_pad_pair",
        "_validate_pad_pair_acknowledgement",
        "_finish_pad_pair",
        "_cancel_pad_pair",
        "capture_recovered_pad_pair",
        "capture_recovered_pad_copy",
        "validate_recovered_pad_copy",
        "release_recovered_pad_copy",
        "validate_recovered_pad_pair",
        "release_recovered_pad_pair",
        "pause_waveform_hold",
        "release_waveform_hold",
        "trigger_captured_input_pad",
        "prepare_captured_input_trigger",
        "trigger_prepared_input_pad",
        "_resolve_pad_feedback",
        "_resolve_loader_event",
    ):
        assert callable(getattr(engine_type, method, None)), (
            f"Native Pair/Hold API missing: {method}"
        )
    source_binding = inspect.signature(
        inspect.getattr_static(engine_type, "_capture_pad_pair")
    ).parameters.get("source_binding")
    assert source_binding is not None, (
        "Native Pair/Hold API missing keyword: _capture_pad_pair.source_binding"
    )
    assert source_binding.kind in {
        inspect.Parameter.POSITIONAL_OR_KEYWORD,
        inspect.Parameter.KEYWORD_ONLY,
    }, "Native Pair/Hold API invalid keyword: _capture_pad_pair.source_binding"
    assert source_binding.default is None, (
        "Native Pair/Hold API invalid default: _capture_pad_pair.source_binding"
    )


def check_timing_completion_api(native_module: object) -> None:
    """Require original-ticket Pending observations before activating timing controls."""
    for class_name, method in (
        ("AudioEngine", "_content_placement_pending"),
        ("AudioEngine", "_content_placement_read_token"),
        ("ConstantTimingTicket", "placement_pending"),
        ("AcceptedTimingRefreshTicket", "placement_pending"),
    ):
        native_type = getattr(native_module, class_name, None)
        assert callable(getattr(native_type, method, None)), (
            f"Native timing completion API missing: {class_name}.{method}"
        )


def check_global_rate_api(engine_type: object) -> None:
    """Require qualified parent control and callback receipts without invoking Native."""
    assert callable(getattr(engine_type, "global_rate_status", None)), (
        "Native rate confirmation API missing: AudioEngine.global_rate_status"
    )
    status = inspect.getattr_static(engine_type, "bpm_lock_status", None)
    assert callable(status), "Native BPM Lock API missing: AudioEngine.bpm_lock_status"
    assert tuple(inspect.signature(status).parameters) == ("self",), (
        "Native BPM Lock API invalid signature: AudioEngine.bpm_lock_status"
    )
    for method, arguments in (
        ("set_bpm_lock", ("self", "enabled", "anchor_binding")),
        ("set_master_period", ("self", "period_seconds", "anchor_binding")),
        ("set_speed_and_master_period", ("self", "speed", "period_seconds", "anchor_binding")),
    ):
        setter = inspect.getattr_static(engine_type, method, None)
        assert callable(setter), f"Native BPM Lock API missing: AudioEngine.{method}"
        parameters = inspect.signature(setter).parameters
        error = f"Native BPM Lock API requires {method} optional keyword-only anchor_binding=None"
        assert tuple(parameters) == arguments, error
        anchor = parameters["anchor_binding"]
        assert anchor.kind is inspect.Parameter.KEYWORD_ONLY, error
        assert anchor.default is None, error
    provenance_version = getattr(engine_type, "GLOBAL_RATE_PROVENANCE_API_VERSION", None)
    provenance_error = "Native master provenance API requires GLOBAL_RATE_PROVENANCE_API_VERSION=1"
    assert type(provenance_version) is int, provenance_error
    assert provenance_version == 1, provenance_error


def check_ordinary_transport_api(native_module: object) -> None:
    """Reflect effective target and ordinary transport ownership before device startup."""
    engine = getattr(native_module, "AudioEngine", None)
    version = getattr(engine, "ORDINARY_TRANSPORT_API_VERSION", None)
    assert type(version) is int, (
        "Native ordinary transport API requires ORDINARY_TRANSPORT_API_VERSION=3"
    )
    assert version == 3, "Native ordinary transport API requires ORDINARY_TRANSPORT_API_VERSION=3"
    getters = {
        "ExplicitPlaybackTarget": (),
        "OrdinaryTransportAdmission": (
            "content_lifetime",
            "transport_revision",
            "requested_paused",
        ),
        "OrdinaryTransportObservation": (
            "sample_id",
            "content_lifetime",
            "transport_revision",
            "observation_sequence",
            "status_kind",
            "requested_effect",
            "outcome",
            "active",
            "ordinary_paused",
        ),
    }
    for name, attributes in getters.items():
        native_type = getattr(native_module, name, None)
        assert isinstance(native_type, type), f"Native ordinary transport API missing type: {name}"
        for attribute in attributes:
            getter = inspect.getattr_static(native_type, attribute, None)
            assert (
                (isinstance(getter, property) and getter.fget is not None and getter.fset is None)
                or inspect.isgetsetdescriptor(getter)
                or inspect.ismemberdescriptor(getter)
            ), f"Native ordinary transport API missing getter: {name}.{attribute}"
    signatures = (
        (engine, "current_explicit_playback_target", ("self",)),
        (engine, "stop_explicit_playback", ("self",)),
        (native_module.ExplicitPlaybackTarget, "current_control_slot", ("self",)),
        (native_module.ExplicitPlaybackTarget, "content_lifetime", ("self",)),
        (native_module.ExplicitPlaybackTarget, "effective_frame", ("self",)),
        (native_module.ExplicitPlaybackTarget, "original_sequence", ("self",)),
        (engine, "pause_sample", ("self", "id")),
        (engine, "resume_sample", ("self", "id")),
        (engine, "_resolve_ordinary_transport_feedback", ("self", "message")),
        (engine, "_current_ordinary_transport_observation", ("self", "id")),
        (native_module.OrdinaryTransportAdmission, "current_control_slot", ("self",)),
        (native_module.OrdinaryTransportAdmission, "current_observation", ("self",)),
        (native_module.OrdinaryTransportObservation, "current_control_slot", ("self",)),
        (native_module.OrdinaryTransportObservation, "current_observation", ("self",)),
        (
            native_module.OrdinaryTransportObservation,
            "matches_admission",
            ("self", "admission"),
        ),
        (
            native_module.OrdinaryTransportObservation,
            "settles_admission",
            ("self", "admission"),
        ),
    )
    for native_type, method, names in signatures:
        implementation = getattr(native_type, method, None)
        assert callable(implementation), f"Native ordinary transport API missing method: {method}"
        parameters = inspect.signature(implementation).parameters
        assert tuple(parameters) == names, (
            f"Native ordinary transport API invalid signature: {method}"
        )
        assert all(
            parameter.kind
            in {
                inspect.Parameter.POSITIONAL_ONLY,
                inspect.Parameter.POSITIONAL_OR_KEYWORD,
            }
            and parameter.default is inspect.Parameter.empty
            for parameter in parameters.values()
        ), f"Native ordinary transport API invalid signature: {method}"
    message_type = getattr(
        getattr(native_module, "AudioMessage", None), "OrdinaryTransportFeedback", None
    )
    assert isinstance(message_type, type), (
        "Native ordinary transport API missing type: AudioMessage.OrdinaryTransportFeedback"
    )
    for attribute in ("owner", "cell", "lifetime", "transport_revision", "observation_sequence"):
        assert inspect.getattr_static(message_type, attribute, None) is not None, (
            f"Native ordinary transport API missing message getter: {attribute}"
        )
    assert callable(getattr(message_type, "feedback_kind", None)), (
        "Native ordinary transport API missing method: feedback_kind"
    )


def check_pitch_action_api(native_module: object) -> None:
    """Require immutable pitch capture and atomic runtime projection without Native work."""
    capture_type = getattr(native_module, "CapturedPitchAction", None)
    assert isinstance(capture_type, type), "Native pitch API missing: CapturedPitchAction"
    for attribute in (
        "accepted_sequence",
        "received_at_ns",
        "due_frame",
        "execution_frame",
        "content_lifetime",
        "content_is_current",
        "base_shift",
        "extra_shift",
        "key_lock",
        "retrigger",
        "resets_key_correction",
        "quantize_step_64ths",
        "sync_enabled",
        "accepted",
        "effective",
        "effective_output_frame",
        "state",
        "reason",
        "current_control_slot",
    ):
        assert inspect.getattr_static(capture_type, attribute, None) is not None, (
            f"Native pitch API missing: CapturedPitchAction.{attribute}"
        )
    engine_type = getattr(native_module, "AudioEngine", None)
    for method in (
        "select_input_pad",
        "input_selected_projection",
        "capture_pad_pitch_action",
        "capture_selected_pitch_action",
        "publish_pitch_action",
        "cancel_pitch_action",
        "set_input_runtime_state",
        "poll_input_events",
        "load_sample_async",
    ):
        assert callable(getattr(engine_type, method, None)), (
            f"Native pitch API missing: AudioEngine.{method}"
        )
    for method, keyword in (
        ("set_input_runtime_state", "pitch_rows"),
        ("capture_pad_pitch_action", "received_at_ns"),
        ("capture_selected_pitch_action", "received_at_ns"),
        ("load_sample_async", "initial_base_shift"),
        ("load_sample_async", "initial_extra_shift"),
        ("load_sample_async", "initial_key_lock"),
        ("load_sample_async", "initial_gain_db"),
        ("load_sample_async", "initial_eq_db"),
    ):
        parameter = inspect.signature(getattr(engine_type, method)).parameters.get(keyword)
        assert parameter is not None, (
            f"Native pitch API missing keyword: AudioEngine.{method}.{keyword}"
        )
        assert parameter.kind != inspect.Parameter.POSITIONAL_ONLY, (
            f"Native pitch API missing keyword: AudioEngine.{method}.{keyword}"
        )


def check_captured_global_start_api(native_module: object) -> None:
    """Require the original GLOBAL group contract without executing native work."""
    capture_type = getattr(native_module, "CapturedGlobalStart", None)
    assert isinstance(capture_type, type), "Native GLOBAL capture API missing: CapturedGlobalStart"
    for method in ("diagnostic_slots", "original_frame", "state", "reason", "cancel"):
        assert callable(inspect.getattr_static(capture_type, method, None)), (
            f"Native GLOBAL capture API missing: CapturedGlobalStart.{method}"
        )
    engine_type = getattr(native_module, "AudioEngine", None)
    for method in ("capture_global_start", "publish_captured_global_start"):
        assert callable(getattr(engine_type, method, None)), (
            f"Native GLOBAL capture API missing: AudioEngine.{method}"
        )
    timestamp = inspect.signature(
        inspect.getattr_static(engine_type, "capture_global_start")
    ).parameters.get("received_at_ns")
    assert timestamp is not None, "Native GLOBAL capture API invalid keyword: received_at_ns"
    assert timestamp.default is None, "Native GLOBAL capture API invalid keyword: received_at_ns"
    assert timestamp.kind in {
        inspect.Parameter.POSITIONAL_OR_KEYWORD,
        inspect.Parameter.KEYWORD_ONLY,
    }, "Native GLOBAL capture API invalid keyword: received_at_ns"
    publication = inspect.signature(
        inspect.getattr_static(engine_type, "publish_captured_global_start")
    ).parameters
    assert list(publication) == ["self", "captured", "capture"], (
        "Native GLOBAL capture API invalid arguments: publish_captured_global_start"
    )
    assert publication["captured"].kind == inspect.Parameter.POSITIONAL_OR_KEYWORD, (
        "Native GLOBAL capture API invalid argument: captured"
    )
    assert publication["captured"].default is inspect.Parameter.empty, (
        "Native GLOBAL capture API requires the original captured group"
    )
    assert publication["capture"].kind == inspect.Parameter.KEYWORD_ONLY, (
        "Native GLOBAL capture API invalid keyword: capture"
    )
    assert publication["capture"].default is None, (
        "Native GLOBAL capture API invalid default: capture"
    )


def check_key_lock_start_behavior_api(engine_type: type) -> None:
    """Require stopped-only global policy controls without executing Native work."""
    for method in (
        "key_lock_start_behavior",
        "can_change_key_lock_start_behavior",
        "set_key_lock_start_behavior",
    ):
        assert callable(inspect.getattr_static(engine_type, method, None)), (
            f"Native Key Lock start API missing: AudioEngine.{method}"
        )
    behavior = inspect.signature(
        inspect.getattr_static(engine_type, "set_key_lock_start_behavior")
    ).parameters.get("behavior")
    assert behavior is not None, "Native Key Lock start API missing argument: behavior"
    assert behavior.kind in {
        inspect.Parameter.POSITIONAL_OR_KEYWORD,
        inspect.Parameter.KEYWORD_ONLY,
    }, "Native Key Lock start API invalid argument: behavior"
    assert behavior.default is inspect.Parameter.empty, (
        "Native Key Lock start API requires an explicit behavior"
    )


def check_ordinary_start_api(native_module: object) -> None:
    """Reflect original ordinary-start ownership without constructing or running Native work."""
    capture_type = getattr(native_module, "CapturedPadStart", None)
    assert isinstance(capture_type, type), "Native ordinary-start API missing: CapturedPadStart"
    assert inspect.getattr_static(capture_type, "diagnostic_slot", None) is not None, (
        "Native ordinary-start API missing: CapturedPadStart.diagnostic_slot"
    )
    for method in (
        "is_current",
        "same_owner",
        "original_owner_status",
        "state",
        "reason",
        "accepted",
        "effective_frame",
        "original_frame",
        "execution_frame",
        "cancel",
    ):
        assert callable(inspect.getattr_static(capture_type, method, None)), (
            f"Native ordinary-start API missing: CapturedPadStart.{method}"
        )
    engine_type = getattr(native_module, "AudioEngine", None)
    for method in ("capture_pad_start", "prepare_captured_pad_start", "publish_pad_start"):
        assert callable(getattr(engine_type, method, None)), (
            f"Native ordinary-start API missing: AudioEngine.{method}"
        )
    for method, keyword, default in (
        ("capture_pad_start", "exclusive", False),
        ("capture_pad_start", "received_at_ns", None),
        ("capture_pad_start", "expected_source_generation", None),
        ("capture_pad_start", "explicit_target", None),
        ("publish_pad_start", "window", None),
        ("prepare_captured_pad_start", "window", None),
    ):
        parameter = inspect.signature(getattr(engine_type, method)).parameters.get(keyword)
        assert parameter is not None, (
            f"Native ordinary-start API missing keyword: AudioEngine.{method}.{keyword}"
        )
        assert parameter.kind in {
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
            inspect.Parameter.KEYWORD_ONLY,
        }, f"Native ordinary-start API missing keyword: AudioEngine.{method}.{keyword}"
        assert parameter.default is default, (
            f"Native ordinary-start API invalid default: AudioEngine.{method}.{keyword}"
        )
    trigger_type = getattr(native_module, "CapturedInputPadTrigger", None)
    assert isinstance(trigger_type, type), (
        "Native ordinary-start API missing: CapturedInputPadTrigger"
    )
    getter = inspect.getattr_static(trigger_type, "captured_start", None)
    assert isinstance(getter, property) or inspect.isgetsetdescriptor(getter), (
        "Native ordinary-start API missing getter: CapturedInputPadTrigger.captured_start"
    )


def _check_mix_capture_signature(
    owner: object, method: str, label: str, arguments: tuple[tuple[str, object], ...]
) -> None:
    implementation = inspect.getattr_static(owner, method, None)
    assert callable(implementation), f"Native mix capture API missing: {label}"
    parameters = inspect.signature(implementation).parameters
    assert list(parameters) == [name for name, _default in arguments], (
        f"Native mix capture API invalid signature: {label}"
    )
    for name, default in arguments:
        parameter = parameters[name]
        kinds = (
            {inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD}
            if name == "self"
            else {inspect.Parameter.POSITIONAL_OR_KEYWORD}
        )
        assert parameter.kind in kinds, f"Native mix capture API invalid argument: {label}.{name}"
        assert type(parameter.default) is type(default), (
            f"Native mix capture API invalid default: {label}.{name}"
        )
        assert parameter.default == default, (
            f"Native mix capture API invalid default: {label}.{name}"
        )


def check_mix_capture_api(native_module: object) -> None:
    """Reflect internal spool/recovery APIs without opening devices or capture files."""
    engine = getattr(native_module, "AudioEngine", None)
    assert isinstance(engine, type), "Native mix capture API missing type: AudioEngine"
    ticket = getattr(native_module, "MixCaptureTicket", None)
    assert isinstance(ticket, type), "Native mix capture API missing type: MixCaptureTicket"
    required = inspect.Parameter.empty
    for owner, method, label, arguments in (
        *(
            (engine, method, f"AudioEngine.{method}", (("self", required),))
            for method in ("audio_stream_failed", "quiesce_failed_audio_stream")
        ),
        (ticket, "session_id", "MixCaptureTicket.session_id", (("self", required),)),
        (
            engine,
            "prepare_mix_capture",
            "AudioEngine.prepare_mix_capture",
            (
                ("self", required),
                ("spool_path", required),
                ("block_frames", 4096),
                ("block_count", 32),
                ("max_pool_pcm_bytes", 8388608),
            ),
        ),
        *(
            (
                engine,
                method,
                f"AudioEngine.{method}",
                (("self", required), ("ticket", required), ("target_frame", None)),
            )
            for method in ("start_mix_capture", "stop_mix_capture")
        ),
        *(
            (
                engine,
                method,
                f"AudioEngine.{method}",
                (("self", required), ("ticket", required)),
            )
            for method in ("cancel_mix_capture", "mix_capture_status")
        ),
        (
            native_module,
            "inspect_mix_capture_spool",
            "inspect_mix_capture_spool",
            (("path", required),),
        ),
    ):
        _check_mix_capture_signature(owner, method, label, arguments)


def check_bpm_only_runtime_api(native_module: object) -> None:
    """Reject a stale Native without opening an audio device or starting work."""
    engine = getattr(native_module, "AudioEngine", None)
    version = getattr(engine, "BPM_ONLY_RUNTIME_API_VERSION", None)
    assert type(version) is int, (
        "Native constant-BPM runtime requires a current Release build; use start-release.bat"
    )
    assert version == 1, (
        "Native constant-BPM runtime requires a current Release build; use start-release.bat"
    )
    configure = inspect.getattr_static(engine, "configure_automatic_bpm_range", None)
    range_error = (
        "Native automatic BPM range requires a current Release build; use start-release.bat"
    )
    assert callable(configure), range_error
    range_parameters = inspect.signature(configure).parameters
    assert tuple(range_parameters) == ("self", "enabled", "minimum_bpm", "maximum_bpm"), range_error
    assert all(
        parameter.default is inspect.Parameter.empty
        and parameter.kind
        in {inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD}
        for parameter in range_parameters.values()
    ), range_error
    runtime = getattr(engine, "set_input_runtime_state", None)
    assert callable(runtime), "Native constant-BPM runtime is missing set_input_runtime_state"
    parameters = inspect.signature(runtime).parameters
    assert "source_map_rows" not in parameters, (
        "Native runtime still exposes the retired source-SYNC policy"
    )
    assert "timing_policy_json" not in parameters, (
        "Native runtime still exposes the retired source-SYNC policy"
    )
    assert not hasattr(native_module, "SourceBeatmap"), "Native still exposes source-map editing"
    assert not hasattr(engine, "request_input_runtime_policy_adoption"), (
        "Native still exposes source-SYNC adoption"
    )
    for method in ("anchor_transport_phase_from_pad", "bootstrap_transport_from_pad"):
        assert not hasattr(engine, method), f"Native still exposes retired phase control: {method}"


def check_beat_this_execution_api(engine_type: object, installation_root: Path) -> None:
    """Only a selected CUDA profile requires the new Native execution protocol."""
    if not (installation_root / "current.json").is_file():
        message = "Portable BeatThis worker is missing; run the installer to repair it"
        raise RuntimeError(message)
    execution = importlib.import_module("flitzis_looper.analysis.execution")
    setup = importlib.import_module("flitzis_looper.analysis.setup")
    configuration = setup.load_worker_configuration(installation_root, installation_root.parent)
    setup.load_staged_worker_configuration(
        installation_root, configuration.manifest.parent, installation_root.parent
    )
    helper_path = Path(__file__).with_name("prepare_analysis.py")
    if not helper_path.is_file():
        message = "Portable analysis verification helper is missing"
        raise RuntimeError(message)
    specification = importlib.util.spec_from_file_location(
        "portable_analysis_verification", helper_path
    )
    helper = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(helper)
    helper.verify_python(configuration.manifest.parent / ".venv", installation_root.parents[1])
    if isinstance(configuration.model, execution.CudaBeatModelIdentity):
        execution.require_cuda_native_capability(engine_type)


def check_runtime(
    root: Path, checkout: Path, runtime: Path, *, require_analysis: bool = True
) -> None:
    # Runtime imports must follow private-path/DLL setup, before any GUI/device opens.
    native_module = importlib.import_module("flitzis_looper_audio")
    native = physical_path(Path(native_module.flitzis_looper_audio.__file__))
    assert native.is_relative_to(physical_path(checkout / "src/flitzis_looper_audio")), native
    assert native_module.native_build_profile() == "release"
    check_resident_api(native_module.AudioEngine)
    check_pad_copy_api(native_module.AudioEngine)
    check_pad_bank_api(native_module)
    check_pad_pair_hold_api(native_module.AudioEngine)
    check_timing_completion_api(native_module)
    check_pitch_action_api(native_module)
    check_ordinary_start_api(native_module)
    check_captured_global_start_api(native_module)
    check_key_lock_start_behavior_api(native_module.AudioEngine)
    check_global_rate_api(native_module.AudioEngine)
    check_mix_capture_api(native_module)
    check_bpm_only_runtime_api(native_module)
    if require_analysis:
        check_beat_this_execution_api(
            native_module.AudioEngine, root / "analysis-runtime" / "beat-this"
        )
    check_ordinary_transport_api(native_module)
    assert physical_path(Path(sys.executable)).is_relative_to(physical_path(runtime / "python"))
    customization = importlib.import_module("sitecustomize")
    assert customization.__file__ is not None, "Portable sitecustomize has no file origin"
    assert physical_path(Path(customization.__file__)).is_relative_to(
        physical_path(runtime / "python")
    )
    for name in ["imgui_bundle", "torch", "torchaudio", "torchcodec"]:
        module = importlib.import_module(name)
        assert module.__file__ is not None, f"Portable dependency has no file origin: {name}"
        assert physical_path(Path(module.__file__)).is_relative_to(
            physical_path(runtime / "python")
        ), module
    assert importlib.import_module("flitzis_looper.controller.stems").StemController
    assert importlib.import_module("flitzis_looper.ui.render.sidebar_left")
    generation = importlib.import_module("flitzis_looper.controller.stem_generation")
    assert generation.default_demucs_model_cache_dir() == root / "models/torch/hub/checkpoints"
    assert physical_path(Path.cwd()) == physical_path(checkout)
    assert (checkout / "assets/models/keynet.onnx").is_file()
    app_module = importlib.import_module("flitzis_looper")
    assert app_module.__file__ is not None, "Portable application has no file origin"
    assert physical_path(Path(app_module.__file__)).is_relative_to(physical_path(checkout / "src"))
    dlls = private_dlls(runtime, checkout)
    child_env = os.environ | {"PYTHONPATH": str(checkout / "src"), "PYTHONDONTWRITEBYTECODE": "1"}
    subprocess.run(
        [
            sys.executable,
            "-s",
            "-B",
            "-c",
            (
                "import torchcodec.encoders; import flitzis_looper.demucs_streaming; "
                "import flitzis_bs_roformer.__main__"
            ),
        ],
        env=child_env,
        check=True,
    )
    for tool in ["ffmpeg.exe", "ffprobe.exe"]:
        subprocess.run(
            [str(runtime / "runtime/ffmpeg" / tool), "-version"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    sys.stdout.write(
        json.dumps({
            "portable_preflight": "passed",
            "analysis_checked": require_analysis,
            "native": str(native),
            "checkout": str(physical_path(checkout)),
            "dlls": dlls,
        })
        + "\n"
    )
    sys.stdout.flush()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--checkout", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--check-runtime", action="store_true")
    parser.add_argument(
        "--check-build-runtime",
        action="store_true",
        help="Internal bundle preflight before installer worker selection; not App readiness",
    )
    args = parser.parse_args()
    if args.check_build_runtime and (args.check or args.check_runtime):
        parser.error("Build runtime preflight is distinct from final readiness")
    # Preserve the starter's short operational path; use resolved paths only
    # when proving that imports/DLLs belong to this private runtime.
    runtime = args.runtime or Path(__file__).absolute().parent
    root = args.root.resolve()
    checkout = args.checkout or root / "flitzis-looper"
    dll_handles = configure(root, checkout, runtime)
    assert dll_handles
    if args.check or args.check_runtime or args.check_build_runtime:
        check_runtime(root, checkout, runtime, require_analysis=not args.check_build_runtime)
    else:
        from flitzis_looper.controller import AppController  # noqa: PLC0415
        from flitzis_looper.ui import run_ui  # noqa: PLC0415

        run_ui(AppController(beat_this_installation_root=root / "analysis-runtime/beat-this"))


if __name__ == "__main__":
    main()
