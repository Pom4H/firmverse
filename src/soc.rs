//! SoC and CPU-backend registry.
//!
//! Firmverse keeps CPU execution separate from chip/peripheral models. Cortex-M
//! SoCs use jjkt/zmu; non-Cortex-M chips can plug in a different backend
//! without changing Board or World.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocKind {
    Phy6252,
    Ch592f,
}

impl SocKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Phy6252 => "phy6252",
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

/// Profiles supported by the upstream jjkt/zmu Cortex-M engine.
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
    implemented: true,
    description: "Cortex-M0 BLE SoC; execution is provided by jjkt/zmu armv6m",
};

pub const CH592F: SocProfile = SocProfile {
    kind: SocKind::Ch592f,
    id: "ch592f",
    name: "WCH CH592F",
    cpu: CpuBackend::PlannedRiscV("qingke-v4c"),
    implemented: false,
    description: "BLE RISC-V SoC; model/backend boundary reserved, execution not implemented yet",
};

pub const PROFILES: &[SocProfile] = &[PHY6252, CH592F];

pub const fn profile(kind: SocKind) -> &'static SocProfile {
    match kind {
        SocKind::Phy6252 => &PHY6252,
        SocKind::Ch592f => &CH592F,
    }
}

pub fn require_implemented(kind: SocKind) -> Result<&'static SocProfile, String> {
    let soc = profile(kind);
    if !soc.implemented {
        return Err(format!(
            "SoC {} uses {} and is not implemented yet",
            soc.id,
            soc.cpu.label()
        ));
    }
    Ok(soc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phy6252_uses_zmu_cortex_m0() {
        assert_eq!(PHY6252.cpu, CpuBackend::Zmu(CortexMProfile::M0));
        assert!(require_implemented(SocKind::Phy6252).is_ok());
    }

    #[test]
    fn zmu_registry_keeps_other_cortex_m_profiles_visible() {
        assert!(ZMU_CORTEX_M_PROFILES.contains(&CortexMProfile::M3));
        assert!(ZMU_CORTEX_M_PROFILES.contains(&CortexMProfile::M4));
        assert!(ZMU_CORTEX_M_PROFILES.contains(&CortexMProfile::M4F));
    }

    #[test]
    fn ch592f_fails_closed_until_riscv_backend_exists() {
        let err = require_implemented(SocKind::Ch592f).unwrap_err();
        assert!(err.contains("not implemented"));
        assert!(err.contains("qingke-v4c"));
    }
}
