"""Child-only numerical policy; importing this module changes no runtime state."""


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
