//! Named game fields bound to memory locations, plus live read/write helpers.

use super::pointer::{resolve, PtrChain};
use super::scan::VType;
use super::Process;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Loc {
    /// absolute address (valid until the game restarts)
    Absolute(u64),
    Pointer(PtrChain),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Binding {
    pub vtype: VType,
    pub loc: Loc,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub add: f64,
}

fn one() -> f64 {
    1.0
}

impl Binding {
    pub fn address(&self, p: &dyn Process) -> Option<u64> {
        match &self.loc {
            Loc::Absolute(a) => Some(*a),
            Loc::Pointer(c) => resolve(p, c),
        }
    }
    pub fn read_raw(&self, p: &dyn Process) -> Option<u64> {
        let a = self.address(p)?;
        let mut b = [0u8; 8];
        let n = self.vtype.size();
        p.read(a, &mut b[..n]).then(|| self.vtype.raw(&b[..n]))
    }
    pub fn read(&self, p: &dyn Process) -> Option<f64> {
        let a = self.address(p)?;
        let mut b = [0u8; 8];
        let n = self.vtype.size();
        if !p.read(a, &mut b[..n]) {
            return None;
        }
        let v = self.vtype.decode(&b[..n]);
        v.is_finite().then(|| v * self.scale + self.add)
    }
    pub fn write(&self, p: &dyn Process, v: f64) -> bool {
        let Some(a) = self.address(p) else { return false };
        let raw = if self.scale != 0.0 { (v - self.add) / self.scale } else { v };
        p.write(a, &self.vtype.encode(raw))
    }
    pub fn describe(&self) -> String {
        let loc = match &self.loc {
            Loc::Absolute(a) => format!("{a:#X}"),
            Loc::Pointer(c) => c.describe(),
        };
        format!("{} {}", self.vtype.name(), loc)
    }
}

/// Fields the mod knows how to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Field {
    WeaponId,
    AmmoId,
    AimDistance,
    CameraPitch,
    CameraYaw,
    WindFrom,
    WindSpeed,
    CameraFov,
    ViewDistance,
}

impl Field {
    pub const ALL: [Field; 9] = [
        Field::WeaponId,
        Field::AmmoId,
        Field::AimDistance,
        Field::CameraPitch,
        Field::CameraYaw,
        Field::WindFrom,
        Field::WindSpeed,
        Field::CameraFov,
        Field::ViewDistance,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Field::WeaponId => "weapon_id",
            Field::AmmoId => "ammo_id",
            Field::AimDistance => "aim_distance",
            Field::CameraPitch => "camera_pitch",
            Field::CameraYaw => "camera_yaw",
            Field::WindFrom => "wind_from",
            Field::WindSpeed => "wind_speed",
            Field::CameraFov => "camera_fov",
            Field::ViewDistance => "view_distance",
        }
    }
    /// Multiple addresses allowed (e.g. several LOD distances)
    pub fn multi(self) -> bool {
        matches!(self, Field::ViewDistance)
    }
}
