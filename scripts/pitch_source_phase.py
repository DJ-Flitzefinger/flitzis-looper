"""Immutable integrated source phase for the isolated linear-chirplet experiment."""

from dataclasses import dataclass
from math import isfinite
from typing import TYPE_CHECKING

import numpy as np
from pitch_mode_carriers import FS

if TYPE_CHECKING:
    from numpy.typing import ArrayLike, NDArray


@dataclass(frozen=True, slots=True)
class LinearSourcePhase:
    """One affine frequency law in fixed source-frame coordinates.

    The epoch declares a phase gauge, rather than a carrier birth or an event
    boundary. Complex stereo coefficients supply the constant phase at that
    epoch. Frequency is measured in Hz and slope in Hz per second.
    """

    epoch_frame: float
    frequency_hz: float
    slope_hz_per_second: float

    def __post_init__(self) -> None:
        """Validate and retain ordinary finite scalar parameters."""
        for name in ("epoch_frame", "frequency_hz", "slope_hz_per_second"):
            value = getattr(self, name)
            if not np.isscalar(value) or np.iscomplexobj(value):
                message = "Linear source phase parameters must be finite real scalars"
                raise ValueError(message)
            numeric = float(value)
            if not isfinite(numeric):
                message = "Linear source phase parameters must be finite real scalars"
                raise ValueError(message)
            object.__setattr__(self, name, numeric)

    def cycles(self, positions: ArrayLike) -> np.float64 | NDArray[np.float64]:
        """Return integrated cycles at scalar or array source-frame positions."""
        tau = (np.asarray(positions, dtype=np.float64) - self.epoch_frame) / FS
        return tau * (self.frequency_hz + 0.5 * self.slope_hz_per_second * tau)

    def frequency_at(self, positions: ArrayLike) -> np.float64 | NDArray[np.float64]:
        """Return instantaneous Hz at scalar or array source-frame positions."""
        tau = (np.asarray(positions, dtype=np.float64) - self.epoch_frame) / FS
        return self.frequency_hz + self.slope_hz_per_second * tau

    def phase_difference(
        self, positions: ArrayLike, anchor_frame: float
    ) -> np.float64 | NDArray[np.float64]:
        """Return radians since an anchor, factoring the integrated difference.

        Factoring avoids subtracting two large integrated phases for a short
        displacement late in a source. The anchor does not change the epoch.
        """
        delta_time = (np.asarray(positions, dtype=np.float64) - anchor_frame) / FS
        anchor_frequency = self.frequency_at(anchor_frame)
        return (
            2
            * np.pi
            * delta_time
            * (anchor_frequency + 0.5 * self.slope_hz_per_second * delta_time)
        )
