import pytest
import flitzis_looper_rs


def test_sum_as_string():
    assert flitzis_looper_rs.sum_as_string(1, 1) == "2"
