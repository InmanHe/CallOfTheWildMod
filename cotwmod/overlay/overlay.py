"""
Transparent, click-through, always-on-top overlay that draws the predicted
point of impact over the game. Requires the game in Borderless / Windowed
mode. Nothing is injected into the game.
"""
from __future__ import annotations

import json
import logging
import sys

from ..config import CONFIG_DIR
from ..game.aim import AimEngine, AimResult

log = logging.getLogger(__name__)


def _make_click_through(hwnd: int) -> None:
    if not sys.platform.startswith("win"):
        return
    import ctypes
    GWL_EXSTYLE = -20
    WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE = 0x80000, 0x20, 0x80, 0x08000000
    user32 = ctypes.windll.user32
    style = user32.GetWindowLongW(hwnd, GWL_EXSTYLE)
    user32.SetWindowLongW(hwnd, GWL_EXSTYLE,
                          style | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)


class HotkeyListener:
    """Global hotkeys via pynput. Optional modifier must be held."""

    def __init__(self, engine: AimEngine, hotkeys: dict, on_record):
        from pynput import keyboard
        self.keyboard = keyboard
        self.engine = engine
        self.on_record = on_record
        self.modifier = hotkeys.get("hotkey_modifier")
        self.mod_down = False
        self.map = {}
        for action, key in hotkeys.items():
            if action.startswith("_") or action == "hotkey_modifier" or not key:
                continue
            self.map[self._norm(key)] = action
        self.listener = keyboard.Listener(on_press=self._press, on_release=self._release)

    def _norm(self, key) -> str:
        if isinstance(key, str):
            return key.lower()
        if isinstance(key, self.keyboard.Key):
            return key.name
        if getattr(key, "char", None):
            return key.char.lower()
        return str(key)

    def _press(self, key):
        k = self._norm(key)
        if self.modifier and k == self.modifier:
            self.mod_down = True
            return
        if self.modifier and not self.mod_down:
            return
        action = self.map.get(k)
        if not action:
            return
        if action == "calib_record":
            self.on_record()
        else:
            self.engine.action(action)

    def _release(self, key):
        if self.modifier and self._norm(key) == self.modifier:
            self.mod_down = False

    def start(self):
        self.listener.start()


def run_overlay(engine: AimEngine, settings: dict) -> int:
    from PyQt6.QtCore import QPointF, QRectF, Qt, QTimer
    from PyQt6.QtGui import QColor, QFont, QPainter, QPen
    from PyQt6.QtWidgets import QApplication, QWidget

    app = QApplication.instance() or QApplication(sys.argv)
    ov = settings["overlay"]
    color = QColor(ov.get("marker_color", "#33FF66"))
    radius = float(ov.get("marker_radius_px", 6))
    shots_file = CONFIG_DIR / "calibration_shots.json"

    def record_shot():
        shot = engine.calibration_shot()
        shots = json.loads(shots_file.read_text("utf-8")) if shots_file.exists() else []
        shots.append(shot)
        shots_file.write_text(json.dumps(shots, indent=2), "utf-8")
        log.info("recorded calibration shot #%d: %s", len(shots), shot)

    class Overlay(QWidget):
        def __init__(self):
            super().__init__()
            self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                                | Qt.WindowType.Tool | Qt.WindowType.WindowTransparentForInput)
            self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
            self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
            sc = settings["screen"]
            self.setGeometry(0, 0, int(sc["width"]), int(sc["height"]))
            self.result: AimResult | None = None
            self.error = ""
            self.timer = QTimer(self)
            self.timer.timeout.connect(self.tick)
            self.timer.start(int(1000 / max(5, settings["input"].get("poll_hz", 30))))

        def showEvent(self, e):
            super().showEvent(e)
            _make_click_through(int(self.winId()))

        def tick(self):
            try:
                self.result = engine.compute()
                self.error = ""
            except Exception as e:  # keep the overlay alive, show the error
                self.error = str(e)
            self.update()

        def paintEvent(self, _):
            st = engine.state
            if not st.visible:
                return
            p = QPainter(self)
            p.setRenderHint(QPainter.RenderHint.Antialiasing)
            cx, cy = self.width() / 2.0, self.height() / 2.0
            r = self.result

            if ov.get("show_zero_reference", True):
                p.setPen(QPen(QColor(255, 255, 255, 140), 1))
                p.drawLine(QPointF(cx - 4, cy), QPointF(cx + 4, cy))
                p.drawLine(QPointF(cx, cy - 4), QPointF(cx, cy + 4))

            if r:
                last_label_y = None
                p.setFont(QFont("Consolas", 8))
                for d, lx, ly in r.ladder:
                    p.setPen(QPen(QColor(255, 255, 255, 110), 1))
                    p.drawLine(QPointF(cx + lx - 6, cy + ly), QPointF(cx + lx + 6, cy + ly))
                    if last_label_y is None or abs(ly - last_label_y) >= 11:   # avoid overlapping labels
                        p.drawText(QPointF(cx + lx + 9, cy + ly + 4), f"{d}")
                        last_label_y = ly
                mx, my = cx + r.dx, cy + r.dy
                p.setPen(QPen(color, 2))
                p.drawEllipse(QPointF(mx, my), radius, radius)
                p.drawLine(QPointF(mx - radius * 2, my), QPointF(mx - radius, my))
                p.drawLine(QPointF(mx + radius, my), QPointF(mx + radius * 2, my))
                p.drawLine(QPointF(mx, my - radius * 2), QPointF(mx, my - radius))
                p.drawLine(QPointF(mx, my + radius), QPointF(mx, my + radius * 2))
                p.drawPoint(QPointF(mx, my))

            if st.calib_mode:
                mx, my = cx + st.calib_marker[0], cy + st.calib_marker[1]
                p.setPen(QPen(QColor("#FF3366"), 2))
                p.drawRect(QRectF(mx - 5, my - 5, 10, 10))

            if ov.get("show_info_panel", True):
                p.setFont(QFont("Consolas", 10))
                lines = []
                if r:
                    src = r.sources
                    tag = lambda k: "●" if src.get(k) == "live" else "○"
                    lines = [
                        f"{r.weapon_name} | {r.ammo_name}{'' if r.calibrated else '  [未校準]'}",
                        f"歸零 {r.zero:.0f} m   放大 {r.magnification:g}x",
                        f"{tag('aim_distance_m')} 距離 {r.distance:5.1f} m   {tag('incline')} 仰角 {r.incline_deg:+.1f}°",
                        f"{tag('wind_speed_ms')} 風 {r.wind_speed:.1f} m/s  {tag('wind_dir')} 來自 {r.wind_rel_deg:.0f}°",
                        f"抬高 {r.drop_mrad:+.1f} mrad  偏移 {r.drift_mrad:+.1f} mrad  飛行 {r.tof:.2f}s",
                    ]
                if st.calib_mode:
                    lines.append(f"[校準模式] 方向鍵移動紅框到箭落點, Insert 記錄 ({st.calib_marker[0]:+.0f},{st.calib_marker[1]:+.0f}px)")
                if self.error:
                    lines.append(f"錯誤: {self.error}")
                y = 30
                for ln in lines:
                    p.setPen(QColor(0, 0, 0, 200))
                    p.drawText(QPointF(21, y + 1), ln)
                    p.setPen(QColor(230, 255, 230))
                    p.drawText(QPointF(20, y), ln)
                    y += 16
            p.end()

    w = Overlay()
    w.show()
    try:
        HotkeyListener(engine, settings["hotkeys"], record_shot).start()
    except Exception as e:
        log.warning("hotkeys unavailable (%s) - install pynput", e)
    return app.exec()
