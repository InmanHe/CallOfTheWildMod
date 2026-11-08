"""
Angle <-> screen-pixel conversion for a perspective camera.

A point that is ``a`` radians off the camera axis appears
``f * tan(a)`` pixels from the screen centre, where ``f`` is the focal
length in pixels:  f = (H/2) / tan(vfov/2)   (or the horizontal equivalent).
Optical zoom (scopes, ADS) divides the FOV, so f scales with magnification.
"""
from __future__ import annotations

import math
from dataclasses import dataclass


@dataclass
class Camera:
    screen_w: int
    screen_h: int
    fov_deg: float                  # the game's FOV setting
    fov_is_horizontal: bool = True
    magnification: float = 1.0      # scope / ADS zoom factor
    px_per_mrad_override: float | None = None  # from calibration, wins if set

    def focal_px(self) -> float:
        if self.px_per_mrad_override:
            return self.px_per_mrad_override * 1000.0
        half = math.radians(self.fov_deg) / 2.0
        if self.fov_is_horizontal:
            f = (self.screen_w / 2.0) / math.tan(half)
        else:
            f = (self.screen_h / 2.0) / math.tan(half)
        return f * self.magnification

    def px_per_mrad(self) -> float:
        return self.focal_px() / 1000.0

    def offset_px(self, up_mrad: float, right_mrad: float) -> tuple[float, float]:
        """
        Screen offset (dx, dy) from the centre for a point that is
        ``up_mrad`` above and ``right_mrad`` right of the camera axis.
        Screen y grows downward.
        """
        f = self.focal_px()
        dx = f * math.tan(right_mrad / 1000.0)
        dy = -f * math.tan(up_mrad / 1000.0)
        return dx, dy


def impact_offset_px(cam: Camera, drop_mrad: float, drift_mrad: float) -> tuple[float, float]:
    """Pixel offset of the predicted impact point relative to the aim reference."""
    return cam.offset_px(-drop_mrad, drift_mrad)
