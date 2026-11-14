"""Acquire the CPU BS-RoFormer FFT library from pinned Intel redistributables."""

from typing import TYPE_CHECKING

from flitzis_looper.analysis.artifacts import file_digest
from flitzis_looper.analysis.native_provisioning import _Archive, _extract_members

if TYPE_CHECKING:
    from collections.abc import Mapping
    from pathlib import Path


_MKL = _Archive(
    "mkl-2026.0.0-py2.py3-none-win_amd64.whl",
    "https://files.pythonhosted.org/packages/7b/2f/"
    "8a77106ef648cc1f04aa27e8b6c50cd82de38d1767801ef29cd75b8026e8/"
    "mkl-2026.0.0-py2.py3-none-win_amd64.whl",
    180_805_356,
    "801c9cc748be81d8269bfec2495d02be38560fc7386439c18499ab3f32b06b3e",
)
_OPENMP = _Archive(
    "intel_openmp-2026.0.0-py2.py3-none-win_amd64.whl",
    "https://files.pythonhosted.org/packages/2f/48/"
    "32b1c24d3f1e4f47495d29bda38002384ae03a9ab709b575dda31842c0a4/"
    "intel_openmp-2026.0.0-py2.py3-none-win_amd64.whl",
    30_721_954,
    "d1f18c4e1be8426da117252a4fb8e5d72be44807b0251fcea23e2fa7b7d85981",
)
_MKL_LICENSE = _Archive(
    "onemkl_license-2026.0.0-py2.py3-none-win_amd64.whl",
    "https://files.pythonhosted.org/packages/2c/8e/"
    "fc57a715c626333e2ac8d2444d97ae2b1421fe33628261645668e6b9624b/"
    "onemkl_license-2026.0.0-py2.py3-none-win_amd64.whl",
    58_961,
    "8106d1177cc473b615e88d2051b4943aeb4c1408d79c8df22ab13ed8660760bb",
)
_MKL_DLLS = {
    "mkl_rt.3.dll": (
        27_890_848,
        "6a312eb1c4a92447e8d4c11d07420678bd07e0666c279fc621a3a6b03db527d8",
    ),
    "mkl_core.3.dll": (
        71_903_904,
        "54293955a106c62a4683d306851a78e6b3dd6ba64749f33639e61b9876956e18",
    ),
    "mkl_avx10.3.dll": (
        67_684_512,
        "c5122e4cb2aaa681039ca2cd39e8fe706f44ed697c943c85e87661e40c0fc609",
    ),
    "mkl_avx2.3.dll": (
        47_482_016,
        "b3d7212310180988f4d2974b212f6a2e978ea3e2377715a2de7d8a070d10d54f",
    ),
    "mkl_avx512.3.dll": (
        75_416_736,
        "d35544ffe706d75e93ce7f318c3587007ee3be9816a5a14215054721e6bf3d42",
    ),
    "mkl_def.3.dll": (
        37_053_600,
        "66cfb9935d2ce7811b49b56ed02ac454500e3d2f8e399724d6fc829a7fe91aba",
    ),
    "mkl_intel_thread.3.dll": (
        39_083_680,
        "eb6a026ae6124f51b8716a901f83441778cb4b63540ca8f4374d5e188299aa03",
    ),
    "mkl_mc3.3.dll": (
        47_141_024,
        "5fd371f139b1041ab3c76e8261ed5dcbde86cc351c7aaf589f9f0a27e16c800f",
    ),
}
_OPENMP_DLLS = {
    "libiomp5md.dll": (
        1_617_256,
        "2299b0460e8118e8187fd57a8d17df836c2a3d59f2639c3681582070da66b7be",
    )
}
BS_MKL_DLLS: Mapping[str, tuple[int, str]] = _MKL_DLLS | _OPENMP_DLLS
_OPENMP_LICENSE = {
    "LICENSE.txt": (25_224, "febfb69a5b058f55892dc7f341108a9ca29d50e9fe85723a57eebbb826436958"),
}
_OPENMP_NOTICES = {
    "third-party-programs.txt": (
        31_578,
        "f8ce918fe7311ce279e68380a2e233f8a42b1cd3dda8f4c48d6de97a0255c1d7",
    ),
}
_MKL_TERMS = {
    "LICENSE.txt": (4_105, "7721633d0ddff43fae25ebfd405f8166a0ce730cbcec44f2f3ad9d5eb8ac9a6f"),
}
_MKL_NOTICES = {
    "third-party-programs-IntelMPI.txt": (
        77_510,
        "82fa46762c37662a52ab9df58781813c1dc6c268ae2b6f9cf2d83183e706aa09",
    ),
    "third-party-programs-benchmarks.txt": (
        5_234,
        "1f1224af12a7d3e51150ce6f762e32472076a31f6e44db41d3b651a7fa20f2f5",
    ),
    "third-party-programs-level-zero.txt": (
        1_124,
        "cfe7d2ba4b4cd53271c7f91937e906256c0f14ea62ba33ffb556664b8d39aa59",
    ),
    "third-party-programs-oneMath.txt": (
        6_777,
        "2ba4e9858c253331691370f4e8e3899ca6a5baa20907f189f5970635870b860d",
    ),
    "third-party-programs-oneTBB.txt": (
        19_504,
        "d507da4fffae647ebc1bbb7e12cc5cca8087d36b0d78cc75caa96cae37688cb1",
    ),
    "third-party-programs-openmp.txt": (
        31_028,
        "8fdfbbdfed13bca3641da35182e7ee1bb92a99f302ce8b0deb981ac1f18adee2",
    ),
    "third-party-programs-safestring.txt": (
        2_448,
        "415000d1887939ffc6826e0ed3f099cebcab6aaeda2da0acb017110a456b3d42",
    ),
    "third-party-programs.txt": (
        65_770,
        "6fb57367c2107b8b3347e0e9db10d368c7b9ea898a08132b2f841b24b86f076f",
    ),
}


def provision_bs_mkl(stage: Path, cache: Path) -> None:
    """Verify and extract the CPU inference closure and its original license files."""
    runtime = stage / "mkl"
    runtime.mkdir()
    mkl = _MKL.acquire(cache, offline=False)
    openmp = _OPENMP.acquire(cache, offline=False)
    license_wheel = _MKL_LICENSE.acquire(cache, offline=False)
    _extract_members(mkl, "mkl-2026.0.0.data/data/Library/bin/", runtime, _MKL_DLLS)
    _extract_members(openmp, "intel_openmp-2026.0.0.data/data/Library/bin/", runtime, _OPENMP_DLLS)

    mkl_notices = stage / "mkl-notices/onemkl"
    openmp_notices = stage / "mkl-notices/openmp"
    mkl_notices.mkdir(parents=True)
    openmp_notices.mkdir()
    _extract_members(license_wheel, "onemkl_license-2026.0.0.dist-info/", mkl_notices, _MKL_TERMS)
    _extract_members(
        license_wheel,
        "onemkl_license-2026.0.0.data/data/share/doc/mkl/licensing/",
        mkl_notices,
        _MKL_NOTICES,
    )
    _extract_members(openmp, "intel_openmp-2026.0.0.dist-info/", openmp_notices, _OPENMP_LICENSE)
    _extract_members(
        openmp,
        "intel_openmp-2026.0.0.data/data/share/doc/compiler/licensing/openmp/",
        openmp_notices,
        _OPENMP_NOTICES,
    )


def verify_bs_mkl(root: Path) -> None:
    """Confirm the CPU FFT library and associated license files without loading DLLs."""
    notices = root / "mkl-notices"
    if notices.is_symlink() or notices.is_junction():
        msg = "BS-RoFormer CPU FFT notices must be ordinary files"
        raise ValueError(msg)
    _verify_files(root / "mkl", BS_MKL_DLLS)
    _verify_files(notices / "onemkl", _MKL_TERMS | _MKL_NOTICES)
    _verify_files(notices / "openmp", _OPENMP_LICENSE | _OPENMP_NOTICES)


def _verify_files(directory: Path, files: Mapping[str, tuple[int, str]]) -> None:
    if (
        directory.is_symlink()
        or directory.is_junction()
        or {path.name for path in directory.iterdir()} != files.keys()
    ):
        msg = "BS-RoFormer CPU FFT package has an unexpected layout"
        raise ValueError(msg)
    for name, (size, expected) in files.items():
        library = directory / name
        if (
            library.is_symlink()
            or library.is_junction()
            or library.stat().st_size != size
            or file_digest(library, max_bytes=size) != expected
        ):
            msg = f"BS-RoFormer CPU FFT package differs from its pin: {name}"
            raise ValueError(msg)
