"""Explicit recording codec commands, independent of process and take authority."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.recording_options import RecordingOptions, WavEncoding

_RAW_FORMAT = {"float32": "f32le", "pcm16": "s16le", "pcm24": "s24le"}
_PCM_CODEC = {"float32": "pcm_f32le", "pcm16": "pcm_s16le", "pcm24": "pcm_s24le"}


def recording_pcm_encoding(options: RecordingOptions) -> WavEncoding:
    """Select fixed PCM precision before encoding, without a format fallback."""
    if options.format == "wav":
        return options.wav_encoding
    if options.format == "flac" and options.flac_bit_depth == 16:
        return "pcm16"
    return "pcm24"


def validate_recording_geometry(rate: int, channels: int, frames: int) -> None:
    """Bound integer geometry; actual codec support still requires preflight."""
    if type(rate) is not int or not 1 <= rate <= 768000:
        message = "recording requires an integer output rate in 1..768000 Hz"
        raise ValueError(message)
    if type(channels) is not int or not 1 <= channels <= 16:
        message = "recording requires an integer output channel count in 1..16"
        raise ValueError(message)
    if type(frames) is not int or frames <= 0 or frames * channels * 4 > 2**64 - 1:
        message = "recording requires a nonempty unsigned-64-bit float extent"
        raise ValueError(message)


def recording_encode_command(
    executable: Path,
    source: Path,
    options: RecordingOptions,
    *,
    rate: int,
    channels: int,
) -> list[str]:
    """Encode already quantized PCM to caller-owned seekable stdout (FFmpeg fd:)."""
    validate_recording_geometry(rate, channels, 1)
    encoding = recording_pcm_encoding(options)
    command = [
        str(executable),
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-f",
        _RAW_FORMAT[encoding],
        "-ar",
        str(rate),
        "-ac",
        str(channels),
        "-i",
        str(source),
        "-map",
        "0:a:0",
        "-map_metadata",
        "-1",
        "-ar",
        str(rate),
        "-ac",
        str(channels),
        "-dither_method",
        "none",
    ]
    if options.format == "wav":
        command.extend(["-c:a", _PCM_CODEC[encoding], "-rf64", "auto"])
    elif options.format == "flac":
        command.extend([
            "-c:a",
            "flac",
            "-sample_fmt",
            "s16" if encoding == "pcm16" else "s32",
            "-bits_per_raw_sample",
            str(options.flac_bit_depth),
            "-compression_level",
            str(options.flac_compression),
        ])
    else:
        command.extend([
            "-c:a",
            "libmp3lame",
            "-b:a",
            f"{options.mp3_bitrate_kbps}k",
            "-write_xing",
            "1",
        ])
    return [*command, "-f", options.format, "fd:"]


def recording_decode_command(
    executable: Path, source: Path, options: RecordingOptions
) -> list[str]:
    """Decode without rate/channel substitution; verification checks actual extent."""
    encoding = "float32" if options.format == "mp3" else recording_pcm_encoding(options)
    return [
        str(executable),
        "-hide_banner",
        "-loglevel",
        "error",
        "-xerror",
        "-nostdin",
        "-i",
        str(source),
        "-map",
        "0:a:0",
        "-c:a",
        _PCM_CODEC[encoding],
        "-f",
        _RAW_FORMAT[encoding],
        "fd:",
    ]


def recording_stream_command(executable: Path, source: Path) -> list[str]:
    """Ask for bounded stream metadata, without a whole-take packet array."""
    return [str(executable), "-v", "error", "-show_streams", "-of", "json", str(source)]


def recording_packet_command(executable: Path, source: Path) -> list[str]:
    """Write MP3 packet evidence as streamable compact records."""
    return [
        str(executable),
        "-v",
        "error",
        "-select_streams",
        "a:0",
        "-show_packets",
        "-show_entries",
        (
            "packet=codec_type,stream_index,pts,duration,size:"
            "packet_side_data=side_data_type,skip_samples,discard_padding"
        ),
        "-of",
        "compact=p=0:nk=0",
        str(source),
    ]
