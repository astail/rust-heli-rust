//! ゲームモード (フリー / リング / 着陸 / ホバリング) の進行。画面への反映は呼び出し側でやる。

use crate::physics::{Heli, sign};
use crate::world::{self, LANDING_ORDER, World};
use glam::Vec3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Free,
    Rings,
    Landing,
    Hover,
}

impl Mode {
    pub fn from_key(s: &str) -> Mode {
        match s {
            "rings" => Mode::Rings,
            "landing" => Mode::Landing,
            "hover" => Mode::Hover,
            _ => Mode::Free,
        }
    }
}

pub const RING_R: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HoverState {
    Outside,
    Moving,
    Still,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// クリアした (best キー, 表示名, タイム)
    Finished {
        key: &'static str,
        label: &'static str,
        time: f32,
    },
    /// 着陸できた (残りあり)
    Landed {
        soft: bool,
        done: usize,
        total: usize,
    },
}

pub struct Mission {
    pub mode: Mode,
    pub t: f32,
    pub idx: usize,
    pub done: bool,
    pub hover_t: f32,
    pub hover: HoverState,
    last_pos: Vec3,
    pub rings: Vec<(Vec3, Vec3)>,
    pub hover_center: Vec3,
    /// ミッション表示 (HTML)
    pub msg: String,
}

pub fn fmt_time(t: f32) -> String {
    format!("{t:.2}s")
}

impl Mission {
    pub fn new() -> Self {
        let (hx, hz) = world::HOVER_ZONE;
        Mission {
            mode: Mode::Free,
            t: 0.0,
            idx: 0,
            done: false,
            hover_t: 0.0,
            hover: HoverState::Outside,
            last_pos: Vec3::ZERO,
            rings: world::ring_layout(),
            hover_center: Vec3::new(hx, world::terrain_h(hx, hz) + 10.0, hz),
            msg: String::new(),
        }
    }

    pub fn start(&mut self, mode: Mode, pos: Vec3) {
        self.mode = mode;
        self.t = 0.0;
        self.idx = 0;
        self.done = false;
        self.hover_t = 0.0;
        self.hover = HoverState::Outside;
        self.last_pos = pos;
    }

    /// 目標の上に出す矢印の位置
    pub fn marker(&self, w: &World) -> Option<Vec3> {
        if self.done {
            return None;
        }
        match self.mode {
            Mode::Free => None,
            Mode::Rings => Some(self.rings[self.idx].0 + Vec3::new(0.0, 11.0, 0.0)),
            Mode::Landing => {
                let s = w.solid(w.pads[LANDING_ORDER[self.idx]].solid);
                Some(Vec3::new(s.x, s.top + 9.0, s.z))
            }
            Mode::Hover => Some(self.hover_center + Vec3::new(0.0, 9.0, 0.0)),
        }
    }

    /// いま目指しているパッド (pads の添字)
    pub fn target_pad(&self) -> Option<usize> {
        (self.mode == Mode::Landing && !self.done)
            .then(|| LANDING_ORDER[self.idx.min(LANDING_ORDER.len() - 1)])
    }

    fn finish(&mut self, key: &'static str, label: &'static str) -> Option<Outcome> {
        self.done = true;
        Some(Outcome::Finished {
            key,
            label,
            time: self.t,
        })
    }

    pub fn touchdown(&mut self, solid: Option<u32>, impact: f32, w: &World) -> Option<Outcome> {
        if self.mode != Mode::Landing || self.done {
            return None;
        }
        let target = w.pads[LANDING_ORDER[self.idx]].solid;
        if solid != Some(target) {
            return None;
        }
        self.idx += 1;
        if self.idx >= LANDING_ORDER.len() {
            return self.finish("best_landing", "全着陸完了");
        }
        Some(Outcome::Landed {
            soft: impact < 2.5,
            done: self.idx,
            total: LANDING_ORDER.len(),
        })
    }

    pub fn update(&mut self, dt: f32, h: &Heli, w: &World) -> Option<Outcome> {
        if h.crashed || self.done {
            return None;
        }
        if self.mode != Mode::Free && (!h.grounded || self.t > 0.0) {
            self.t += dt;
        }
        match self.mode {
            Mode::Free => self.msg = "<b>フリーフライト</b> — 好きに飛んでみよう".into(),
            Mode::Rings => {
                let (rp, rn) = self.rings[self.idx];
                // 前回位置 → 今回位置の線分がリングの面を半径内で横切ったか
                let d0 = (self.last_pos - rp).dot(rn);
                let d1 = (h.pos - rp).dot(rn);
                let center = h.pos + Vec3::Y;
                if sign(d0) != sign(d1) && center.distance(rp) < RING_R {
                    self.idx += 1;
                    if self.idx >= self.rings.len() {
                        return self.finish("best_rings", "コース完走");
                    }
                }
                self.msg = format!(
                    "<b>リングコース</b> {}/{} — {}",
                    self.idx,
                    self.rings.len(),
                    fmt_time(self.t)
                );
            }
            Mode::Landing => {
                let s = w.solid(w.pads[LANDING_ORDER[self.idx]].solid);
                let dist = (h.pos.x - s.x).hypot(h.pos.z - s.z);
                self.msg = format!(
                    "<b>着陸チャレンジ</b> {}/{} — オレンジのパッドへ ({:.0} m) — {}",
                    self.idx,
                    LANDING_ORDER.len(),
                    dist,
                    fmt_time(self.t)
                );
            }
            Mode::Hover => {
                let z = self.hover_center;
                let inside = (h.pos.x - z.x).hypot(h.pos.z - z.z) < 6.0
                    && h.pos.y > z.y - 4.0
                    && h.pos.y < z.y + 4.0
                    && !h.grounded;
                let still = h.vel.length() < 3.0;
                if inside && still {
                    self.hover_t += dt;
                }
                self.hover = match (inside, still) {
                    (false, _) => HoverState::Outside,
                    (true, false) => HoverState::Moving,
                    (true, true) => HoverState::Still,
                };
                self.msg = format!(
                    "<b>ホバリング</b> 青い円柱の中で静止 (速度3m/s未満) — {:.1} / 20.0s — 経過 {}",
                    self.hover_t,
                    fmt_time(self.t)
                );
                if self.hover_t >= 20.0 {
                    return self.finish("best_hover", "ホバリング達成");
                }
            }
        }
        self.last_pos = h.pos;
        None
    }
}

impl Default for Mission {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flying_through_a_ring_advances() {
        let w = World::build();
        let mut m = Mission::new();
        let (rp, rn) = m.rings[0];
        let mut h = Heli {
            grounded: false,
            pos: rp - rn * 3.0 - Vec3::Y,
            ..Default::default()
        };
        m.start(Mode::Rings, h.pos);
        m.update(0.1, &h, &w);
        assert_eq!(m.idx, 0);
        h.pos = rp + rn * 3.0 - Vec3::Y;
        m.update(0.1, &h, &w);
        assert_eq!(m.idx, 1);
    }

    #[test]
    fn passing_beside_a_ring_does_not_count() {
        let w = World::build();
        let mut m = Mission::new();
        let (rp, rn) = m.rings[0];
        let side = rn.cross(Vec3::Y) * 12.0;
        let mut h = Heli {
            grounded: false,
            pos: rp + side - rn * 3.0,
            ..Default::default()
        };
        m.start(Mode::Rings, h.pos);
        h.pos = rp + side + rn * 3.0;
        m.update(0.1, &h, &w);
        assert_eq!(m.idx, 0);
    }

    #[test]
    fn landing_on_the_target_pad_counts() {
        let w = World::build();
        let mut m = Mission::new();
        m.start(Mode::Landing, Vec3::ZERO);
        assert_eq!(m.touchdown(Some(w.home), 1.0, &w), None);
        let first = w.pads[LANDING_ORDER[0]].solid;
        assert_eq!(
            m.touchdown(Some(first), 1.0, &w),
            Some(Outcome::Landed {
                soft: true,
                done: 1,
                total: 5
            })
        );
        for &p in &LANDING_ORDER[1..LANDING_ORDER.len() - 1] {
            m.touchdown(Some(w.pads[p].solid), 3.0, &w);
        }
        let last = w.pads[LANDING_ORDER[4]].solid;
        assert!(matches!(
            m.touchdown(Some(last), 3.0, &w),
            Some(Outcome::Finished {
                key: "best_landing",
                ..
            })
        ));
        assert!(m.done);
    }

    #[test]
    fn hovering_still_for_20s_finishes() {
        let w = World::build();
        let mut m = Mission::new();
        let h = Heli {
            grounded: false,
            pos: m.hover_center,
            ..Default::default()
        };
        m.start(Mode::Hover, h.pos);
        let mut out = None;
        for _ in 0..205 {
            out = out.or(m.update(0.1, &h, &w));
        }
        assert!(matches!(
            out,
            Some(Outcome::Finished {
                key: "best_hover",
                ..
            })
        ));
    }
}
