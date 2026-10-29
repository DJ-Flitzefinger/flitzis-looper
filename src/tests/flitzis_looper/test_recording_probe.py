"""Independent FFprobe metadata arithmetic; no encoder or bitstream acceptance."""

from dataclasses import FrozenInstanceError, replace
from fractions import Fraction
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.recording_probe import (
    RecordingMp3Info,
    RecordingStreamInfo,
    validate_recording_streams,
    verify_mp3_packets,
)

if TYPE_CHECKING:
    from collections.abc import Iterator


def _metadata(fields: dict[str, object] | None = None) -> dict[str, object]:
    stream: dict[str, object] = {
        "index": 0,
        "codec_type": "audio",
        "codec_name": "pcm_f32le",
        "sample_rate": "48000",
        "channels": 2,
        "bits_per_sample": 32,
        "time_base": "1/48000",
        "duration_ts": 10001,
        "duration": "0.208354",
    }
    stream.update(fields or {})
    return {"streams": [stream]}


def _mp3_info() -> RecordingStreamInfo:
    return validate_recording_streams(
        _metadata({
            "codec_name": "mp3",
            "bits_per_sample": 0,
            "time_base": "1/14112000",
            "duration_ts": 3386880,
            "duration": "0.240000",
            "bit_rate": "320000",
        }),
        RecordingOptions(format="mp3"),
        rate=48000,
        channels=2,
        frames=10001,
    )


def _line(index: int, *, trim: tuple[int, int] | None = None, plain: bool = False) -> bytes:
    text = f"codec_type=audio|stream_index=0|pts={index * 338688}|duration=338688|size=960"
    if trim is not None:
        prefix = "" if plain else "side_datum/skip_samples:"
        text += (
            f"|{prefix}side_data_type=Skip Samples|{prefix}skip_samples={trim[0]}"
            f"|{prefix}discard_padding={trim[1]}"
        )
    return text.encode("ascii")


def _ten_packets(*, plain: bool = False) -> list[bytes]:
    return [
        _line(
            index, trim=(1105, 0) if index == 0 else (0, 414) if index == 9 else None, plain=plain
        )
        for index in range(10)
    ]


@pytest.mark.parametrize(
    ("options", "fields", "bits"),
    [
        (RecordingOptions(format="wav"), {}, 32),
        (
            RecordingOptions(format="wav", wav_encoding="pcm16"),
            {"codec_name": "pcm_s16le", "bits_per_sample": 16},
            16,
        ),
        (
            RecordingOptions(format="wav", wav_encoding="pcm24"),
            {"codec_name": "pcm_s24le", "bits_per_sample": 24, "bits_per_raw_sample": "24"},
            24,
        ),
        (
            RecordingOptions(format="flac", flac_bit_depth=16),
            {"codec_name": "flac", "bits_per_sample": 0, "bits_per_raw_sample": "16"},
            16,
        ),
        (
            RecordingOptions(format="flac"),
            {"codec_name": "flac", "bits_per_sample": 0, "bits_per_raw_sample": "24"},
            24,
        ),
    ],
)
def test_selected_lossless_codec_precision_and_exact_rational_duration(
    options: RecordingOptions, fields: dict[str, object], bits: int
) -> None:
    info = validate_recording_streams(
        _metadata(fields), options, rate=48000, channels=2, frames=10001
    )
    assert info == RecordingStreamInfo(
        str(fields.get("codec_name", "pcm_f32le")), 48000, 2, bits, Fraction(1, 48000)
    )


def test_exact_duration_uses_timebase_not_rounded_descriptive_duration() -> None:
    info = validate_recording_streams(
        _metadata({"time_base": "2/192000", "duration_ts": "20002", "duration": "0.2"}),
        RecordingOptions(format="wav"),
        rate=48000,
        channels=2,
        frames=10001,
    )
    assert info.time_base == Fraction(1, 96000)
    field = "time_base"
    with pytest.raises(FrozenInstanceError):
        setattr(info, field, Fraction(1, 1))


def test_mp3_nominal_duration_does_not_replace_packet_trim_verification() -> None:
    info = _mp3_info()
    assert info == RecordingStreamInfo("mp3", 48000, 2, 0, Fraction(1, 14112000))
    result = verify_mp3_packets(_ten_packets(), info, frames=10001, bitrate_kbps=320)
    assert result == RecordingMp3Info(10, 1152, 11520, 1105, 414, 10001, 9600, 320)
    field = "frames"
    with pytest.raises(FrozenInstanceError):
        setattr(result, field, 10000)


@pytest.mark.parametrize(
    "metadata",
    [
        None,
        [],
        {},
        {"streams": []},
        {"streams": [None]},
        {"streams": [{"codec_type": "video"}]},
        {"streams": [{}, {}]},
        {"streams": (object(),)},
        {1: "not a string key"},
    ],
)
def test_malformed_or_additional_streams_fail(metadata: object) -> None:
    with pytest.raises(ValueError, match="recording"):
        validate_recording_streams(
            metadata, RecordingOptions(format="wav"), rate=48000, channels=2, frames=10001
        )


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("index", 1),
        ("index", True),
        ("codec_type", "video"),
        ("codec_name", "pcm_s32le"),
        ("sample_rate", 44100),
        ("sample_rate", 48000.0),
        ("sample_rate", True),
        ("sample_rate", "048000"),
        ("sample_rate", "48000.0"),
        ("sample_rate", "NaN"),
        ("sample_rate", "inf"),
        ("sample_rate", "18446744073709551616"),
        ("channels", 0),
        ("channels", 3),
        ("channels", 2.0),
        ("channels", True),
        ("bits_per_sample", 24),
        ("bits_per_sample", 32.0),
        ("bits_per_raw_sample", "24"),
        ("time_base", "0/48000"),
        ("time_base", "1/0"),
        ("time_base", "-1/48000"),
        ("time_base", "1 / 48000"),
        ("time_base", "nan"),
        ("time_base", 1.0),
        ("duration_ts", 0),
        ("duration_ts", -1),
        ("duration_ts", 10001.0),
        ("duration_ts", "NaN"),
        ("duration_ts", 10000),
        ("duration_ts", 10002),
        ("duration", "NaN"),
        ("duration", "Infinity"),
        ("duration", "-0.1"),
        ("duration", "0.000000"),
        ("start_time", "-0.1"),
        ("start_pts", -1),
        ("initial_padding", True),
    ],
)
def test_stream_known_numeric_fields_are_strict_and_geometry_cannot_drift(
    field: str, value: object
) -> None:
    with pytest.raises(ValueError, match="recording"):
        validate_recording_streams(
            _metadata({field: value}),
            RecordingOptions(format="wav"),
            rate=48000,
            channels=2,
            frames=10001,
        )


@pytest.mark.parametrize("raw_bits", [None, "0", "16", 24.0, True])
def test_flac_requires_actual_selected_raw_precision(raw_bits: object) -> None:
    with pytest.raises(ValueError, match="recording"):
        validate_recording_streams(
            _metadata({
                "codec_name": "flac",
                "bits_per_sample": 0,
                "bits_per_raw_sample": raw_bits,
            }),
            RecordingOptions(format="flac"),
            rate=48000,
            channels=2,
            frames=10001,
        )


@pytest.mark.parametrize("bitrate", [None, "319999", "192000", "320000.0", 320000.0, True])
def test_mp3_nominal_bitrate_must_match_even_though_it_is_only_secondary(
    bitrate: object,
) -> None:
    with pytest.raises(ValueError, match="recording"):
        validate_recording_streams(
            _metadata({
                "codec_name": "mp3",
                "bits_per_sample": 0,
                "time_base": "1/14112000",
                "bit_rate": bitrate,
            }),
            RecordingOptions(format="mp3"),
            rate=48000,
            channels=2,
            frames=10001,
        )


@pytest.mark.parametrize("plain", [False, True])
def test_packet_prefix_versions_and_blank_cr_lines_preserve_exact_trim(*, plain: bool) -> None:
    lines = _ten_packets(plain=plain)
    lines[1:1] = [b"\r", b"\r\n", b"\n", b""]
    result = verify_mp3_packets(lines, _mp3_info(), frames=10001, bitrate_kbps=320)
    assert (result.packet_count, result.frames, result.packet_bytes) == (10, 10001, 9600)


def test_cbr_padding_may_alternate_floor_and_ceil_sizes_at_44100_hz() -> None:
    info = RecordingStreamInfo("mp3", 44100, 2, 0, Fraction(1, 14112000))
    lines = [
        (
            b"codec_type=audio|stream_index=0|pts=0|duration=368640|size=1044"
            b"|skip_samples=1105|discard_padding=0"
        ),
        b"codec_type=audio|stream_index=0|pts=368640|duration=368640|size=1045",
        (
            b"codec_type=audio|stream_index=0|pts=737280|duration=368640|size=1045"
            b"|skip_samples=0|discard_padding=351"
        ),
    ]
    result = verify_mp3_packets(lines, info, frames=2000, bitrate_kbps=320)
    assert (result.encoded_frames, result.skip_samples, result.discard_padding) == (3456, 1105, 351)
    assert result.packet_bytes == 3134


def test_lower_mpeg_rate_requires_576_samples_per_packet_and_exact_trim() -> None:
    info = RecordingStreamInfo("mp3", 24000, 1, 0, Fraction(1, 14112000))
    lines = [
        _line(index, trim=(1105, 0) if index == 0 else (0, 199) if index == 3 else None).replace(
            b"size=960", b"size=384"
        )
        for index in range(4)
    ]
    result = verify_mp3_packets(lines, info, frames=1000, bitrate_kbps=128)
    assert (result.packet_samples, result.encoded_frames, result.frames) == (576, 2304, 1000)


def test_one_packet_can_explicitly_carry_both_initial_and_terminal_trim() -> None:
    result = verify_mp3_packets(
        [_line(0, trim=(1105, 46))], _mp3_info(), frames=1, bitrate_kbps=320
    )
    assert (result.packet_count, result.encoded_frames, result.frames) == (1, 1152, 1)


@pytest.mark.parametrize(
    ("old", "new"),
    [
        (b"codec_type=audio", b"codec_type=video"),
        (b"stream_index=0", b"stream_index=1"),
        (b"pts=0", b"pts=1"),
        (b"pts=0", b"pts=-1"),
        (b"pts=0", b"pts=0.0"),
        (b"pts=0", b"pts=NaN"),
        (b"duration=338688", b"duration=0"),
        (b"duration=338688", b"duration=-1"),
        (b"duration=338688", b"duration=338687"),
        (b"duration=338688", b"duration=677376"),
        (b"size=960", b"size=0"),
        (b"size=960", b"size=959"),
        (b"size=960", b"size=961"),
        (b"size=960", b"size=960.0"),
        (b"skip_samples=1105", b"skip_samples=-1105"),
        (b"skip_samples=1105", b"skip_samples=1106"),
        (b"side_data_type=Skip Samples", b"side_data_type=Other"),
        (b"discard_padding=0", b"discard_padding=1"),
    ],
)
def test_false_timing_bitrate_and_trim_fields_are_rejected(old: bytes, new: bytes) -> None:
    lines = _ten_packets()
    lines[0] = lines[0].replace(old, new)
    with pytest.raises(ValueError, match="recording"):
        verify_mp3_packets(lines, _mp3_info(), frames=10001, bitrate_kbps=320)


@pytest.mark.parametrize(
    "change", ["duplicate", "gap", "reorder", "late_skip", "no_first", "no_last"]
)
def test_packet_sequence_and_trim_lifetime_cannot_be_reused_or_moved(change: str) -> None:
    lines = _ten_packets()
    if change == "duplicate":
        lines.insert(1, lines[0])
    elif change == "gap":
        lines[1] = lines[1].replace(b"pts=338688", b"pts=338689")
    elif change == "reorder":
        lines[1], lines[2] = lines[2], lines[1]
    elif change == "late_skip":
        lines[1] = _line(1, trim=(1, 0))
    elif change == "no_first":
        lines[0] = _line(0)
    else:
        lines[-1] = _line(9)
    with pytest.raises(ValueError, match="recording"):
        verify_mp3_packets(lines, _mp3_info(), frames=10001, bitrate_kbps=320)


def test_same_nominal_and_average_bitrate_does_not_accept_variable_packet_sizes() -> None:
    lines = _ten_packets()
    lines[3] = lines[3].replace(b"size=960", b"size=920")
    lines[4] = lines[4].replace(b"size=960", b"size=1000")
    with pytest.raises(ValueError, match="CBR"):
        verify_mp3_packets(lines, _mp3_info(), frames=10001, bitrate_kbps=320)


@pytest.mark.parametrize(
    "suffix",
    [
        b"|pts=0",
        b"|unknown=1",
        b"|skip_samples=1105",
        b"|duration",
        b"|side_datum/other:skip_samples=1105",
        b"|side_datum/skip_samples:pts=0",
        b"|",
        b"\xff",
    ],
)
def test_unknown_duplicate_or_malformed_compact_fields_fail(suffix: bytes) -> None:
    with pytest.raises(ValueError, match="recording"):
        verify_mp3_packets(
            [_line(0, trim=(1105, 46)) + suffix], _mp3_info(), frames=1, bitrate_kbps=320
        )


@pytest.mark.parametrize("line", [b"x" * 8193, b"codec_type=audio", b"packet|pts=0"])
def test_packet_read_bound_and_required_shape_are_enforced_again_in_helper(line: bytes) -> None:
    with pytest.raises(ValueError, match="recording"):
        verify_mp3_packets([line], _mp3_info(), frames=1, bitrate_kbps=320)


@pytest.mark.parametrize("lines", [[], [b"\r\n", b"\r", b""]])
def test_empty_packet_stream_does_not_prove_a_take(lines: list[bytes]) -> None:
    with pytest.raises(ValueError, match="missing"):
        verify_mp3_packets(lines, _mp3_info(), frames=1, bitrate_kbps=320)


@pytest.mark.parametrize("frames", [10000, 10002])
def test_stale_captured_frame_extent_does_not_match_valid_other_take_packets(frames: int) -> None:
    with pytest.raises(ValueError, match="extent"):
        verify_mp3_packets(_ten_packets(), _mp3_info(), frames=frames, bitrate_kbps=320)


@pytest.mark.parametrize(
    "info",
    [
        RecordingStreamInfo("flac", 48000, 2, 0, Fraction(1, 14112000)),
        RecordingStreamInfo("mp3", 48000, 2, 16, Fraction(1, 14112000)),
        RecordingStreamInfo("mp3", 48000, 2, 0, Fraction(0)),
        RecordingStreamInfo("mp3", 48000, 2, 0, Fraction(-1, 14112000)),
        RecordingStreamInfo("mp3", 48000, 3, 0, Fraction(1, 14112000)),
        RecordingStreamInfo("mp3", 47000, 2, 0, Fraction(1, 14112000)),
    ],
)
def test_packet_verification_rejects_incompatible_stream_info(info: RecordingStreamInfo) -> None:
    with pytest.raises(ValueError, match="recording"):
        verify_mp3_packets(_ten_packets(), info, frames=10001, bitrate_kbps=320)


@pytest.mark.parametrize("bitrate", [0, 160, 321, True, 320.0, "320"])
def test_packet_verification_requires_the_original_supported_bitrate(bitrate: object) -> None:
    with pytest.raises(ValueError, match="recording"):
        verify_mp3_packets(
            _ten_packets(), _mp3_info(), frames=10001, bitrate_kbps=cast("int", bitrate)
        )


def test_long_generator_is_consumed_incrementally_with_only_scalar_totals() -> None:
    count = 100000
    produced = observed = 0

    def checkpoint() -> None:
        nonlocal observed
        observed = produced

    def lines() -> Iterator[bytes]:
        nonlocal produced
        for index in range(count):
            produced += 1
            yield _line(
                index, trim=(1105, 0) if index == 0 else (0, 414) if index == count - 1 else None
            )
            assert observed == produced, "packets must be checked before requesting the next"

    frames = 115200000 - 1105 - 414
    result = verify_mp3_packets(
        lines(), _mp3_info(), frames=frames, bitrate_kbps=320, checkpoint=checkpoint
    )
    assert result.packet_count == produced == observed == count
    assert result.encoded_frames == 115200000
    assert result.packet_bytes == 96000000
    assert result.frames == 115198481


@pytest.mark.parametrize(("cancel_at", "yielded"), [(1, 0), (3, 2), (12, 10)])
def test_checkpoint_cancels_before_during_or_after_packet_consumption(
    cancel_at: int, yielded: int
) -> None:
    calls = produced = 0

    def checkpoint() -> None:
        nonlocal calls
        calls += 1
        if calls == cancel_at:
            message = "cancel packet verification"
            raise InterruptedError(message)

    def lines() -> Iterator[bytes]:
        nonlocal produced
        for line in _ten_packets():
            produced += 1
            yield line

    with pytest.raises(InterruptedError, match="cancel packet"):
        verify_mp3_packets(
            lines(), _mp3_info(), frames=10001, bitrate_kbps=320, checkpoint=checkpoint
        )
    assert calls == cancel_at
    assert produced == yielded


def test_trim_tuple_must_be_complete_even_when_its_numbers_are_zero() -> None:
    line = b"codec_type=audio|stream_index=0|pts=0|duration=338688|size=960|skip_samples=0"
    with pytest.raises(ValueError, match="trim"):
        verify_mp3_packets([line], _mp3_info(), frames=1152, bitrate_kbps=320)


def test_nonintegral_packet_sample_duration_is_not_rounded_into_valid_extent() -> None:
    info = replace(_mp3_info(), time_base=Fraction(1, 14112001))
    with pytest.raises(ValueError, match="exact MPEG"):
        verify_mp3_packets(_ten_packets(), info, frames=10001, bitrate_kbps=320)
