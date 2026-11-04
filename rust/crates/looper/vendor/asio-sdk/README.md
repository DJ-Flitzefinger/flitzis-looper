# ASIO interface source

These pristine interface headers come from Steinberg ASIO SDK 2.3.4, dated
2025-10-15. `provenance.json` records the exact official archive and retained bytes.
This GPLv3 project uses the SDK's GPL version 3 option. Both upstream license
copies and the original header notices are retained.

The application supplies its own small C++ host bridge. It does not use the SDK's
global host-driver list, install a driver or download SDK code during a build.
WASAPI users do not need an ASIO driver. Linux/macOS skip the Windows bridge.

ASIO is a trademark of Steinberg Media Technologies GmbH. No ASIO logo is used.
