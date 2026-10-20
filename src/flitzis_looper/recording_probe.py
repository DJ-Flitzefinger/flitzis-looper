"""Bounded FFprobe metadata checks without file, codec or publication authority.

Callers bound the JSON response and each compact packet line before passing it
here. Packet verification retains one line and scalar totals, never a take-sized
packet list. These checks do not parse MP3 bitstreams or prove lossy PCM identity.
"""

import re
from dataclasses import dataclass
from fractions import Fraction
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable

    from flitzis_looper.recording_options import RecordingOptions

_U64_MAX = (1 << 64) - 1
_DECIMAL = re.compile(r"(?:0|[1-9][0-9]{0,19})\Z")
_TIME_BASE = re.compile(r"([1-9][0-9]{0,19})/([1-9][0-9]{0,19})\Z")
_DECIMAL_TIME = re.compile(r"(?:0|[1-9][0-9]{0,19})(?:\.[0-9]{1,20})?\Z")
_WAV_CODECS = {"float32": ("pcm_f32le", 32), "pcm16": ("pcm_s16le", 16), "pcm24": ("pcm_s24le", 24)}
_MPEG1_RATES = {32000, 44100, 48000}
_MPEG2_RATES = {8000, 11025, 12000, 16000, 22050, 24000}
_PACKET_FIELDS = {"codec_type", "stream_index", "pts", "duration", "size"}
_TRIM_FIELDS = {"side_data_type", "skip_samples", "discard_padding"}
_ALL_PACKET_FIELDS = _PACKET_FIELDS | _TRIM_FIELDS
_TRIM_PREFIX = "side_datum/skip_samples:"


@dataclass(frozen=True, slots=True)
class RecordingStreamInfo:
    """Verified selected stream geometry and its exact timestamp unit."""

    codec: str
    rate: int
    channels: int
    bits: int
    time_base: Fraction


@dataclass(frozen=True, slots=True)
class RecordingMp3Info:
    """Packet timing/CBR and gapless-trim measurements, excluding container tags."""

    packet_count: int
    packet_samples: int
    encoded_frames: int
    skip_samples: int
    discard_padding: int
    frames: int
    packet_bytes: int
    bitrate_kbps: int


def _uint(value: object, name: str, *, positive: bool = False) -> int:
    if type(value) is int:
        result = value
    elif isinstance(value, str) and _DECIMAL.fullmatch(value) is not None:
        result = int(value)
    else:
        message = f"recording metadata {name} must be an unsigned integer"
        raise ValueError(message)
    if not int(positive) <= result <= _U64_MAX:
        message = f"recording metadata {name} is outside its unsigned extent"
        raise ValueError(message)
    return result


def _mapping(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or any(type(key) is not str for key in value):
        message = "recording metadata requires objects with string keys"
        raise ValueError(message)
    return cast("dict[str, object]", value)


def _time_base(value: object) -> Fraction:
    if not isinstance(value, str) or _TIME_BASE.fullmatch(value) is None:
        message = "recording stream time_base must be a positive integer fraction"
        raise ValueError(message)
    numerator, denominator = value.split("/")
    return Fraction(
        _uint(numerator, "time_base numerator", positive=True),
        _uint(denominator, "time_base denominator", positive=True),
    )


def _expected_geometry(rate: int, channels: int, frames: int) -> None:
    if type(rate) is not int or not 1 <= rate <= 768000:
        message = "recording verification requires a positive integer rate up to 768000"
        raise ValueError(message)
    if type(channels) is not int or not 1 <= channels <= 16:
        message = "recording verification requires 1 through 16 integer channels"
        raise ValueError(message)
    if type(frames) is not int or not 0 < frames * channels * 4 <= _U64_MAX:
        message = "recording verification requires a positive u64 float32 payload extent"
        raise ValueError(message)


def _optional_timing(stream: dict[str, object]) -> None:
    for name in ("start_pts", "initial_padding"):
        if name in stream:
            _uint(stream[name], name)
    for name in ("start_time", "duration"):
        if name in stream:
            value = stream[name]
            if not isinstance(value, str) or _DECIMAL_TIME.fullmatch(value) is None:
                message = f"recording metadata {name} must be a finite nonnegative decimal"
                raise ValueError(message)
            if name == "duration" and Fraction(value) <= 0:
                message = "recording metadata duration must be positive"
                raise ValueError(message)


def _stream_precision(stream: dict[str, object], codec: str, bits: int) -> None:
    actual = _uint(stream.get("bits_per_sample"), "bits_per_sample")
    if actual != (0 if codec in {"flac", "mp3"} else bits):
        message = "recording stream sample precision does not match the selected codec"
        raise ValueError(message)
    if codec == "flac" or "bits_per_raw_sample" in stream:
        raw = _uint(stream.get("bits_per_raw_sample"), "bits_per_raw_sample")
        if raw != bits:
            message = "recording stream raw precision does not match the selected codec"
            raise ValueError(message)


def _mp3_samples(rate: int, channels: int) -> int:
    if channels not in {1, 2} or rate not in _MPEG1_RATES | _MPEG2_RATES:
        message = "recording MP3 requires a supported MPEG rate and mono or stereo"
        raise ValueError(message)
    return 1152 if rate in _MPEG1_RATES else 576


def validate_recording_streams(
    metadata: object, options: RecordingOptions, *, rate: int, channels: int, frames: int
) -> RecordingStreamInfo:
    """Check one FFprobe JSON audio stream against the original take choices.

    Descriptive fields are not authority. WAV/FLAC duration is exact rational
    sample arithmetic; MP3's nominal bitrate is secondary to packet verification.
    """
    _expected_geometry(rate, channels, frames)
    streams = _mapping(metadata).get("streams")
    if not isinstance(streams, list) or len(streams) != 1:
        message = "recording must contain exactly one audio stream and no other streams"
        raise ValueError(message)
    stream = _mapping(streams[0])
    if stream.get("codec_type") != "audio" or _uint(stream.get("index"), "index") != 0:
        message = "recording requires the single index-zero audio stream"
        raise ValueError(message)
    if options.format == "wav":
        codec, bits = _WAV_CODECS[options.wav_encoding]
    else:
        codec = options.format
        bits = options.flac_bit_depth if codec == "flac" else 0
    if stream.get("codec_name") != codec:
        message = "recording codec does not match the selected format"
        raise ValueError(message)
    if (
        _uint(stream.get("sample_rate"), "sample_rate", positive=True),
        _uint(stream.get("channels"), "channels", positive=True),
    ) != (rate, channels):
        message = "recording stream rate or channels differ from the capture"
        raise ValueError(message)
    _stream_precision(stream, codec, bits)
    time_base = _time_base(stream.get("time_base"))
    duration = _uint(stream.get("duration_ts"), "duration_ts", positive=True)
    _optional_timing(stream)
    if codec == "mp3":
        _mp3_samples(rate, channels)
        if (
            _uint(stream.get("bit_rate"), "bit_rate", positive=True)
            != options.mp3_bitrate_kbps * 1000
        ):
            message = "recording MP3 nominal bitrate differs from the selected bitrate"
            raise ValueError(message)
    elif duration * time_base * rate != frames:
        message = "recording lossless duration differs from the captured frame extent"
        raise ValueError(message)
    return RecordingStreamInfo(codec, rate, channels, bits, time_base)


def _compact_fields(line: bytes) -> dict[str, str] | None:
    if type(line) is not bytes or len(line) > 8192:
        message = "recording packet line must be bytes bounded to 8192 bytes"
        raise ValueError(message)
    raw = line.rstrip(b"\r\n")
    if not raw:
        return None
    try:
        text = raw.decode("ascii")
    except UnicodeDecodeError as error:
        message = "recording packet line must contain ASCII metadata"
        raise ValueError(message) from error
    fields: dict[str, str] = {}
    for item in text.split("|"):
        key, separator, value = item.partition("=")
        prefixed = key.startswith(_TRIM_PREFIX)
        key = key.removeprefix(_TRIM_PREFIX)
        if (
            not separator
            or key not in _ALL_PACKET_FIELDS
            or key in fields
            or (prefixed and key not in _TRIM_FIELDS)
        ):
            message = "recording packet has malformed, unknown or duplicate fields"
            raise ValueError(message)
        fields[key] = value
    if _PACKET_FIELDS.difference(fields):
        message = "recording packet is missing required fields"
        raise ValueError(message)
    return fields


@dataclass(frozen=True, slots=True)
class _Packet:
    pts: int
    duration: int
    size: int
    skip: int
    discard: int
    trim: bool


def _packet(fields: dict[str, str]) -> _Packet:
    if fields["codec_type"] != "audio" or _uint(fields["stream_index"], "stream_index") != 0:
        message = "recording packet must belong to the single audio stream"
        raise ValueError(message)
    trim = bool(fields.keys() & _TRIM_FIELDS)
    if trim and not {"skip_samples", "discard_padding"} <= fields.keys():
        message = "recording packet trim must include skip_samples and discard_padding"
        raise ValueError(message)
    if "side_data_type" in fields and fields["side_data_type"] != "Skip Samples":
        message = "recording packet has unsupported trim side data"
        raise ValueError(message)
    return _Packet(
        _uint(fields["pts"], "pts"),
        _uint(fields["duration"], "duration", positive=True),
        _uint(fields["size"], "size", positive=True),
        _uint(fields["skip_samples"], "skip_samples") if trim else 0,
        _uint(fields["discard_padding"], "discard_padding") if trim else 0,
        trim,
    )


@dataclass(slots=True)
class _Packets:
    count: int = 0
    next_pts: int = 0
    samples: int = 0
    size: int = 0
    first_skip: int = 0
    last_discard: int = 0
    last_trim: bool = False

    def include(self, packet: _Packet, info: RecordingStreamInfo, bitrate: int) -> None:
        if packet.pts != self.next_pts or (self.count and self.last_discard):
            message = "recording packet timing is not contiguous or discard is not terminal"
            raise ValueError(message)
        if not self.count:
            if not packet.trim:
                message = "recording MP3 is missing its first packet trim metadata"
                raise ValueError(message)
            self.first_skip = packet.skip
        elif packet.skip:
            message = "recording MP3 skip_samples is only allowed in its first packet"
            raise ValueError(message)
        samples = packet.duration * info.time_base * info.rate
        if samples != _mp3_samples(info.rate, info.channels):
            message = "recording MP3 packet duration is not an exact MPEG sample extent"
            raise ValueError(message)
        ideal = Fraction(bitrate * 1000 * int(samples), 8 * info.rate)
        lower = ideal.numerator // ideal.denominator
        upper = -(-ideal.numerator // ideal.denominator)
        if packet.size not in {lower, upper}:
            message = "recording MP3 packet size does not match selected CBR framing"
            raise ValueError(message)
        self.count += 1
        self.next_pts += packet.duration
        self.samples += int(samples)
        self.size += packet.size
        self.last_discard, self.last_trim = packet.discard, packet.trim
        if max(self.next_pts, self.samples, self.size, self.count) > _U64_MAX:
            message = "recording MP3 packet totals exceed unsigned 64-bit extents"
            raise ValueError(message)


def _validate_mp3_stream(info: RecordingStreamInfo) -> None:
    if info.codec != "mp3" or type(info.bits) is not int or info.bits != 0:
        message = "recording packet verification requires a valid MP3 stream"
        raise ValueError(message)
    if (
        type(info.time_base) is not Fraction
        or info.time_base <= 0
        or max(info.time_base.numerator, info.time_base.denominator) > _U64_MAX
    ):
        message = "recording packet verification requires a valid MP3 stream"
        raise ValueError(message)


def verify_mp3_packets(
    lines: Iterable[bytes],
    info: RecordingStreamInfo,
    *,
    frames: int,
    bitrate_kbps: int,
    checkpoint: Callable[[], None] | None = None,
) -> RecordingMp3Info:
    """Verify bounded compact packet lines, exact CBR timing and gapless extent.

    The caller must also bound reads before a line enters this helper. Known
    skip-samples prefixes and older plain trim keys are accepted; all other
    fields, duplicate keys, missing trim and discontinuous timelines fail.
    """
    _expected_geometry(info.rate, info.channels, frames)
    _validate_mp3_stream(info)
    if type(bitrate_kbps) is not int or bitrate_kbps not in {128, 192, 256, 320}:
        message = "recording MP3 requires a selected CBR bitrate"
        raise ValueError(message)
    packet_samples = _mp3_samples(info.rate, info.channels)
    packets = _Packets()
    if checkpoint is not None:
        checkpoint()
    for line in lines:
        if checkpoint is not None:
            checkpoint()
        fields = _compact_fields(line)
        if fields is not None:
            packets.include(_packet(fields), info, bitrate_kbps)
    if not packets.count or not packets.last_trim:
        message = "recording MP3 is missing packets or terminal trim metadata"
        raise ValueError(message)
    if packets.samples - packets.first_skip - packets.last_discard != frames:
        message = "recording MP3 trimmed packet extent differs from captured frames"
        raise ValueError(message)
    if checkpoint is not None:
        checkpoint()
    return RecordingMp3Info(
        packets.count,
        packet_samples,
        packets.samples,
        packets.first_skip,
        packets.last_discard,
        frames,
        packets.size,
        bitrate_kbps,
    )
