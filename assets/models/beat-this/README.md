# BeatThis model and Windows runtime notices

Windows setup acquires the pinned dynamic FP32 export of BeatThis 1.1.0 `final0`
as a separate runtime asset. The file contains 82,104,250 bytes with SHA-256
`46bfb2c1a1f1e5c1d8786d0d9462f285971ff7fa8a592ac5f4ab609698bd44ca`.
Its source checkpoint SHA-256 is
`8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331`.
The opset 18 export retained inline weights and disabled the shared rotary cache.
The authors publish the weights under the [MIT license](BEAT-THIS-LICENSE.txt).
[Published graph and setup provenance](https://github.com/DJ-Flitzefinger/flitzis-looper/commit/cfd0c83eb6896dd30d6a2974e47b3aeb0f38c267).

Setup also acquires libsoxr 0.1.3 for Windows x64, SHA-256
`b2e3a0ff4806540250fa45e9522c3a3549663888397fce4708249369b25db37e`.
Its [corresponding source](windows-x64/soxr-source) includes the source archive,
license and vcpkg 0.1.3#8 port and patches used to build the shared
`x64-windows` triplet. [LGPL terms](windows-x64/soxr-source/COPYING.LGPL).

Setup verifies ONNX Runtime 1.24.4 CPU and CUDA DLLs from the official Windows
x64 wheels. CUDA 12.8.1 and cuDNN 9.10.2 DLLs come from NVIDIA's
redistributable archives. The [runtime notices](windows-x64) accompany these
libraries. Setup uses the Visual C++ redistributable files from the selected
MSVC toolchain. The installed NVIDIA driver supplies `nvcuda.dll`.
