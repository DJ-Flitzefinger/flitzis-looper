"""Decode the complete BS-RoFormer source with the original FFmpeg contract."""

import argparse
import json
import math
import sys
from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.bs_input_provenance import (
    begin_input_attempt,
    finish_input_attempt,
    input_completion_line,
    run_input_tool,
)
from flitzis_looper.offline_runtime import ffmpeg_command
from flitzis_looper.stem_cancellation import (
    CANCEL_EXIT_CODE,
    StemCancellation,
    StemInferenceCancelledError,
    check_cancelled,
    emit_cancellation_ack,
)

if TYPE_CHECKING:
    from collections.abc import Callable

FRAME_BYTES = 8
SAMPLE_RATE = 44_100


def decode_source(
    source: Path, destination: Path, *, checkpoint: Callable[[str], None] | None = None
) -> int:
    """Probe and decode to a complete interleaved stereo float32 scratch file."""
    check_cancelled(checkpoint, "before_source_decode")
    probe = run_input_tool(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=sample_rate,channels,duration:format=duration",
            "-of",
            "json",
            str(source),
        ],
        checkpoint=checkpoint,
    )
    metadata = json.loads(probe.stdout)
    streams = metadata.get("streams", [])
    if len(streams) != 1:
        msg = "Source must contain a decodable audio stream"
        raise ValueError(msg)
    stream = streams[0]
    channels = int(stream.get("channels", 0))
    rate = int(stream.get("sample_rate", 0))
    duration = float(stream.get("duration", metadata.get("format", {}).get("duration", 0)))
    if not 1 <= channels <= 64 or rate <= 0 or not math.isfinite(duration) or duration <= 0:
        msg = "Source has invalid audio metadata"
        raise ValueError(msg)
    channel_conversion = ["-af", "pan=stereo|c0=c0|c1=c0"] if channels == 1 else []
    run_input_tool(
        ffmpeg_command([
            "ffmpeg",
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-i",
            str(source),
            "-map",
            "0:a:0",
            "-vn",
            "-threads",
            "1",
            *channel_conversion,
            "-ac",
            "2",
            "-ar",
            str(SAMPLE_RATE),
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            str(destination),
        ]),
        checkpoint=checkpoint,
    )
    check_cancelled(checkpoint, "after_source_decode")
    decoded_bytes = destination.stat().st_size
    if decoded_bytes == 0 or decoded_bytes % FRAME_BYTES:
        msg = "Decoded source has incomplete stereo float32 PCM"
        raise ValueError(msg)
    return decoded_bytes // FRAME_BYTES


def main() -> None:
    """Decode inside the existing owned Stem Job and emit its input-tool witness."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    cancellation = StemCancellation.from_environment()
    begin_input_attempt(args.destination.parent)
    try:
        decode_source(args.source, args.destination, checkpoint=cancellation.checkpoint)
    finally:
        finish_input_attempt()
    cancellation.checkpoint("before_decode_completion")
    sys.stdout.write(input_completion_line() + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    except StemInferenceCancelledError as error:
        emit_cancellation_ack(error)
        sys.exit(CANCEL_EXIT_CODE)
