"""Child-only numerical policy; importing this module changes no runtime state."""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable


def child_environment(environment: dict[str, str]) -> dict[str, str]:
    """Copy limits before numerical imports, without mutating the parent."""
    child = dict(environment)
    for name in (
        "OMP_NUM_THREADS",
        "OMP_THREAD_LIMIT",
        "MKL_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "GOTO_NUM_THREADS",
        "NUMEXPR_NUM_THREADS",
        "NUMBA_NUM_THREADS",
        "VECLIB_MAXIMUM_THREADS",
        "BLIS_NUM_THREADS",
    ):
        child[name] = "1"
    child["OMP_DYNAMIC"] = "FALSE"
    child["MKL_DYNAMIC"] = "FALSE"
    return child


def configure_child(
    set_intra: Callable[[int], None],
    set_inter: Callable[[int], None],
    get_intra: Callable[[], int],
    get_inter: Callable[[], int],
) -> tuple[int, int]:
    """Set and verify effective pools before model construction or tensor work."""
    set_intra(1)
    set_inter(1)
    observed = get_intra(), get_inter()
    if observed != (1, 1):
        message = "Offline numerical thread policy was not applied"
        raise RuntimeError(message)
    return observed


def ffmpeg_command(arguments: list[str]) -> list[str]:
    """Bound decoder, encoder and filter pools without changing PCM options."""
    return [
        arguments[0],
        "-threads",
        "1",
        "-filter_threads",
        "1",
        "-filter_complex_threads",
        "1",
        *arguments[1:-1],
        "-threads",
        "1",
        arguments[-1],
    ]
