import math
import random

import pytest

from cotwmod.ballistics import (Camera, Projectile, ShotConditions, impact_offset_px,
                                relative_wind_dir, solve, solve_hold, wind_components,
                                zero_angle)
from cotwmod.ballistics.calibration import fit

ARROW = Projectile(muzzle_velocity=80.0, drag_k=0.0008, sight_height=0.08)


@pytest.mark.parametrize("zero", [10, 20, 30, 40, 50, 60])
def test_zero_distance_has_no_drop(zero):
    s = solve(ARROW, zero, ShotConditions(distance=zero))
    assert abs(s.drop_m) < 1e-5
    assert abs(s.drift_m) < 1e-9


def test_vacuum_matches_closed_form():
    p = Projectile(muzzle_velocity=60.0, drag_k=0.0, sight_height=0.0)
    s = solve(p, 0.6, ShotConditions(distance=50.0))  # tiny zero ~ bore along LOS
    # closed form with bore angle ~ 0: drop = g t^2 / 2, t = d / v
    t = 50.0 / 60.0
    expected = 0.5 * 9.80665 * t * t
    a = zero_angle(p, 0.6)
    # account for the tiny bore angle
    expected -= math.tan(a) * 50.0
    assert s.drop_m == pytest.approx(expected, rel=2e-3)


def test_drop_increases_beyond_zero_and_negative_inside():
    near = solve(ARROW, 40, ShotConditions(distance=25))
    far = solve(ARROW, 40, ShotConditions(distance=60))
    assert near.drop_m < 0      # arrow is above LOS between near and far zero
    assert far.drop_m > 0
    assert solve(ARROW, 40, ShotConditions(distance=80)).drop_m > far.drop_m


def test_heavier_drag_more_drop():
    heavy = Projectile(muzzle_velocity=80.0, drag_k=0.003, sight_height=0.08)
    assert solve(heavy, 20, ShotConditions(60)).drop_m > solve(ARROW, 20, ShotConditions(60)).drop_m


def test_crosswind_drift_direction():
    # wind from the left (270) blows left -> right => impact drifts right
    hw, cw = wind_components(5.0, 270)
    assert cw == pytest.approx(5.0)
    s = solve(ARROW, 20, ShotConditions(50, headwind=hw, crosswind=cw))
    assert s.drift_m > 0
    hw, cw = wind_components(5.0, 90)
    assert solve(ARROW, 20, ShotConditions(50, headwind=hw, crosswind=cw)).drift_m < 0


def test_headwind_increases_drop():
    calm = solve(ARROW, 20, ShotConditions(60))
    head = solve(ARROW, 20, ShotConditions(60, headwind=6))
    tail = solve(ARROW, 20, ShotConditions(60, headwind=-6))
    assert head.drop_m > calm.drop_m > tail.drop_m


def test_incline_shoots_high():
    flat = solve(ARROW, 20, ShotConditions(60, incline_deg=0))
    up = solve(ARROW, 20, ShotConditions(60, incline_deg=30))
    down = solve(ARROW, 20, ShotConditions(60, incline_deg=-30))
    assert up.drop_m < flat.drop_m and down.drop_m < flat.drop_m


def test_solve_hold_converges():
    s = solve_hold(ARROW, 20, ShotConditions(70, incline_deg=25))
    assert math.isfinite(s.drop_mrad)


def test_relative_wind():
    assert relative_wind_dir(90, 90) == 0
    assert relative_wind_dir(10, 350) == 20


def test_projection_center_and_sign():
    cam = Camera(1920, 1080, fov_deg=90, fov_is_horizontal=True)
    assert cam.focal_px() == pytest.approx(960.0)
    dx, dy = impact_offset_px(cam, drop_mrad=10, drift_mrad=5)
    assert dy > 0 and dx > 0          # below and right on screen
    cam2 = Camera(1920, 1080, fov_deg=90, magnification=4)
    assert cam2.px_per_mrad() == pytest.approx(4 * cam.px_per_mrad())


def test_calibration_recovers_parameters():
    truth = Projectile(muzzle_velocity=73.0, drag_k=0.0015, wind_factor=0.7, sight_height=0.08)
    rnd = random.Random(1)
    shots = []
    for d in (10, 20, 30, 40, 50, 60, 70):
        for (hw, cw) in ((0, 0), (0, 4), (3, -2)):
            s = solve(truth, 30, ShotConditions(d, headwind=hw, crosswind=cw))
            shots.append({"distance": d, "zero_distance": 30, "headwind": hw, "crosswind": cw,
                          "drop_mrad": s.drop_mrad + rnd.gauss(0, 0.02),
                          "drift_mrad": s.drift_mrad + rnd.gauss(0, 0.02)})
    guess = Projectile(muzzle_velocity=90.0, drag_k=0.0005, wind_factor=1.0, sight_height=0.08)
    fitted, rep = fit(guess, shots)
    assert fitted.muzzle_velocity == pytest.approx(73.0, rel=0.02)
    assert fitted.drag_k == pytest.approx(0.0015, rel=0.15)
    assert fitted.wind_factor == pytest.approx(0.7, rel=0.05)
    assert rep["rms_mrad"] < 0.05
