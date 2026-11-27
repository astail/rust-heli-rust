//! 地形・建物・ヘリパッド・木の配置。乱数の種と消費順を元の JS と同じにしてあるので、同じ並びの街になる。

use glam::Vec3;

pub const WORLD: f32 = 1600.0;

pub fn terrain_h(x: f32, z: f32) -> f32 {
    let d = x.hypot(z);
    let flat = ((d - 60.0) / 140.0).clamp(0.0, 1.0);
    let mut h = 14.0 * (x * 0.006 + 0.5).sin() * (z * 0.0075).cos()
        + 6.0 * (x * 0.021 + 1.3).sin() * (z * 0.018 + 0.7).sin()
        + 2.0 * (x * 0.07).sin() * (z * 0.06).cos();
    h += 40.0 * ((d - 450.0) / 350.0).max(0.0).powi(2); // 外周の山
    h * flat
}

/// 元の JS と同じ Park–Miller 乱数
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next_f32(&mut self) -> f32 {
        self.0 = (self.0 * 16807) % 2147483647;
        (self.0 as f64 / 2147483647.0) as f32
    }
}

/// 建物やパッドの直方体。当たり判定にも使う
#[derive(Clone, Debug)]
pub struct Solid {
    pub id: u32,
    pub x: f32,
    pub z: f32,
    pub hw: f32,
    pub hd: f32,
    pub top: f32,
    pub base: f32,
    pub color: u32,
}

#[derive(Clone, Debug)]
pub struct Pad {
    pub solid: u32,
    /// 白い H マークの半径
    pub mark_r: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Tree {
    pub pos: Vec3,
    pub scale: f32,
}

pub struct World {
    pub solids: Vec<Solid>,
    pub home: u32,
    pub pads: Vec<Pad>,
    pub trees: Vec<Tree>,
}

pub struct Ground {
    pub h: f32,
    pub solid: Option<u32>,
}

const LANDING_DEFS: [(f32, f32, f32, f32); 8] = [
    (160.0, -60.0, 12.0, 1.2),
    (-140.0, -180.0, 10.0, 22.0),
    (260.0, 190.0, 12.0, 1.2),
    (-260.0, 110.0, 9.0, 30.0),
    (40.0, 280.0, 10.0, 14.0),
    (-60.0, -320.0, 12.0, 1.2),
    (330.0, -230.0, 9.0, 18.0),
    (-340.0, -60.0, 12.0, 1.2),
];

pub const RING_DEFS: [[f32; 3]; 10] = [
    [0.0, 18.0, -90.0],
    [70.0, 26.0, -170.0],
    [170.0, 30.0, -150.0],
    [230.0, 22.0, -60.0],
    [210.0, 40.0, 60.0],
    [110.0, 18.0, 140.0],
    [0.0, 14.0, 190.0],
    [-120.0, 34.0, 120.0],
    [-170.0, 20.0, 0.0],
    [-80.0, 12.0, -60.0],
];

/// 着陸チャレンジで回るパッドの順番 (pads の添字)
pub const LANDING_ORDER: [usize; 5] = [0, 1, 4, 3, 5];

pub const HOVER_ZONE: (f32, f32) = (0.0, -45.0);

impl World {
    pub fn build() -> World {
        let mut w = World {
            solids: Vec::new(),
            home: 0,
            pads: Vec::new(),
            trees: Vec::new(),
        };
        let mut rng = Rng::new(1337);
        let mut next_id = 0u32;
        let mut add_box = |solids: &mut Vec<Solid>,
                           x: f32,
                           z: f32,
                           bw: f32,
                           bd: f32,
                           h: f32,
                           color: u32,
                           base: Option<f32>| {
            let base = base.unwrap_or_else(|| {
                terrain_h(x - bw / 2.0, z - bd / 2.0)
                    .min(terrain_h(x + bw / 2.0, z + bd / 2.0))
                    .min(terrain_h(x, z))
                    - 1.0
            });
            let id = next_id;
            next_id += 1;
            solids.push(Solid {
                id,
                x,
                z,
                hw: bw / 2.0,
                hd: bd / 2.0,
                top: base + h,
                base,
                color,
            });
            id
        };

        // ホームパッド
        w.home = add_box(
            &mut w.solids,
            0.0,
            0.0,
            14.0,
            14.0,
            1.2,
            0x55595f,
            Some(-0.8),
        );

        // 街の建物
        for _ in 0..40 {
            let ang = rng.next_f32() * std::f32::consts::TAU;
            let dist = 90.0 + rng.next_f32() * 380.0;
            let (x, z) = (ang.cos() * dist, ang.sin() * dist);
            if w.solids
                .iter()
                .any(|s| (s.x - x).abs() < s.hw + 16.0 && (s.z - z).abs() < s.hd + 16.0)
            {
                continue;
            }
            let bw = 10.0 + rng.next_f32() * 14.0;
            let bd = 10.0 + rng.next_f32() * 14.0;
            let h = 6.0 + rng.next_f32() * 34.0;
            let gray = 0x6a + (rng.next_f32() * 0x30 as f32).floor() as u32;
            add_box(
                &mut w.solids,
                x,
                z,
                bw,
                bd,
                h,
                (gray << 16) | (gray << 8) | (gray - 8),
                None,
            );
        }

        // 着陸先: 地上パッドと屋上
        for (lx, lz, lw, lh) in LANDING_DEFS {
            let home = w.home;
            w.solids.retain(|s| {
                s.id == home || !((s.x - lx).abs() < s.hw + lw && (s.z - lz).abs() < s.hd + lw)
            });
            let ground_pad = lh < 2.0;
            let id = add_box(
                &mut w.solids,
                lx,
                lz,
                lw,
                lw,
                lh,
                if ground_pad { 0x55595f } else { 0x7a6f66 },
                ground_pad.then(|| terrain_h(lx, lz) - 0.6),
            );
            w.pads.push(Pad {
                solid: id,
                mark_r: lw * 0.42,
            });
        }

        // 木
        const N: usize = 900;
        let mut i = 0;
        while i < N * 3 && w.trees.len() < N {
            i += 1;
            let x = (rng.next_f32() - 0.5) * WORLD * 0.9;
            let z = (rng.next_f32() - 0.5) * WORLD * 0.9;
            if x.hypot(z) < 50.0 {
                continue;
            }
            if w.solids
                .iter()
                .any(|s| (s.x - x).abs() < s.hw + 4.0 && (s.z - z).abs() < s.hd + 4.0)
            {
                continue;
            }
            let y = terrain_h(x, z);
            let sc = 0.8 + rng.next_f32() * 0.8;
            w.trees.push(Tree {
                pos: Vec3::new(x, y, z),
                scale: sc,
            });
        }
        w
    }

    pub fn solid(&self, id: u32) -> &Solid {
        self.solids.iter().find(|s| s.id == id).expect("solid id")
    }

    /// (x, z) の地面の高さ。footprint の範囲に建物があればその屋上
    pub fn ground_at(&self, x: f32, z: f32, footprint: f32) -> Ground {
        let mut g = Ground {
            h: terrain_h(x, z),
            solid: None,
        };
        for s in &self.solids {
            if (x - s.x).abs() < s.hw + footprint
                && (z - s.z).abs() < s.hd + footprint
                && s.top > g.h
            {
                g.h = s.top;
                g.solid = Some(s.id);
            }
        }
        g
    }

    pub fn home_top(&self) -> f32 {
        self.solid(self.home).top
    }
}

/// リングの (中心, くぐる向き)
pub fn ring_layout() -> Vec<(Vec3, Vec3)> {
    let n = RING_DEFS.len();
    (0..n)
        .map(|i| {
            let p = RING_DEFS[i];
            let next = RING_DEFS[(i + 1) % n];
            let prev = RING_DEFS[(i + n - 1) % n];
            let pos = Vec3::new(p[0], p[1] + terrain_h(p[0], p[2]), p[2]);
            let dir = Vec3::new(next[0] - prev[0], 0.0, next[2] - prev[2]).normalize();
            (pos, dir)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_area_is_flat() {
        assert_eq!(terrain_h(0.0, 0.0), 0.0);
        assert_eq!(terrain_h(40.0, -30.0), 0.0);
    }

    #[test]
    fn rng_matches_js() {
        // JS: seed = 1337; (seed * 16807) % 2147483647 / 2147483647
        let mut r = Rng::new(1337);
        assert!((r.next_f32() - 22470959.0 / 2147483647.0).abs() < 1e-7);
    }

    #[test]
    fn world_has_all_pads_and_trees() {
        let w = World::build();
        assert_eq!(w.pads.len(), 8);
        assert!(w.trees.len() > 700, "trees: {}", w.trees.len());
        assert!((w.home_top() - 0.4).abs() < 1e-6);
        // パッドはどれも他の建物と重なっていない
        for p in &w.pads {
            let s = w.solid(p.solid);
            for o in &w.solids {
                if o.id == s.id || o.id == w.home {
                    continue;
                }
                let overlap = (o.x - s.x).abs() < o.hw + s.hw && (o.z - s.z).abs() < o.hd + s.hd;
                assert!(!overlap, "pad {} overlaps solid {}", s.id, o.id);
            }
        }
        let g = w.ground_at(0.0, 0.0, 1.0);
        assert_eq!(g.solid, Some(w.home));
    }
}
