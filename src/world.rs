//! Host RF environment: positions, RSSI, looping virtual advertisers.

use crate::cmd::ChipCmd;
use std::collections::HashMap;
use std::f64::consts::PI;

const HEAR_M: f64 = 25.0;
const ADV_MS: u32 = 250;
pub const CROWD_PERIOD_MS: u32 = 20_000;

#[derive(Clone, Copy)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub present: bool,
}

pub struct RadioEvent {
    pub listener: usize,
    pub cmd: ChipCmd,
}

pub struct World {
    name: &'static str,
    looping: bool,
    virtuals: Vec<Virtual>,
    last_link: HashMap<(usize, [u8; 6]), bool>,
}

struct Virtual {
    mac: [u8; 6],
    kind: PathKind,
}

enum PathKind {
    Orbit {
        cx: f64,
        cy: f64,
        r: f64,
        phase: f64,
    },
    Pace {
        x0: f64,
        y: f64,
        span: f64,
        phase: f64,
    },
}

impl World {
    pub fn crowd(looping: bool) -> Self {
        let mut virtuals = Vec::new();
        for i in 0..6u8 {
            let mut mac = [0xC0, 0x12, 0xD0, 0, 0, i + 1];
            mac[3] = i.wrapping_mul(17);
            mac[4] = 0xA0 ^ i;
            let kind = if i % 2 == 0 {
                PathKind::Orbit {
                    cx: 4.0,
                    cy: 0.0,
                    r: 3.0 + f64::from(i) * 0.4,
                    phase: f64::from(i) * 0.9,
                }
            } else {
                PathKind::Pace {
                    x0: -2.0,
                    y: f64::from(i) - 2.0,
                    span: 10.0,
                    phase: f64::from(i) * 0.7,
                }
            };
            virtuals.push(Virtual { mac, kind });
        }
        Self {
            name: "crowd",
            looping,
            virtuals,
            last_link: HashMap::new(),
        }
    }

    pub fn still(looping: bool) -> Self {
        let virtuals = (0..5u8)
            .map(|i| Virtual {
                mac: [0xC0, 0x12, 0xD0, 0x11, i, 0x10],
                kind: PathKind::Pace {
                    x0: f64::from(i) * 2.0,
                    y: 1.0,
                    span: 0.0,
                    phase: 0.0,
                },
            })
            .collect();
        Self {
            name: "still",
            looping,
            virtuals,
            last_link: HashMap::new(),
        }
    }

    pub fn empty(looping: bool) -> Self {
        Self {
            name: "mesh",
            looping,
            virtuals: Vec::new(),
            last_link: HashMap::new(),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn period_ms(&self) -> u32 {
        CROWD_PERIOD_MS
    }

    pub fn open(name: &str, looping: bool) -> Result<Self, String> {
        match name {
            "crowd" => Ok(Self::crowd(looping)),
            "still" => Ok(Self::still(looping)),
            "mesh" => Ok(Self::empty(looping)),
            other => Err(format!("unknown world {other:?} — crowd, still, mesh")),
        }
    }

    pub fn list() -> &'static [(&'static str, &'static str)] {
        &[
            ("crowd", "six looping walkers around the chips"),
            ("still", "five static beacons"),
            ("mesh", "chips only — each node hears the others"),
        ]
    }

    pub fn radio(&mut self, now_ms: u32, chips: &[(usize, [u8; 6], f64, f64)]) -> Vec<RadioEvent> {
        let t = if self.looping {
            now_ms % self.period_ms()
        } else {
            now_ms
        };
        let mut sources: Vec<([u8; 6], Pose)> = Vec::new();
        for virt in &self.virtuals {
            sources.push((virt.mac, virt.pose(t, self.looping, self.period_ms())));
        }
        for (_, mac, x, y) in chips {
            sources.push((
                *mac,
                Pose {
                    x: *x,
                    y: *y,
                    present: true,
                },
            ));
        }

        let mut events = Vec::new();
        for &(li, listen_mac, lx, ly) in chips {
            for &(mac, pose) in &sources {
                if mac == listen_mac {
                    continue;
                }
                let rssi = if pose.present {
                    hear((lx, ly), (pose.x, pose.y))
                } else {
                    None
                };
                let key = (li, mac);
                let was = self.last_link.get(&key).copied().unwrap_or(false);
                let now = rssi.is_some();
                if let Some(rssi) = rssi {
                    let beat = ADV_MS + u32::from(mac[5]) * 17;
                    if !was || now_ms.is_multiple_of(beat) {
                        events.push(RadioEvent {
                            listener: li,
                            cmd: ChipCmd::Scan { addr: mac, rssi },
                        });
                    }
                } else if was {
                    events.push(RadioEvent {
                        listener: li,
                        cmd: ChipCmd::Gone { addr: mac },
                    });
                }
                self.last_link.insert(key, now);
            }
        }
        events
    }
}

impl Virtual {
    fn pose(&self, t_ms: u32, looping: bool, period: u32) -> Pose {
        if !looping && t_ms >= period {
            return Pose {
                x: 0.0,
                y: 0.0,
                present: false,
            };
        }
        let frac = f64::from(t_ms % period) / f64::from(period);
        match self.kind {
            PathKind::Orbit { cx, cy, r, phase } => {
                let a = frac * 2.0 * PI + phase;
                Pose {
                    x: cx + r * a.cos(),
                    y: cy + r * a.sin(),
                    present: true,
                }
            }
            PathKind::Pace { x0, y, span, phase } => {
                let w = (frac * 2.0 + phase / (2.0 * PI)).fract();
                let trip = if w < 0.5 { w * 2.0 } else { 2.0 - w * 2.0 };
                Pose {
                    x: x0 + span * trip,
                    y,
                    present: true,
                }
            }
        }
    }
}

pub fn hear(from: (f64, f64), to: (f64, f64)) -> Option<i8> {
    let dx = from.0 - to.0;
    let dy = from.1 - to.1;
    let dist = dx.hypot(dy);
    if dist > HEAR_M {
        return None;
    }
    Some(rssi_at(dist.max(0.1)))
}

pub fn rssi_at(dist_m: f64) -> i8 {
    let db = -40.0 - 25.0 * dist_m.log10();
    db.round().clamp(-90.0, -20.0) as i8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closer_is_stronger() {
        assert!(rssi_at(1.0) > rssi_at(8.0));
        assert!(hear((0.0, 0.0), (1.0, 0.0)).is_some());
        assert!(hear((0.0, 0.0), (80.0, 0.0)).is_none());
    }

    #[test]
    fn looping_crowd_keeps_walkers() {
        let mut looping = World::crowd(true);
        let mut finite = World::crowd(false);
        let chips = [(0usize, [0x02, 0, 0, 0, 0, 1], 0.0, 0.0)];
        assert!(!looping.radio(0, &chips).is_empty());
        assert!(!finite.radio(0, &chips).is_empty());
        let beat = ADV_MS + 17;
        assert!(looping
            .radio(beat, &chips)
            .iter()
            .any(|e| matches!(e.cmd, ChipCmd::Scan { .. })));
        let wrapped = looping.radio(CROWD_PERIOD_MS + beat, &chips);
        assert!(!wrapped
            .iter()
            .any(|e| matches!(e.cmd, ChipCmd::Gone { .. })));
        let gone = finite.radio(CROWD_PERIOD_MS + beat, &chips);
        assert!(gone.iter().any(|e| matches!(e.cmd, ChipCmd::Gone { .. })));
    }

    #[test]
    fn two_chips_hear_each_other() {
        let mut world = World::empty(true);
        let a = [0x02, 0x62, 0x52, 0, 0, 1];
        let b = [0x02, 0x62, 0x52, 0, 0, 2];
        let chips = [(0usize, a, 0.0, 0.0), (1usize, b, 3.0, 0.0)];
        let ev = world.radio(0, &chips);
        let a_hears_b = ev
            .iter()
            .any(|e| e.listener == 0 && matches!(e.cmd, ChipCmd::Scan { addr, .. } if addr == b));
        let b_hears_a = ev
            .iter()
            .any(|e| e.listener == 1 && matches!(e.cmd, ChipCmd::Scan { addr, .. } if addr == a));
        assert!(a_hears_b && b_hears_a);
    }
}
