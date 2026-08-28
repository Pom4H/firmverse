//! SoC and CPU-backend registry.
//!
//! Firmverse keeps CPU execution separate from chip/peripheral models. Cortex-M
//! SoCs use jjkt/zmu; non-Cortex-M chips can plug in a different backend
//! without changing Board or World.

pub mod phy6252;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocKind {
    Phy6252,
    GenericCortexM4,
    Ch592f,
}

impl SocKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Phy6252 => "phy6252",
            Self::GenericCortexM4 => "cortex-m4-generic",
            Self::Ch592f => "ch592f",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CortexMProfile {
    M0,
    M0Plus,
    M3,
    M4,
    M4F,
    M7,
}

impl CortexMProfile {
    pub const fn id(self) -> &'static str {
        match self {
            Self::M0 => "cortex-m0",
            Self::M0Plus => "cortex-m0+",
            Self::M3 => "cortex-m3",
            Self::M4 => "cortex-m4",
            Self::M4F => "cortex-m4f",
            Self::M7 => "cortex-m7",
        }
    }
}

/// Profiles supported by the pinned jjkt/zmu Cortex-M engine.
pub const ZMU_CORTEX_M_PROFILES: &[CortexMProfile] = &[
    CortexMProfile::M0,
    CortexMProfile::M0Plus,
    CortexMProfile::M3,
    CortexMProfile::M4,
    CortexMProfile::M4F,
    CortexMProfile::M7,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuBackend {
    Zmu(CortexMProfile),
    PlannedRiscV(&'static str),
}

impl CpuBackend {
    pub fn label(self) -> String {
        match self {
            Self::Zmu(profile) => format!("zmu/{}", profile.id()),
            Self::PlannedRiscV(core) => format!("riscv/{core} [planned]"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SocProfile {
    pub kind: SocKind,
    pub id: &'static str,
    pub name: &'static str,
    pub cpu: CpuBackend,
    pub implemented: bool,
    pub description: &'static str,
}

pub const PHY6252: SocProfile = SocProfile {
    kind: SocKind::Phy6252,
    id: "phy6252",
    name: "PHY6252",
    cpu: CpuBackend::Zmu(CortexMProfile::M0),
    implemented: cfg!(feature = "armv6m"),
    description: "Cortex-M0 BLE SoC; execution requires the Firmverse armv6m build",
};

pub const GENERIC_CORTEX_M4: SocProfile = SocProfile {
    kind: SocKind::GenericCortexM4,
    id: "cortex-m4-generic",
    name: "Generic Cortex-M4",
    cpu: CpuBackend::Zmu(CortexMProfile::M4),
    implemented: cfg!(feature = "armv7em"),
    description: "strict linear-memory Cortex-M4 target for portable firmware and resource probes",
};

pub const CH592F: SocProfile = SocProfile {
    kind: SocKind::Ch592f,
    id: "ch592f",
    name: "WCH CH592F",
    cpu: CpuBackend::PlannedRiscV("qingke-v4c"),
    implemented: false,
    description: "BLE RISC-V SoC; model/backend boundary reserved, execution not implemented yet",
};

pub const PROFILES: &[SocProfile] = &[PHY6252, GENERIC_CORTEX_M4, CH592F];

pub const fn profile(kind: SocKind) -> &'static SocProfile {
    match kind {
        SocKind::Phy6252 => &PHY6252,
        SocKind::GenericCortexM4 => &GENERIC_CORTEX_M4,
        SocKind::Ch592f => &CH592F,
    }
}

pub fn require_implemented(kind: SocKind) -> Result<&'static SocProfile, String> {
    let soc = profile(kind);
    if !soc.implemented {
        return Err(format!(
            "SoC {} uses {} and is not available in this Firmverse build",
            soc.id,
            soc.cpu.label()
        ));
    }
    Ok(soc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "armv6m")]
    #[test]
    fn armv6m_build_exposes_phy6252_only() {
        assert_eq!(PHY6252.cpu, CpuBackend::Zmu(CortexMProfile::M0));
        assert!(require_implemented(SocKind::Phy6252).is_ok());
        assert!(require_implemented(SocKind::GenericCortexM4).is_err());
    }

    #[cfg(feature = "armv7em")]
    #[test]
    fn armv7em_build_exposes_generic_cortex_m4_only() {
        assert!(require_implemented(SocKind::GenericCortexM4).is_ok());
        assert!(require_implemented(SocKind::Phy6252).is_err());
    }

    #[test]
    fn zmu_registry_keeps_supported_cortex_m_profiles_visible() {
        assert!(ZMU_CORTEX_M_PROFILES.contains(&CortexMProfile::M3));
        assert!(ZMU_CORTEX_M_PROFILES.contains(&CortexMProfile::M4));
        assert!(ZMU_CORTEX_M_PROFILES.contains(&CortexMProfile::M4F));
    }

    #[test]
    fn ch592f_fails_closed_until_riscv_backend_exists() {
        let err = require_implemented(SocKind::Ch592f).unwrap_err();
        assert!(err.contains("not available"));
        assert!(err.contains("qingke-v4c"));
    }
}
