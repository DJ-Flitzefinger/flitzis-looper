import argparse
import hashlib
import os
import sys
import tempfile
import urllib.request
from pathlib import Path

UPSTREAM_COMMIT = "aef04b2e52fb3beaf25e333199f5a7236e628e7b"
UPSTREAM_RELEASE = "v1.0.12"
MODEL_NAME = "BS-RoFormer MUSDB18HQ"
MODEL_IDENTITY = "bs-roformer:musdb18hq:v1.0.12:3e9daecd70aaed5b"
CONFIG_FILENAME = "config_bs_roformer_384_8_2_485100.yaml"
CHECKPOINT_FILENAME = "model_bs_roformer_ep_17_sdr_9.6568.ckpt"
MODEL_ASSETS = (
    (CONFIG_FILENAME, 4566, "d8afb980318d0c08b9c2e24a7adc00d4f3150320c127a7e4de861800d1321939"),
    (
        CHECKPOINT_FILENAME,
        527385512,
        "3e9daecd70aaed5b5a0d1f861cc4d77eaa45afb3fc6301b1cf32c1be0f5868fb",
    ),
)
ASSET_BASE_URL = (
    "https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/download/v1.0.12/"
)


def verify_model_assets(directory: Path) -> None:
    """Reject missing, altered or changing model files before deserialization."""
    for filename, size, expected in MODEL_ASSETS:
        path = directory / filename
        if not path.is_file():
            msg = f"no Model installed: {MODEL_NAME} ({filename})"
            raise RuntimeError(msg)
        with path.open("rb") as source:
            before = os.fstat(source.fileno())
            digest = hashlib.file_digest(source, "sha256").hexdigest()
            after = os.fstat(source.fileno())
        if (
            digest != expected
            or before.st_size != size
            or after.st_size != size
            or before.st_mtime_ns != after.st_mtime_ns
        ):
            msg = f"Model integrity check failed: {MODEL_NAME} ({filename})"
            raise RuntimeError(msg)


def install_model_assets(directory: Path) -> None:
    """Explicitly download the pinned release assets and expose verified files."""
    directory.mkdir(parents=True, exist_ok=True)
    for filename, size, expected in MODEL_ASSETS:
        target = directory / filename
        if target.is_file():
            with target.open("rb") as source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            if target.stat().st_size == size and digest == expected:
                continue
        with tempfile.TemporaryDirectory(prefix=".install-", dir=directory) as temporary:
            staged = Path(temporary) / filename
            with (
                urllib.request.urlopen(ASSET_BASE_URL + filename, timeout=120) as response,
                staged.open("xb") as destination,
            ):
                written = 0
                while chunk := response.read(1024 * 1024):
                    written += len(chunk)
                    if written > size:
                        msg = f"Model download exceeds pinned size: {filename}"
                        raise RuntimeError(msg)
                    destination.write(chunk)
            with staged.open("rb") as source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            if written != size or digest != expected:
                msg = f"Model download integrity check failed: {filename}"
                raise RuntimeError(msg)
            staged.replace(target)
    verify_model_assets(directory)


def main() -> None:
    """Run explicit installation; never called by generation or Settings."""
    parser = argparse.ArgumentParser(description=f"Install verified {MODEL_NAME} assets")
    parser.add_argument("--directory", type=Path, required=True)
    options = parser.parse_args()
    install_model_assets(options.directory)
    sys.stdout.write(f"Installed and verified {MODEL_IDENTITY} in {options.directory}\n")


if __name__ == "__main__":
    main()
