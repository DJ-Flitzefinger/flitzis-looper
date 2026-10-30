"""Full public model identities shared by source and Portable setup recipes."""

# Existing verified bytes, matching the locked Demucs remote/files.txt identity.
# This receipt is authored source; no personal workspace or previous cache is needed.
DEMUCS_ASSETS: dict[str, tuple[int, str, str]] = {
    "955717e8-8726e21a.th": (
        84141911,
        "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4",
        "https://dl.fbaipublicfiles.com/demucs/hybrid_transformer/955717e8-8726e21a.th",
    ),
}
