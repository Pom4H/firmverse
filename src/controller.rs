//! Managed controller targets.
//!
//! A Firmverse controller is intentionally not a fake SoC. Firmware targets
//! execute machine code through CPU -> SoC -> Board; managed controllers run
//! their native program runtime behind a controller-specific I/O boundary.

use clap::ValueEnum;

// Browser builds expose the Saturn target metadata and `.fbdbin` inspector, but
// deliberately do not link the process-global native C runtime yet. Keep the
// native-only FFI imports inside the module from becoming browser build noise.
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
// The module's binary-layout test deliberately places an ASCII date directly
// after a NUL byte, exactly as `.fbdbin` stores consecutive ASCIIZ strings.
#[allow(clippy::octal_escapes)]
pub mod saturn;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum ControllerKind {
    #[default]
    #[value(name = "saturn-plc")]
    SaturnPlc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerRuntime {
    FbdV11,
}

impl ControllerRuntime {
    pub const fn id(self) -> &'static str {
        match self {
            Self::FbdV11 => "fbd-runtime-v11",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ControllerProfile {
    pub kind: ControllerKind,
    pub id: &'static str,
    pub name: &'static str,
    pub manufacturer: &'static str,
    pub runtime: ControllerRuntime,
    pub artifact: &'static str,
    pub description: &'static str,
    pub native_execution: bool,
    pub browser_execution: bool,
}

pub const SATURN_PLC: ControllerProfile = ControllerProfile {
    kind: ControllerKind::SaturnPlc,
    id: "saturn-plc",
    name: "Saturn-PLC",
    manufacturer: "MNPP Saturn",
    runtime: ControllerRuntime::FbdV11,
    artifact: ".fbdbin",
    description: "Saturn-PLC managed target executing the upstream FBD runtime v11",
    native_execution: cfg!(firmverse_saturn_native),
    browser_execution: false,
};

pub const PROFILES: &[ControllerProfile] = &[SATURN_PLC];

pub const fn profile(kind: ControllerKind) -> &'static ControllerProfile {
    match kind {
        ControllerKind::SaturnPlc => &SATURN_PLC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturn_is_a_controller_not_a_soc() {
        let profile = profile(ControllerKind::SaturnPlc);
        assert_eq!(profile.runtime, ControllerRuntime::FbdV11);
        assert_eq!(profile.artifact, ".fbdbin");
        assert_eq!(profile.native_execution, saturn::native_runtime_available());
    }
}
