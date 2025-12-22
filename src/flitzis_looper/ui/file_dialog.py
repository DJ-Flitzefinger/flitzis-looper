from typing import TYPE_CHECKING

from imgui_bundle import im_file_dialog as ifd

if TYPE_CHECKING:
    from flitzis_looper.app import FlitzisLooperApp


def _get_key(sample_id: int) -> str:
    return f"file_dialog_{sample_id}"


def open_file_dialog(sample_id: int) -> None:
    ifd.FileDialog.instance().open(
        _get_key(sample_id),
        "Load Audio",
        filter="Audio file (*.wav;*.aiff;*.aif;*.flac;*.mp3;*.ogg){.wav,.aiff,.aif,.flac,.mp3}",
    )


def check_file_dialog(app: FlitzisLooperApp, sample_id: int) -> None:
    if ifd.FileDialog.instance().is_done(_get_key(sample_id)):
        try:
            if ifd.FileDialog.instance().has_result():
                res = ifd.FileDialog.instance().get_result()
                app.load_sample(sample_id, res.path())
        finally:
            ifd.FileDialog.instance().close()
            app.close_file_dialog()
