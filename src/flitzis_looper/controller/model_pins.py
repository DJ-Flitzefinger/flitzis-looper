"""Full public model identities shared by source and Portable setup recipes."""

# Stock HTDemucs checkpoint used to hydrate the locally installed ONNX graphs.
DEMUCS_ASSETS: dict[str, tuple[int, str, str]] = {
    "955717e8-8726e21a.th": (
        84141911,
        "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4",
        "https://dl.fbaipublicfiles.com/demucs/hybrid_transformer/955717e8-8726e21a.th",
    ),
}
