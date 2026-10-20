"""Off-realtime recording WAV geometry on a caller-owned, seekable binary file.

The caller retains file identity, stable contents and lifetime. Inspection does
not close the file, scan its PCM, verify a take or grant publication authority.
"""

import wave
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper import pcm16_wav
from flitzis_looper.analysis import loop_capture_wav

if TYPE_CHECKING:
    from typing import BinaryIO

    from flitzis_looper.recording_options import WavEncoding

_ENCODINGS: dict[tuple[int, int], WavEncoding] = {
    (3, 32): "float32",
    (1, 16): "pcm16",
    (1, 24): "pcm24",
}


@dataclass(frozen=True, slots=True)
class RecordingWavInfo:
    """Observed container geometry, without PCM integrity or Native/take authority."""

    encoding: WavEncoding
    channels: int
    rate: int
    frames: int
    data_offset: int
    data_bytes: int
    file_bytes: int
    rf64: bool


class _RecordingChunks(pcm16_wav._Chunks):
    """Use the existing RIFF/RF64 scanner with the existing float format validator."""

    __slots__ = ("encoding",)

    encoding: WavEncoding

    def _fmt(self, handle: BinaryIO, length: int) -> None:
        if self.shape is not None or length < 16:
            message = "invalid or repeated recording WAV format"
            raise wave.Error(message)
        # Parsing reads at most 40 bytes even when the chunk has an extension.
        # The inherited scanner alone owns chunk skipping and RF64 extents.
        raw = pcm16_wav._exact(handle, min(length, 40))
        try:
            tag, channels, rate, bits = loop_capture_wav._format(raw)
        except ValueError as error:
            raise wave.Error(str(error)) from error
        encoding = _ENCODINGS.get((tag, bits))
        if encoding is None:
            message = "recording WAV requires float32, PCM16 or PCM24"
            raise wave.Error(message)
        self.encoding = encoding
        self.shape = channels, rate, bits // 8


def inspect_recording_wav(handle: BinaryIO) -> RecordingWavInfo:
    """Inspect complete nonempty RIFF/RF64 geometry without reading PCM payload.

    The file must be seekable and stable for the inspection. Its original
    position is restored on success or rejection. Reads use the shared scanner's
    fixed-size headers; work may still scale with the number of metadata chunks.
    The reused format validator supports at most 16 channels and 768000 Hz.
    """
    position = handle.tell()
    try:
        handle.seek(0)
        rf64, extent = pcm16_wav._file_extent(handle)
        chunks = _RecordingChunks(rf64, extent)
        while handle.tell() < extent:
            chunks.scan_chunk(handle)
        geometry = chunks.geometry()
        if geometry.data_bytes == 0:
            message = "recording WAV must contain at least one complete frame"
            raise wave.Error(message)
        return RecordingWavInfo(
            encoding=chunks.encoding,
            channels=geometry.channels,
            rate=geometry.rate,
            frames=geometry.data_bytes // (geometry.channels * geometry.width),
            data_offset=geometry.data_start,
            data_bytes=geometry.data_bytes,
            file_bytes=geometry.file_bytes,
            rf64=rf64,
        )
    finally:
        handle.seek(position)
