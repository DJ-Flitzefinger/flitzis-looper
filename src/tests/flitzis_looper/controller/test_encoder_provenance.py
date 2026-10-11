"""Actual supported encoder images/processes; neural inference is not exercised."""

import hashlib
import json
import os
import shutil
from pathlib import Path

import pytest
import torch

from flitzis_looper import demucs_streaming, encoder_provenance
from flitzis_looper.demucs_streaming import write_demucs_pcm16
from flitzis_looper.pcm16_wav import PcmWavReader
from flitzis_looper_audio import SealedProcessingFiles


def _owned_encoder(tmp: Path) -> Path:
    configured = os.environ.get("FLITZIS_MATERIAL_ENCODER")
    if configured is None:
        pytest.skip("Actual shared encoder proof requires the explicit supported image path")
    images = encoder_provenance.supported_encoder_images(Path(configured))
    assert len(images) == 8
    directory = tmp / "owned-encoder"
    directory.mkdir()
    for image in images:
        shutil.copyfile(image, directory / image.name)
    return directory / "ffmpeg.exe"


def test_actual_four_encoder_children_use_held_images_and_reject_changed_dependency(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    executable = _owned_encoder(tmp_path)
    images = encoder_provenance.supported_encoder_images(executable)
    guard: SealedProcessingFiles | None = SealedProcessingFiles([str(path) for path in images])
    contract = encoder_provenance.encoder_contract(executable)
    assert contract is not None
    monkeypatch.setattr(encoder_provenance, "_completed", {})
    monkeypatch.setenv(encoder_provenance.ENCODER_CONTRACT_ENV, contract)
    monkeypatch.setenv("FLITZIS_DEMUCS_ENCODER", str(executable))
    output = tmp_path / "output"
    output.mkdir()
    source = torch.tensor([[0.0, 0.1, -0.1, 1.0, -1.0] * 101], dtype=torch.float32)
    for kind in ("drums", "bass", "other", "vocals"):
        write_demucs_pcm16(source, output / f"{kind}.wav", 44100, block_frames=7)
        with PcmWavReader(output / f"{kind}.wav") as reader:
            assert reader.getnframes() == 505
            assert reader.getnchannels() == 1
            assert reader.getframerate() == 44100
            assert reader.verify()
    stdout = encoder_provenance.encoder_completion_line()
    assert encoder_provenance.verified_encoder_output(output, contract, stdout)
    receipt = json.loads((output / encoder_provenance.ENCODER_RECEIPT).read_bytes())
    assert len(receipt["runs"]) == 4
    assert {run["leaf"] for run in receipt["runs"]} == {
        "drums.wav",
        "bass.wav",
        "other.wav",
        "vocals.wav",
    }
    assert len({run["proof"]["pid"] for run in receipt["runs"]}) == 4
    receipt_path = output / encoder_provenance.ENCODER_RECEIPT
    original_receipt = receipt_path.read_bytes()
    receipt_path.write_bytes(original_receipt + b" ")
    assert not encoder_provenance.verified_encoder_output(output, contract, stdout)
    receipt_path.write_bytes(original_receipt)
    assert encoder_provenance.verified_encoder_output(output, contract, stdout)
    receipt_path.replace(output / "held-prior-execution.json")
    receipt_path.write_bytes(original_receipt)
    assert not encoder_provenance.verified_encoder_output(output, contract, stdout)
    assert guard is not None
    witnesses = guard.witnesses()
    assert len(witnesses) == 8
    for path, digest, size in witnesses:
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest
        assert Path(path).stat().st_size == size
    dependency = executable.parent / "avcodec-62.dll"
    with pytest.raises(PermissionError):
        dependency.write_bytes(b"changed while actual encoder witness is held")
    guard.verify()
    previous = next(digest for path, digest, _ in witnesses if Path(path).samefile(dependency))
    guard = None
    with dependency.open("r+b") as handle:
        handle.seek(-1, os.SEEK_END)
        value = handle.read(1)
        handle.seek(-1, os.SEEK_END)
        handle.write(bytes([value[0] ^ 1]))
    changed = SealedProcessingFiles([str(path) for path in images])
    assert (
        next(digest for path, digest, _ in changed.witnesses() if Path(path).samefile(dependency))
        != previous
    )


def test_preexisting_encoder_diagnostic_is_preserved_and_grants_no_execution_proof(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    contract = "[]"
    monkeypatch.setenv(encoder_provenance.ENCODER_CONTRACT_ENV, contract)
    receipt = tmp_path / encoder_provenance.ENCODER_RECEIPT
    forged = json.dumps({
        "contract": contract,
        "verified": True,
        "runs": [
            {"leaf": kind + ".wav", "proof": {"pid": index, "images": []}}
            for index, kind in enumerate(("drums", "bass", "other", "vocals"), 1)
        ],
    }).encode()
    receipt.write_bytes(forged)
    identity = receipt.stat().st_ino
    encoder_provenance.record_encoder(tmp_path / "drums.wav", None)
    assert receipt.read_bytes() == forged
    assert receipt.stat().st_ino == identity
    monkeypatch.setattr(encoder_provenance, "_completed", {})
    assert not encoder_provenance.verified_encoder_output(
        tmp_path,
        contract,
        encoder_provenance.encoder_completion_line(),
    )


def test_stock_input_tools_launch_captured_absolute_images_with_poisoned_cwd(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    executable = _owned_encoder(tmp_path)
    configured = os.environ["FLITZIS_MATERIAL_ENCODER"]
    probe = executable.with_name("ffprobe.exe")
    shutil.copyfile(Path(configured).with_name("ffprobe.exe"), probe)
    images = (*encoder_provenance.supported_encoder_images(executable), probe)
    guard = SealedProcessingFiles([str(path) for path in images])
    monkeypatch.setenv("FLITZIS_DEMUCS_ENCODER", str(executable))
    monkeypatch.chdir(tmp_path)
    for name in ("ffmpeg.exe", "ffprobe.exe"):
        (tmp_path / name).write_bytes(b"unowned current-directory shadow, never executable")
    source = tmp_path / "source.wav"
    write_demucs_pcm16(torch.zeros((1, 128)), source, 44100)
    info = json.loads(
        demucs_streaming._InputTools.check_output([
            "ffprobe",
            "-loglevel",
            "panic",
            str(source),
            "-print_format",
            "json",
            "-show_streams",
        ])
    )
    assert int(info["streams"][0]["sample_rate"]) == 44100
    output = tmp_path / "decoded.f32"
    result = demucs_streaming._InputTools.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(source),
            "-f",
            "f32le",
            str(output),
        ],
        check=True,
    )
    assert result.returncode == 0
    assert output.read_bytes() == bytes(128 * 4)
    assert len(guard.witnesses()) == 9
    guard.verify()
    for name in ("ffmpeg.exe", "ffprobe.exe"):
        assert (
            tmp_path / name
        ).read_bytes() == b"unowned current-directory shadow, never executable"
