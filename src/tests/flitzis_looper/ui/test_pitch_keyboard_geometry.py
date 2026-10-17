import math
from itertools import pairwise

import pytest

from flitzis_looper.ui.render.pitch_keyboard import piano_geometry, piano_hit_test


def test_corrected_pitch_reference_and_detected_original_marker_stay_distinct() -> None:
    geometry = piano_geometry(2, original_root=0, width=840.0, height=140.0)
    selected_d = next(item for item in geometry if item.key.value == 0)
    original_c = next(item for item in geometry if item.is_original)
    assert selected_d.semitone_index == 14
    assert selected_d.key.label.startswith("D\n")
    assert original_c.semitone_index == 12
    assert original_c.key.value == -2
    assert original_c.key.label.startswith("C\n")
    corrected_unknown_original = piano_geometry(2, original_root=None, width=840.0, height=140.0)
    assert not any(item.is_original for item in corrected_unknown_original)


@pytest.mark.parametrize("root", range(12))
def test_three_octaves_preserve_all_signed_targets_and_middle_original(root: int) -> None:
    geometry = piano_geometry(root, width=840.0, height=140.0)

    assert len(geometry) == 36
    assert sum(item.key.black is False for item in geometry) == 21
    assert sum(item.key.black is True for item in geometry) == 15
    assert [item.key.value for item in geometry] == list(range(-12 - root, 24 - root))
    assert len({item.key.label for item in geometry}) == 36
    originals = [item for item in geometry if item.is_original]
    assert len(originals) == 1
    assert originals[0].semitone_index == 12 + root
    assert originals[0].key.value == 0
    assert "original octave" in originals[0].key.label

    for shift in (-12, 0, 12):
        item = next(item for item in geometry if item.key.value == shift)
        assert item.semitone_index == 12 + root + shift
        assert piano_hit_test(geometry, item.center_x, item.height / 2.0) is item


@pytest.mark.parametrize("width", [63.0, 399.5, 840.0, 1333.3])
@pytest.mark.parametrize("height", [84.0, 136.0, 220.0])
def test_responsive_keys_fit_available_area_without_gaps(width: float, height: float) -> None:
    geometry = piano_geometry(5, width=width, height=height)
    whites = [item for item in geometry if item.key.black is False]
    blacks = [item for item in geometry if item.key.black is True]

    assert whites[0].x == 0.0
    assert whites[-1].x + whites[-1].width == pytest.approx(width)
    assert all(item.height == height for item in whites)
    assert all(0.0 < item.width < whites[0].width for item in blacks)
    assert all(0.0 < item.height < height for item in blacks)
    assert all(0.0 <= item.x < item.x + item.width <= width for item in geometry)
    for left, right in pairwise(whites):
        assert left.x + left.width == right.x
        assert piano_hit_test(geometry, right.x, height - 1.0) is right
    for item in whites:
        assert piano_hit_test(geometry, item.center_x, height - 1.0) is item


@pytest.mark.parametrize("root", range(12))
def test_black_hit_priority_matches_visible_overlays_and_short_key_height(root: int) -> None:
    geometry = piano_geometry(root, width=840.0, height=140.0)
    whites = [item for item in geometry if item.key.black is False]

    for item in geometry:
        if not item.key.black:
            continue
        for x in (item.x + item.width * 0.1, item.x + item.width * 0.9):
            underlying = next(white for white in whites if white.contains(x, item.height))
            assert piano_hit_test(geometry, x, item.height / 2.0) is item
            assert piano_hit_test(geometry, x, item.height) is underlying
            assert piano_hit_test(geometry, x, 139.0) is underlying


def test_each_octave_has_the_physical_two_and_three_black_key_groups() -> None:
    geometry = piano_geometry(7, width=840.0, height=140.0)

    for octave in range(3):
        keys = geometry[octave * 12 : (octave + 1) * 12]
        assert [item.semitone_index % 12 for item in keys if item.key.black] == [1, 3, 6, 8, 10]
        assert [item.semitone_index % 12 for item in keys if not item.key.black] == [
            0,
            2,
            4,
            5,
            7,
            9,
            11,
        ]


@pytest.mark.parametrize("point", [(-0.01, 20.0), (840.0, 20.0), (1.0, -0.01), (1.0, 140.0)])
def test_piano_rejects_points_outside_its_half_open_bounds(point: tuple[float, float]) -> None:
    geometry = piano_geometry(0, width=840.0, height=140.0)
    assert piano_hit_test(geometry, *point) is None


def test_unknown_source_exposes_relative_targets_without_claiming_an_absolute_key() -> None:
    geometry = piano_geometry(None, width=840.0, height=140.0)

    assert [item.key.value for item in geometry] == list(range(-12, 24))
    assert len({item.key.label for item in geometry}) == 36
    assert not any(item.is_original for item in geometry)
    assert all(int(item.key.label.split()[0]) == item.key.value for item in geometry)
    assert all("st" in item.key.label for item in geometry)
    assert not any("original" in item.key.label for item in geometry)
    zero = geometry[12]
    assert zero.key.value == 0
    assert zero.key.label == "0 st"
    assert piano_hit_test(geometry, zero.center_x, 139.0) is zero


@pytest.mark.parametrize("root", [-1, 12])
def test_invalid_source_root_is_rejected(root: int) -> None:
    with pytest.raises(ValueError, match="Source root"):
        piano_geometry(root, width=840.0, height=140.0)


@pytest.mark.parametrize("dimension", [0.0, -1.0, math.inf, math.nan])
def test_invalid_dimensions_do_not_produce_unbounded_or_degenerate_keys(dimension: float) -> None:
    with pytest.raises(ValueError, match="dimensions"):
        piano_geometry(0, width=dimension, height=140.0)
    with pytest.raises(ValueError, match="dimensions"):
        piano_geometry(0, width=840.0, height=dimension)
