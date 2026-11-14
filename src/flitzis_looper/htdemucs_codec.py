"""Decode and encode HTDemucs PCM in the owned stem codec process."""

import argparse
import hashlib
import json
import os
import stat
import subprocess
import sys
from pathlib import Path

from flitzis_looper.bs_input_provenance import (
    begin_input_attempt,
    finish_input_attempt,
    input_completion_line,
    run_input_tool,
)
from flitzis_looper.encoder_provenance import (
    encoder_completion_line,
    inspect_encoder,
    record_encoder,
)
from flitzis_looper.offline_runtime import ffmpeg_command
from flitzis_looper.stem_cancellation import (
    CANCEL_EXIT_CODE,
    StemCancellation,
    StemInferenceCancelledError,
    emit_cancellation_ack,
    tool_environment,
    wait_cancellable,
)

RATE = 44_100
STEMS = ("drums", "bass", "other", "vocals")
CODEC_OUTPUT = "FLITZIS_HTDEMUCS_CODEC_OUTPUT_V1:"


def _decode_ffmpeg(source: Path, destination: Path, cancellation: StemCancellation) -> int:
    checkpoint = cancellation.checkpoint
    probe = run_input_tool(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=channels",
            "-of",
            "json",
            str(source),
        ],
        checkpoint=checkpoint,
    )
    [stream] = json.loads(probe.stdout)["streams"]
    channels = stream["channels"]
    # Match stock FFmpeg resampling while retaining its original channels for
    # the subsequent first-two/repeat-mono conversion in the Native worker.
    if type(channels) is not int or not 0 < channels <= 8192:
        msg = "Invalid HTDemucs source channel count"
        raise ValueError(msg)
    run_input_tool(
        ffmpeg_command([
            "ffmpeg",
            "-nostdin",
            "-n",
            "-v",
            "error",
            "-i",
            str(source),
            "-map",
            "0:a:0",
            "-f",
            "f32le",
            "-ar",
            str(RATE),
            str(destination),
        ]),
        checkpoint=checkpoint,
    )
    return channels


def decode(source: Path, destination: Path, cancellation: StemCancellation) -> None:
    if destination.exists() or destination.is_symlink():
        msg = "HTDemucs decode destination must be a fresh scratch leaf"
        raise FileExistsError(msg)
    channels = _decode_ffmpeg(source, destination, cancellation)
    sys.stdout.write(json.dumps({"channels": channels}) + "\n")


def encode(executable: Path, directory: Path, cancellation: StemCancellation) -> None:
    for stem in STEMS:
        cancellation.checkpoint("encoder_start")
        raw, wav = directory / f"{stem}.f32", directory / f"{stem}.wav"
        command = ffmpeg_command([
            str(executable),
            "-nostdin",
            "-n",
            "-v",
            "error",
            "-f",
            "f32le",
            "-ar",
            str(RATE),
            "-ac",
            "2",
            "-i",
            "pipe:0",
            "-map_metadata",
            "-1",
            "-c:a",
            "pcm_s16le",
            "-rf64",
            "auto",
            str(wav),
        ])
        with (
            raw.open("rb") as input_stream,
            subprocess.Popen(
                command,
                stdin=subprocess.PIPE,
                stdout=subprocess.DEVNULL,
                env=tool_environment(),
            ) as process,
        ):
            assert process.stdin is not None
            proof = inspect_encoder(process)
            try:
                with process.stdin:
                    while block := input_stream.read(64 * 1024):
                        cancellation.checkpoint("encoder_write")
                        if process.stdin.write(block) != len(block):
                            msg = "Incomplete encoder PCM input"
                            raise OSError(msg)  # noqa: TRY301 -- Reap this exact encoder.
                wait_cancellable(process, cancellation.checkpoint)
            except OSError, StemInferenceCancelledError:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                raise
        if process.returncode:
            msg = f"HTDemucs encoder failed for {stem} ({process.returncode})"
            raise RuntimeError(msg)
        record_encoder(wav, proof)


def _report_output_files(paths: tuple[Path, ...], cancellation: StemCancellation) -> None:
    result = {}
    for path in paths:
        with path.open("rb") as stream:
            birth = os.fstat(stream.fileno())
            if not stat.S_ISREG(birth.st_mode) or birth.st_size <= 0:
                msg = f"Invalid codec output at {path.name}"
                raise ValueError(msg)
            digest = hashlib.sha256()
            while block := stream.read(256 * 1024):
                cancellation.checkpoint("codec_output_witness")
                digest.update(block)
            final = path.stat(follow_symlinks=False)
            identity = (birth.st_dev, birth.st_ino, birth.st_size)
            if identity != (final.st_dev, final.st_ino, final.st_size):
                msg = f"Codec output changed before completion: {path.name}"
                raise ValueError(msg)
            result[path.name] = [*identity, digest.hexdigest()]
    sys.stdout.write(CODEC_OUTPUT + json.dumps(result, separators=(",", ":")) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("decode", "encode"))
    parser.add_argument("first", type=Path)
    parser.add_argument("second", type=Path)
    args = parser.parse_args()
    cancellation = StemCancellation.from_environment()
    if args.mode == "decode":
        begin_input_attempt(args.second.parent)
        try:
            decode(args.first, args.second, cancellation)
        finally:
            finish_input_attempt()
        _report_output_files((args.second,), cancellation)
        sys.stdout.write(input_completion_line() + "\n")
    else:
        encode(args.first, args.second, cancellation)
        _report_output_files(tuple(args.second / f"{stem}.wav" for stem in STEMS), cancellation)
        sys.stdout.write(encoder_completion_line() + "\n")


if __name__ == "__main__":
    try:
        main()
    except StemInferenceCancelledError as error:
        emit_cancellation_ack(error)
        sys.exit(CANCEL_EXIT_CODE)
