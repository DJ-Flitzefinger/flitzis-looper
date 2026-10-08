"""Actual traced runtime and retained producer sources of approved QM diagnostics."""

from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.analysis.beat_native_lineage import _file, _object
from flitzis_looper.analysis.corrected_legacy_sources import source_bindings, source_file
from flitzis_looper.analysis.reference_inputs_validation import fail, private_path, unique_json

if TYPE_CHECKING:
    from flitzis_looper.analysis.beat_candidate_models import ArtifactBinding


def _key(binding: ArtifactBinding) -> tuple[str, str, int]:
    return binding.path, binding.sha256, binding.bytes


def _path(value: object) -> str:
    if not isinstance(value, str) or not value or not Path(value).is_absolute():
        fail("corrected_legacy_actual_module_path_required")
    return value


def _trace(
    workspace: Path, runtime: dict[str, object], raw: dict[str, bytes]
) -> tuple[ArtifactBinding, dict[str, object], dict[str, object], set[str]]:
    binding = _file(workspace, runtime.get("actual_module_trace"), "actual_qm_process_module_trace")
    data = private_path(workspace, binding.path).read_bytes()
    if data != raw["loaded-runtime-modules.json"]:
        fail("corrected_legacy_module_trace_bytes_mismatch")
    trace = _object(unique_json(data))
    native = trace.get("loaded_native_modules")
    pid = trace.get("pid")
    if type(pid) is not int or pid <= 0:
        fail("corrected_legacy_actual_process_identity_required")
    if (
        not isinstance(native, list)
        or not native
        or trace.get("native_observation")
        != "WinAPI EnumProcessModulesEx/GetModuleFileNameExW actual current process"
        or trace.get("external_file_bytes_read") is not False
    ):
        fail("corrected_legacy_actual_native_python_trace_required")
    paths = [_path(_object(value).get("path")) for value in native]
    if len(set(paths)) != len(paths):
        fail("corrected_legacy_duplicate_actual_native_module")
    native_runtime = _object(unique_json(raw["native-runtime.json"]))
    original_exe = _object(native_runtime.get("native_test_executable"))
    retained_exe = _object(runtime.get("native_test_executable"))
    installed = _object(runtime.get("installed_native_extension"))
    if (
        native_runtime.get("hardware_started") is not False
        or native_runtime.get("keynet") != "not_executed"
        or native_runtime.get("beat_this_worker") != "not_executed"
    ):
        fail("corrected_legacy_actual_executing_module_mismatch")
    if (
        _path(original_exe.get("path")) not in paths
        or any(original_exe.get(k) != retained_exe.get(k) for k in ("sha256", "bytes"))
        or native_runtime.get("build_profile") != runtime.get("build_profile")
        or _path(installed.get("original_path")) in paths
        or native_runtime.get("embedded_python") != runtime.get("embedded_python")
    ):
        fail("corrected_legacy_actual_executing_module_mismatch")
    return binding, trace, native_runtime, set(paths)


def _dependency_scope(workspace: Path, source: str, base_python: Path) -> str:
    path = Path(source)
    if path.is_relative_to(workspace.resolve()):
        return "actual_traced_workspace_native_runtime_library"
    if path.is_relative_to(base_python) and path.suffix.lower() in {".dll", ".pyd"}:
        return "actual_traced_external_standard_python_runtime"
    if path.name.lower() in {"rubberband-3.dll", "sleefdft.dll", "sleef.dll", "samplerate.dll"}:
        return "actual_traced_external_linked_native_runtime"
    return fail("corrected_legacy_unapproved_external_dependency_retention")


def _module_partition(
    workspace: Path,
    runtime: dict[str, object],
    sources: dict[str, ArtifactBinding],
    executable: str,
    paths: set[str],
) -> None:
    metadata = runtime.get("external_runtime_module_metadata")
    if not isinstance(metadata, list):
        fail("corrected_legacy_external_module_metadata_required")
    external: set[str] = set()
    for value in metadata:
        item = _object(value)
        path = _path(item.get("path"))
        if (
            item.get("bytes_rehashed") is not False
            or path in external
            or Path(path).is_relative_to(workspace.resolve())
            or path in sources
        ):
            fail("corrected_legacy_external_module_partition_invalid")
        external.add(path)
    if (
        executable in sources
        or executable in external
        or paths != {executable} | set(sources) | external
    ):
        fail("corrected_legacy_complete_actual_module_partition_required")


def _libraries(
    workspace: Path,
    runtime: dict[str, object],
    native_runtime: dict[str, object],
    paths: set[str],
) -> tuple[list[ArtifactBinding], dict[str, ArtifactBinding]]:
    libraries, mappings = runtime.get("loaded_libraries"), runtime.get("loaded_library_sources")
    if not isinstance(libraries, list) or not libraries or not isinstance(mappings, list):
        fail("corrected_legacy_executing_dependencies_required")
    loaded = [_file(workspace, item, "actual_executing_native_dependency") for item in libraries]
    remaining = {_key(b) for b in loaded}
    if len(remaining) != len(loaded) or len(mappings) != len(loaded):
        fail("corrected_legacy_runtime_source_mapping_required")
    sources: dict[str, ArtifactBinding] = {}
    base_python = Path(_path(_object(runtime.get("embedded_python")).get("base_prefix")))
    for value in mappings:
        item = _object(value)
        source = _path(item.get("source_path"))
        bound = _file(workspace, item.get("binding"), "mapped_runtime_source")
        if source not in paths or source in sources or _key(bound) not in remaining:
            fail("corrected_legacy_dependency_not_actually_uniquely_traced")
        remaining.remove(_key(bound))
        if item.get("role") != _dependency_scope(workspace, source, base_python):
            fail("corrected_legacy_traced_dependency_scope_mismatch")
        sources[source] = bound
    executable = _path(_object(native_runtime.get("native_test_executable")).get("path"))
    _module_partition(workspace, runtime, sources, executable, paths)
    return loaded, sources


def _import_names(imported: object) -> dict[str, str]:
    if not isinstance(imported, list) or not imported:
        fail("corrected_legacy_actual_python_source_trace_required")
    names: dict[str, str] = {}
    for value in imported:
        row = _object(value)
        name, path = row.get("name"), row.get("path")
        if (
            not isinstance(name, str)
            or not name
            or name in names
            or name.startswith("flitzis_looper_audio")
        ):
            fail("corrected_legacy_native_pyd_import_or_duplicate_not_allowed")
        if not isinstance(path, str) or not path:
            fail("corrected_legacy_actual_python_source_trace_required")
        names[name] = path
    return names


def _snapshot_source(
    workspace: Path,
    row: dict[str, object],
    bound: ArtifactBinding,
    libraries: dict[str, ArtifactBinding],
    support: dict[str, ArtifactBinding],
    root: Path,
    base_python: Path,
) -> None:
    source = Path(_path(row.get("path")))
    if (
        not source.is_relative_to(workspace.resolve()) and not source.is_relative_to(base_python)
    ) or row.get("role") != "actual_traced_imported_module_file_snapshot":
        fail("corrected_legacy_imported_source_scope_mismatch")
    if source.suffix.lower() in {".dll", ".pyd"}:
        actual = libraries.get(str(row["path"]))
        if actual is None or _key(actual) != _key(bound):
            fail("corrected_legacy_imported_extension_library_mismatch")
    if source.is_relative_to(root):
        original = support.get(source.relative_to(root).as_posix())
        if original is not None and (original.sha256, original.bytes) != (
            bound.sha256,
            bound.bytes,
        ):
            fail("corrected_legacy_imported_support_source_mismatch")


def _imports(
    workspace: Path,
    runtime: dict[str, object],
    trace: dict[str, object],
    libraries: dict[str, ArtifactBinding],
    support: dict[str, ArtifactBinding],
    native_runtime: dict[str, object],
) -> list[ArtifactBinding]:
    names = _import_names(trace.get("imported_python_modules"))
    mappings = runtime.get("imported_python_sources")
    if not isinstance(mappings, list):
        fail("corrected_legacy_actual_python_source_trace_required")
    remaining = dict(names)
    bindings = []
    root = Path(_path(native_runtime.get("repository"))).resolve()
    base_python = Path(_path(_object(runtime.get("embedded_python")).get("base_prefix")))
    embedded = "rust/crates/looper/src/audio_engine/b2_corrected_legacy_probe.py"
    for value in mappings:
        row = _object(value)
        name, path = row.get("name"), row.get("path")
        if not isinstance(name, str) or name not in remaining or remaining[name] != path:
            fail("corrected_legacy_imported_source_trace_bijection_required")
        del remaining[name]
        bound = source_file(workspace, row.get("binding"), "actual_imported_python_source")
        if name == "b2_native_candidate_probe" and path == "b2_native_candidate_probe.py":
            original = support[embedded]
            if (
                row.get("role") != "actual_exe_embedded_python_source"
                or Path(bound.path).name != "compiled-producer.py"
                or (bound.sha256, bound.bytes) != (original.sha256, original.bytes)
            ):
                fail("corrected_legacy_embedded_python_source_mismatch")
        else:
            _snapshot_source(workspace, row, bound, libraries, support, root, base_python)
        bindings.append(bound)
    if remaining or len(mappings) != len(names):
        fail("corrected_legacy_imported_source_trace_bijection_required")
    return bindings


def _extra_bindings(workspace: Path, values: object) -> list[ArtifactBinding]:
    if not isinstance(values, list) or not values:
        fail("corrected_legacy_retained_protocol_dependencies_required")
    bindings = []
    for value in values:
        item = _object(value)
        role = item.get("role")
        if not isinstance(role, str) or not role:
            fail("corrected_legacy_dependency_role_required")
        check = (
            source_file
            if role
            in {
                "actual_imported_python_module_source_or_extension",
                "complete_preflight_local_source_and_build_snapshot",
            }
            else _file
        )
        bindings.append(
            check(workspace, {k: item.get(k) for k in ("path", "sha256", "bytes")}, role)
        )
    return bindings


def runtime_bindings(
    workspace: Path, provenance: dict[str, object], raw: dict[str, bytes]
) -> list[ArtifactBinding]:
    """Rehash retained bytes and tie every library/source to the fixed producer trace."""
    runtime = _object(provenance.get("runtime"))
    if runtime.get("build_profile") not in {"debug", "release"} or (
        runtime.get("executed_procedure") != "native-shared-analyze_bpm_raw"
        or runtime.get("native_exe_actually_executed") is not True
        or runtime.get("key_worker_executed") is not False
        or runtime.get("beat_this_worker_executed") is not False
    ):
        fail("corrected_legacy_executing_profile_procedure_required")
    installed = _object(runtime.get("installed_native_extension"))
    if installed.get("used_by_probe") is not False:
        fail("corrected_legacy_installed_pyd_not_producer")
    bindings = [
        _file(
            workspace,
            runtime.get("native_test_executable"),
            "actual_executing_qm_native_test_executable",
        ),
        _file(workspace, installed.get("binding"), "separate_installed_pyd_unused_by_qm_probe"),
    ]
    trace_binding, trace, native_runtime, paths = _trace(workspace, runtime, raw)
    loaded, libraries = _libraries(workspace, runtime, native_runtime, paths)
    extras = _extra_bindings(workspace, provenance.get("file_bindings"))
    sources, support = source_bindings(workspace, provenance, native_runtime, extras)
    imports = _imports(workspace, runtime, trace, libraries, support, native_runtime)
    return [*bindings, trace_binding, *loaded, *sources, *extras, *imports]
