from flitzis_looper.ui.render.performance_view import stem_grid_indicator_label


def test_stem_grid_indicator_labels_are_compact() -> None:
    assert stem_grid_indicator_label(None) is None
    assert stem_grid_indicator_label("available") == "ST"
    assert stem_grid_indicator_label("generating") == "..."
    assert stem_grid_indicator_label("blocked") == "BLK"
    assert stem_grid_indicator_label("error") == "!"
