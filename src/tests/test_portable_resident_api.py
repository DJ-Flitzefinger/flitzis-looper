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


class _CompatibleCopyEngine(_CompatibleEngine):
    def capture_pad_copy(self, binding: object, target_id: int, intent_json: str) -> object:
        raise NotImplementedError

    def prepare_pad_copy(self, captured: object) -> object:
        raise NotImplementedError

    def publish_pad_copy(self, ticket: object) -> None:
        raise NotImplementedError

    def reconcile_pad_copy(self, ticket: object) -> bool:
        raise NotImplementedError

    def finish_pad_copy(self, ticket: object) -> None:
        raise NotImplementedError

    def cancel_pad_copy(self, captured: object) -> bool:
        raise NotImplementedError


def test_complete_copy_interface_is_checked_without_an_engine() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    check(_CompatibleCopyEngine)


@pytest.mark.parametrize(
    "method",
    [
        "capture_pad_copy",
        "prepare_pad_copy",
        "publish_pad_copy",
        "reconcile_pad_copy",
        "finish_pad_copy",
        "cancel_pad_copy",
    ],
)
def test_incomplete_copy_interface_cannot_activate(method: str) -> None:
    engine_type = type("IncompatibleCopyEngine", (_CompatibleCopyEngine,), {method: None})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    with pytest.raises(AssertionError, match=method):
        check(engine_type)


def test_old_residency_interface_alone_cannot_activate_copy_controls() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    with pytest.raises(AssertionError, match="capture_pad_copy"):
        check(_CompatibleEngine)
