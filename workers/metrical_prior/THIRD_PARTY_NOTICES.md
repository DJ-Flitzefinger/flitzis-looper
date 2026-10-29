# Third-party provenance and licenses

TempoCNN by Hendrik Schreiber and contributors, source revision
`482c64ad2e01b61d39bc161b796e3c2a1afaf67e` (release0.0.8), supplies the retained
unmodified `upstream/feature.py`, `classifier.py` and `setup.py`.
Source: https://github.com/hendriks73/tempo-cnn/tree/482c64ad2e01b61d39bc161b796e3c2a1afaf67e

The two frontend helpers adapt that feature/normalization implementation.
The Node entry adapts LOOPER's old9BB experimental inference runner for isolated
process custody, bounded packed transport and resource observation. Its graph,
prediction and aggregation arithmetic remains unchanged. These worker adaptations
are provided under GNU Affero General Public License version3; see
`upstream/code-LICENSE` for the complete license text. This isolated notice does
not change the enclosing project's global licensing declaration.

The separately installed DeepTemp k16-3 TFJS model and retained metadata are from
https://essentia.upf.edu/models/tempo/tempocnn/ . The downloaded model-license text
is retained as `upstream/weights-LICENSE` (GNU AGPL version3), independently of the
code license. The graph/archive hashes in `numeric-profile.json` are local pins
of official HTTPS acquisition; no publisher signature or published SHA was seen.
Metadata's last-class286 description is retained verbatim; actual256 outputs
map by index+30 to30..285 in the author classifier.

Runtime packages retain their own licenses/notices in the physical installation.
The pinned requirement lock and installation inventory describe that closure;
this directory does not vendor those packages or model weights. Redistribution
must retain corresponding licenses, notices and applicable source obligations.
