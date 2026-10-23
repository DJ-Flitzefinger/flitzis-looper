"""Run the private portable runtime; import checks never start the GUI/device."""

# Standalone entrypoint copied into each runtime.
import argparse
import ctypes
import importlib
import inspect
import json
import os
import runpy
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
    runtime = runtime or Path(sys.executable).absolute().parent.parent
    sys.path.insert(0, str(checkout / "src"))
    os.environ["PYTHONPATH"] = str(checkout / "src")
    os.chdir(checkout)
    os.environ["FLITZIS_MODEL_CACHE_DIR"] = str(root / "models/torch/hub/checkpoints")
    os.environ["TORCH_HOME"] = str(root / "models/torch")
    os.environ["HF_HOME"] = str(root / "models/huggingface")
    os.environ["XDG_CACHE_HOME"] = str(root / "cache")
    os.environ["DORA_DIR"] = str(root / "cache/dora")
    os.environ["TMP"] = os.environ["TEMP"] = str(root / "cache/temp")
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
    """Require callback rate observation before activating the global rate controls."""
    assert callable(getattr(engine_type, "global_rate_status", None)), (
        "Native rate confirmation API missing: AudioEngine.global_rate_status"
    )


def check_ordinary_transport_api(native_module: object) -> None:
    """Reject admission-only pause runtimes before any application or device starts."""
    engine = getattr(native_module, "AudioEngine", None)
    version = getattr(engine, "ORDINARY_TRANSPORT_API_VERSION", None)
    assert type(version) is int, (
        "Native ordinary transport API requires ORDINARY_TRANSPORT_API_VERSION=1"
    )
    assert version == 1, "Native ordinary transport API requires ORDINARY_TRANSPORT_API_VERSION=1"
    getters = {
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
        ("set_input_runtime_state", "source_map_rows"),
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


def _check_global_timing_policy_api(engine: type) -> None:
    """Reject stale timing publication/adoption APIs using reflection alone."""
    timing_version = getattr(engine, "GLOBAL_TIMING_POLICY_API_VERSION", None)
    assert type(timing_version) is int, (
        "Native timing API requires GLOBAL_TIMING_POLICY_API_VERSION=1"
    )
    assert timing_version == 1, "Native timing API requires GLOBAL_TIMING_POLICY_API_VERSION=1"
    runtime = getattr(engine, "set_input_runtime_state", None)
    assert callable(runtime), "Native timing API missing: set_input_runtime_state"
    timing_parameter = inspect.signature(runtime).parameters.get("timing_policy_json")
    timing_error = "Native timing API requires optional keyword-only timing_policy_json=None"
    assert timing_parameter is not None, timing_error
    assert timing_parameter.kind == inspect.Parameter.KEYWORD_ONLY, timing_error
    assert timing_parameter.default is None, timing_error
    adoption = getattr(engine, "request_input_runtime_policy_adoption", None)
    adoption_error = (
        "Native timing API requires AudioEngine.request_input_runtime_policy_adoption(self)"
    )
    assert callable(adoption), adoption_error
    parameters = inspect.signature(adoption).parameters
    assert tuple(parameters) == ("self",), adoption_error
    receiver = parameters["self"]
    assert receiver.kind in {
        inspect.Parameter.POSITIONAL_ONLY,
        inspect.Parameter.POSITIONAL_OR_KEYWORD,
    }, adoption_error
    assert receiver.default is inspect.Parameter.empty, adoption_error


def check_source_beatmap_editor_api(native_module: object) -> None:
    """Reflect the source-qualified editor contract without starting Native work."""
    engine = getattr(native_module, "AudioEngine", None)
    beatmap = getattr(native_module, "SourceBeatmap", None)
    raw_candidate = getattr(native_module, "SourceBeatmapRawCandidate", None)
    job = getattr(native_module, "OfflineAnalysisJob", None)
    assert isinstance(engine, type), "Native source beatmap editor API missing type: AudioEngine"
    _check_global_timing_policy_api(engine)
    assert isinstance(beatmap, type), "Native source beatmap editor API missing type: SourceBeatmap"
    assert isinstance(raw_candidate, type), (
        "Native source beatmap editor API missing type: SourceBeatmapRawCandidate"
    )
    assert isinstance(job, type), (
        "Native source beatmap editor API missing type: OfflineAnalysisJob"
    )
    raw_version = getattr(raw_candidate, "RAW_REVIEW_API_VERSION", None)
    assert type(raw_version) is int, (
        "Native source beatmap editor API requires RAW_REVIEW_API_VERSION=2"
    )
    assert raw_version == 2, "Native source beatmap editor API requires RAW_REVIEW_API_VERSION=2"
    correction_version = getattr(beatmap, "EDITOR_CORRECTION_API_VERSION", None)
    assert type(correction_version) is int, (
        "Native source beatmap editor API requires EDITOR_CORRECTION_API_VERSION=4"
    )
    assert correction_version == 4, (
        "Native source beatmap editor API requires EDITOR_CORRECTION_API_VERSION=4"
    )
    for method in (
        "revision",
        "source_identity",
        "evaluate_source",
        "evaluate_beat",
        "local_tempo_at_source",
        "nearest_grid_source",
    ):
        assert callable(inspect.getattr_static(beatmap, method, None)), (
            f"Native source beatmap editor API missing: SourceBeatmap.{method}"
        )
    required = inspect.Parameter.empty
    for owner, method, arguments, keyword_only in (
        (
            engine,
            "begin_source_beatmap_review_async",
            (
                ("self", required),
                ("sample_id", required),
                ("candidate", required),
                ("review_json", required),
                ("expected_source_generation", required),
            ),
            {"expected_source_generation"},
        ),
        (
            engine,
            "begin_source_beatmap_beat_this_raw_async",
            (
                ("self", required),
                ("sample_id", required),
                ("installation_root", required),
                ("expected_source_generation", required),
            ),
            {"expected_source_generation"},
        ),
        (
            engine,
            "begin_source_beatmap_raw_async",
            (
                ("self", required),
                ("sample_id", required),
                ("expected_source_generation", required),
            ),
            {"expected_source_generation"},
        ),
        (
            job,
            "resolve_source_beatmap_raw_completion",
            (("self", required), ("delivery", required)),
            set(),
        ),
        (job, "source_beatmap_async_status", (("self", required),), set()),
        (raw_candidate, "info", (("self", required),), set()),
        (
            raw_candidate,
            "inspect",
            (("self", required), ("start_index", required), ("max_events", 128)),
            set(),
        ),
        (
            raw_candidate,
            "inspect_downbeats",
            (("self", required), ("start_index", required), ("max_events", 128)),
            set(),
        ),
        (
            engine,
            "pad_source_sync_status",
            (("self", required), ("id", required)),
            set(),
        ),
        (
            engine,
            "begin_source_beatmap_create_async",
            (
                ("self", required),
                ("sample_id", required),
                ("origin_seconds", required),
                ("period_seconds", required),
                ("expected_source_generation", None),
            ),
            {"expected_source_generation"},
        ),
        (
            engine,
            "begin_source_beatmap_edit_async",
            (
                ("self", required),
                ("sample_id", required),
                ("receipt_json", required),
                ("operation_json", required),
            ),
            set(),
        ),
        (beatmap, "editor_info", (("self", required),), set()),
        (beatmap, "editor_review_info", (("self", required),), set()),
        (
            beatmap,
            "editor_review_page",
            (
                ("self", required),
                ("first_anchor_index", required),
                ("last_anchor_index", required),
                ("start_index", required),
                ("max_events", 128),
            ),
            set(),
        ),
        (
            beatmap,
            "editor_span",
            (
                ("self", required),
                ("first_anchor_index", required),
                ("last_anchor_index", required),
            ),
            set(),
        ),
        (
            beatmap,
            "editor_view",
            (
                ("self", required),
                ("start_s", required),
                ("end_s", required),
                ("loop_start_s", required),
                ("px_per_s", required),
                ("max_points", 2048),
            ),
            set(),
        ),
        (
            beatmap,
            "source_after_beats",
            (
                ("self", required),
                ("source_seconds", required),
                ("beats", required),
                ("require_four_four", True),
            ),
            set(),
        ),
    ):
        label = f"{owner.__name__}.{method}"
        implementation = inspect.getattr_static(owner, method, None)
        assert callable(implementation), f"Native source beatmap editor API missing: {label}"
        parameters = inspect.signature(implementation).parameters
        assert list(parameters) == [name for name, _default in arguments], (
            f"Native source beatmap editor API invalid signature: {label}"
        )
        for name, default in arguments:
            parameter = parameters[name]
            kinds: set[int] = (
                {inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD}
                if name == "self"
                else {inspect.Parameter.KEYWORD_ONLY}
                if name in keyword_only
                else {inspect.Parameter.POSITIONAL_OR_KEYWORD}
            )
            assert parameter.kind in kinds, (
                f"Native source beatmap editor API invalid argument: {label}.{name}"
            )
            assert type(parameter.default) is type(default), (
                f"Native source beatmap editor API invalid default: {label}.{name}"
            )
            assert parameter.default == default, (
                f"Native source beatmap editor API invalid default: {label}.{name}"
            )


def check_runtime(root: Path, checkout: Path, runtime: Path) -> None:
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
    check_source_beatmap_editor_api(native_module)
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
    args = parser.parse_args()
    # Preserve the starter's short operational path; use resolved paths only
    # when proving that imports/DLLs belong to this private runtime.
    runtime = args.runtime or Path(__file__).absolute().parent
    root = args.root.resolve()
    checkout = args.checkout or root / "flitzis-looper"
    dll_handles = configure(root, checkout, runtime)
    assert dll_handles
    if args.check or args.check_runtime:
        check_runtime(root, checkout, runtime)
    else:
        runpy.run_module("flitzis_looper", run_name="__main__")


if __name__ == "__main__":
    main()
