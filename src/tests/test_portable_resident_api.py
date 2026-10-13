"""Reject stale Native interfaces before a portable runtime can activate."""

import runpy
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]


class _CompatibleEngine:
    RESIDENT_PCM_ADMISSION_VERSION = 1

    def __init__(self) -> None:
        pytest.fail("Preflight must not construct an engine or open a device")

    def set_resident_pcm_budget_mib(self, value: int) -> None:
        pass

    def get_resident_pcm_budget_status(self) -> dict[str, int]:
        return {}

    def request_committed_stem_residency(
        self, ticket: object, *, interest: str = "explicit"
    ) -> None:
        pass


def test_compatible_class_is_checked_without_an_engine() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_resident_api"]
    check(_CompatibleEngine)


@pytest.mark.parametrize(
    ("attribute", "value", "message"),
    [
        ("RESIDENT_PCM_ADMISSION_VERSION", 0, "protocol missing"),
        ("set_resident_pcm_budget_mib", None, "set_resident_pcm_budget_mib"),
        ("get_resident_pcm_budget_status", None, "get_resident_pcm_budget_status"),
        ("request_committed_stem_residency", lambda self, ticket: None, "preload interest"),
    ],
)
def test_incompatible_native_is_rejected_before_activation(
    attribute: str,
    value: object,
    message: str,
) -> None:
    engine_type = type("IncompatibleEngine", (_CompatibleEngine,), {attribute: value})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_resident_api"]
    with pytest.raises(AssertionError, match=message):
        check(engine_type)
