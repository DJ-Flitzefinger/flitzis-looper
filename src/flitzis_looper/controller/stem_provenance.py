"""Background full model witnesses held through actual processing and compatible reuse."""

import hashlib
import json
import platform
import sys
from importlib.metadata import PackageNotFoundError, version
from importlib.util import find_spec
from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.controller import bs_roformer_generation
from flitzis_looper.controller.bs_roformer_assets import CHECKPOINT_FILENAME, CONFIG_FILENAME
from flitzis_looper.controller.processing_code import loaded_code_identity
from flitzis_looper.controller.stem_generation import (
    DEMUCS_REQUIRED_CHECKPOINTS,
    resolved_demucs_encoder,
)
from flitzis_looper.encoder_provenance import supported_encoder_images
from flitzis_looper_audio import SealedProcessingFiles

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import StemGenerationRequest

BS_PRODUCER_FILES = (
    "__init__.py",
    "__main__.py",
    "audio.py",
    "inference.py",
    "model.py",
    "network/__init__.py",
    "network/attend.py",
    "network/bs_roformer.py",
)

DEMUCS_PRODUCER_FILES = (
    "remote/htdemucs.yaml",
    "remote/files.txt",
    "__init__.py",
    "htdemucs.py",
    "hdemucs.py",
    "demucs.py",
    "transformer.py",
    "spec.py",
    "utils.py",
    "states.py",
    "repo.py",
    "apply.py",
    "pretrained.py",
    "separate.py",
    "audio.py",
)


def _host_processing_paths(request: StemGenerationRequest) -> tuple[Path, ...]:
    """Name ordered executed host roles independently of optional encoder images."""
    package = Path(__file__).parent.parent
    separator = (
        (
            package / "controller" / "bs_roformer_generation.py",
            Path(bs_roformer_generation.verify_model_assets.__code__.co_filename),
            package / "bs_input_provenance.py",
        )
        if request.separator == "bs-roformer:musdb18hq"
        else ()
    )
    return (
        package / "controller" / "stem_artifacts.py",
        package / "pcm16_wav.py",
        package / "controller" / "stem_generation.py",
        *separator,
        package / "encoder_provenance.py",
        package / "controller" / "processing_code.py",
        package / "controller" / "stem_process.py",
        package / "analysis" / "process.py",
        package / "analysis" / "windows_job.py",
        package / "analysis" / "contracts.py",
        package / "stem_cancellation.py",
        package / "stem_execution.py",
        package / "__init__.py",
    )


def processing_paths(request: StemGenerationRequest) -> tuple[Path, ...]:
    """Name the exact supported checkpoint, model definition and executed transform files."""
    project_package = Path(__file__).parent.parent
    if request.separator == "bs-roformer:musdb18hq":
        module = find_spec("flitzis_bs_roformer")
        if module is None or module.submodule_search_locations is None:
            return ()
        package = Path(next(iter(module.submodule_search_locations)))
        encoder = request.encoder_executable or resolved_demucs_encoder(request.model_cache_dir)
        if encoder is None:
            return ()
        return (
            request.model_cache_dir / CHECKPOINT_FILENAME,
            request.model_cache_dir / CONFIG_FILENAME,
            *(package / name for name in BS_PRODUCER_FILES),
            *_host_processing_paths(request),
            encoder,
            *supported_encoder_images(encoder)[1:],
            *(
                (encoder.with_name("ffprobe.exe"),)
                if encoder.with_name("ffprobe.exe").is_file()
                else ()
            ),
        )
    module = find_spec("demucs")
    if module is None or module.submodule_search_locations is None:
        return ()
    package = Path(next(iter(module.submodule_search_locations)))
    encoder = request.encoder_executable or resolved_demucs_encoder(request.model_cache_dir)
    if encoder is None:
        return ()
    return (
        *(request.model_cache_dir / name for name in DEMUCS_REQUIRED_CHECKPOINTS["htdemucs"]),
        *(package / name for name in DEMUCS_PRODUCER_FILES),
        project_package / "demucs_streaming.py",
        *_host_processing_paths(request),
        encoder,
        *supported_encoder_images(encoder)[1:],
        *(
            (encoder.with_name("ffprobe.exe"),)
            if encoder.with_name("ffprobe.exe").is_file()
            else ()
        ),
    )


def capture_processing_files(
    request: StemGenerationRequest,
) -> tuple[SealedProcessingFiles, str | None] | None:
    """Seal and hash all actual producer inputs off-thread; unavailable lineage is unknown."""
    paths = processing_paths(request)
    libraries = processing_library_versions(request)
    if not paths or any(not path.is_file() for path in paths):
        return None
    guard = SealedProcessingFiles([str(path) for path in paths])
    parent_paths = _parent_processing_paths(request, paths)
    execution = loaded_code_identity(parent_paths)
    if libraries is None or execution is None or not processing_source_roots(request, guard):
        # Exact known files still have execution custody through publication;
        # incomplete implementation lineage grants no completed-set reuse claim.
        return guard, None
    # The ordered role, complete bytes and size bind the exact implementation;
    # absolute model storage locations are not a portable producer identity.
    witnesses = [(index, digest, size) for index, (_, digest, size) in enumerate(guard.witnesses())]
    encoded = json.dumps((witnesses, libraries, execution), separators=(",", ":")).encode("utf-8")
    return guard, hashlib.sha256(encoded).hexdigest()


def _parent_processing_paths(
    request: StemGenerationRequest, paths: tuple[Path, ...]
) -> tuple[Path, ...]:
    # These named host roles execute in the current parent. Separator/model
    # package modules execute in fresh child imports and keep their own guards.
    expected = _host_processing_paths(request)
    parents = tuple(path for path in paths if path in expected)
    return parents if parents == expected else ()


def processing_source_roots(
    request: StemGenerationRequest, guard: SealedProcessingFiles
) -> tuple[Path, ...]:
    """Bind normal child imports to exact guarded host/separator package roots."""
    witnesses = guard.witnesses()
    bs = request.separator == "bs-roformer:musdb18hq"
    expected = processing_paths(request)
    if len(witnesses) != len(expected) or any(
        not Path(name).samefile(path)
        for (name, _digest, _size), path in zip(witnesses, expected, strict=True)
    ):
        return ()
    host_init = _host_processing_paths(request)[-1]
    separator_init = expected[2 if bs else 3]
    if host_init.name != "__init__.py" or host_init.read_bytes() != b"":
        return ()
    if separator_init.name != "__init__.py":
        return ()
    return host_init.parent.parent, separator_init.parent.parent


def processing_library_versions(
    request: StemGenerationRequest,
) -> tuple[tuple[str, str], ...] | None:
    """Bind installed inference/normalization implementations; unavailable versions stay unknown."""
    if request.separator == "bs-roformer:musdb18hq":
        if not _known_encoder(request):
            return None
        bs_packages = (
            "flitzis-bs-roformer",
            "torch",
            "numpy",
            "beartype",
            "einops",
            "rotary-embedding-torch",
            "packaging",
        )
        return _library_versions(bs_packages)
    if request.separator == "demucs:htdemucs" and (
        not _known_encoder(request)
        or request.model_cache_dir.name != "checkpoints"
        or request.model_cache_dir.parent.name != "hub"
    ):
        return None
    packages: tuple[str, ...] = (
        "torch",
        "torchaudio",
        "numpy",
        "demucs",
        "torchcodec",
        "omegaconf",
        "julius",
        "openunmix",
        "einops",
        "dora-search",
        "PyYAML",
    )
    return _library_versions(packages)


def _library_versions(packages: tuple[str, ...]) -> tuple[tuple[str, str], ...] | None:
    try:
        libraries = tuple((package, version(package)) for package in packages)
        return (*libraries, ("python", sys.version), ("platform", platform.platform()))
    except PackageNotFoundError:
        return None


def _known_encoder(request: StemGenerationRequest) -> bool:
    encoder = request.encoder_executable or resolved_demucs_encoder(request.model_cache_dir)
    return (
        encoder is not None
        and bool(supported_encoder_images(encoder))
        and encoder.with_name("ffprobe.exe").is_file()
        and not encoder.with_name("ffprobe.exe.local").exists()
    )
