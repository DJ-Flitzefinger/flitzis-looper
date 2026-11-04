"""Physical channel labels must belong to the explicitly selected driver."""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.ui.render import audio_output as output_render

if TYPE_CHECKING:
    from flitzis_looper.controller.audio_output import AudioOutputController


@pytest.mark.parametrize("selected", [None, "driver-b", "driver-a"])
def test_named_channels_are_not_borrowed_from_another_or_auto_driver(selected: str | None) -> None:
    channels = [{"index": 2, "name": "Original physical output", "pcm_supported": True}]
    control = Mock()
    control.draft = AudioOutputOptions(backend="asio", device_id=selected)
    control.observed = {"asio_control": {"device_id": "driver-a", "channels": channels}}
    result = output_render._installed_channels(cast("AudioOutputController", control))
    assert result == (channels if selected == "driver-a" else [])
