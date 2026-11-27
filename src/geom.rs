//! CPU 側のメッシュと、Three.js と同じ形を作るプリミティブ生成。
//! 面は表から見て反時計回り (CCW)、法線は外向き。

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Default, Debug)]
pub struct CpuMesh {
    pub pos: Vec<Vec3>,
    pub nrm: Vec<Vec3>,
    pub uv: Vec<Vec2>,
    /// 頂点カラー (空なら使わない)
    pub col: Vec<Vec3>,
    pub idx: Vec<u32>,
}

impl CpuMesh {
    fn vert(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.pos.push(p);
        self.nrm.push(n);
        self.uv.push(uv);
        (self.pos.len() - 1) as u32
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.idx.extend_from_slice(&[a, b, c]);
    }

    /// 頂点を行列で変換する (法線は逆転置で)
    pub fn transform(&mut self, m: Mat4) {
        let nm = Mat3::from_mat4(m).inverse().transpose();
        for p in &mut self.pos {
            *p = m.transform_point3(*p);
        }
        for n in &mut self.nrm {
            *n = (nm * *n).normalize_or_zero();
        }
    }

    pub fn transformed(mut self, m: Mat4) -> Self {
        self.transform(m);
        self
    }

    /// 別のメッシュを継ぎ足す (頂点カラーはどちらも持っているときだけ残す)
    pub fn append(&mut self, o: &CpuMesh) {
        let base = self.pos.len() as u32;
        let keep_col = (self.pos.is_empty() || !self.col.is_empty()) && !o.col.is_empty();
        self.pos.extend_from_slice(&o.pos);
        self.nrm.extend_from_slice(&o.nrm);
        self.uv.extend_from_slice(&o.uv);
        if keep_col {
            self.col.extend_from_slice(&o.col);
        } else {
            self.col.clear();
        }
        self.idx.extend(o.idx.iter().map(|i| i + base));
    }

    /// 面の法線を面積で重み付けして頂点法線を作り直す (Three.js の computeVertexNormals 相当)
    pub fn compute_normals(&mut self) {
        let mut acc = vec![Vec3::ZERO; self.pos.len()];
        for t in self.idx.as_chunks::<3>().0 {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let n = (self.pos[b] - self.pos[a]).cross(self.pos[c] - self.pos[a]);
            acc[a] += n;
            acc[b] += n;
            acc[c] += n;
        }
        self.nrm = acc.into_iter().map(|n| n.normalize_or(Vec3::Y)).collect();
    }
}

/// 直方体 (中心が原点)
pub fn cuboid(w: f32, h: f32, d: f32) -> CpuMesh {
    let half = Vec3::new(w, h, d) * 0.5;
    let mut m = CpuMesh::default();
    // (法線, u 軸, v 軸)  u × v = 法線
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    for (n, u, v) in faces {
        let s = m.pos.len() as u32;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = (n + u * su + v * sv) * half;
            m.vert(p, n, Vec2::new((su + 1.0) / 2.0, (sv + 1.0) / 2.0));
        }
        m.tri(s, s + 1, s + 2);
        m.tri(s, s + 2, s + 3);
    }
    m
}

/// XY 平面の板 (+Z 向き)
pub fn plane(w: f32, h: f32) -> CpuMesh {
    let mut m = CpuMesh::default();
    for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        m.vert(
            Vec3::new(su * w / 2.0, sv * h / 2.0, 0.0),
            Vec3::Z,
            Vec2::new((su + 1.0) / 2.0, (sv + 1.0) / 2.0),
        );
    }
    m.tri(0, 1, 2);
    m.tri(0, 2, 3);
    m
}

/// XZ 平面の格子 (+Y 向き)。地形用
pub fn grid(size: f32, segs: u32) -> CpuMesh {
    let mut m = CpuMesh::default();
    let step = size / segs as f32;
    let n = segs + 1;
    for iz in 0..n {
        for ix in 0..n {
            let x = -size / 2.0 + ix as f32 * step;
            let z = -size / 2.0 + iz as f32 * step;
            m.vert(
                Vec3::new(x, 0.0, z),
                Vec3::Y,
                Vec2::new(ix as f32 / segs as f32, iz as f32 / segs as f32),
            );
        }
    }
    for iz in 0..segs {
        for ix in 0..segs {
            let a = ix + n * iz;
            let b = ix + n * (iz + 1);
            let c = ix + 1 + n * (iz + 1);
            let d = ix + 1 + n * iz;
            m.tri(a, b, d);
            m.tri(b, c, d);
        }
    }
    m
}

pub fn sphere(r: f32, wseg: u32, hseg: u32) -> CpuMesh {
    let mut m = CpuMesh::default();
    let mut rows = Vec::new();
    for iy in 0..=hseg {
        let v = iy as f32 / hseg as f32;
        let mut row = Vec::new();
        for ix in 0..=wseg {
            let u = ix as f32 / wseg as f32;
            let (phi, theta) = (u * TAU, v * PI);
            let p = Vec3::new(
                -r * phi.cos() * theta.sin(),
                r * theta.cos(),
                r * phi.sin() * theta.sin(),
            );
            row.push(m.vert(p, p.normalize_or(Vec3::Y), Vec2::new(u, 1.0 - v)));
        }
        rows.push(row);
    }
    for iy in 0..hseg as usize {
        for ix in 0..wseg as usize {
            let a = rows[iy][ix + 1];
            let b = rows[iy][ix];
            let c = rows[iy + 1][ix];
            let d = rows[iy + 1][ix + 1];
            if iy != 0 {
                m.tri(a, b, d);
            }
            if iy != hseg as usize - 1 {
                m.tri(b, c, d);
            }
        }
    }
    m
}

/// Y 軸方向の円柱 (open_ended なら上下のふた無し)
pub fn cylinder(r_top: f32, r_bot: f32, h: f32, radial: u32, open_ended: bool) -> CpuMesh {
    let mut m = CpuMesh::default();
    let slope = (r_bot - r_top) / h;
    let mut rows = Vec::new();
    for y in 0..=1u32 {
        let v = y as f32;
        let radius = v * (r_bot - r_top) + r_top;
        let mut row = Vec::new();
        for x in 0..=radial {
            let u = x as f32 / radial as f32;
            let (s, c) = (u * TAU).sin_cos();
            let p = Vec3::new(radius * s, -v * h + h / 2.0, radius * c);
            let n = Vec3::new(s, slope, c).normalize();
            row.push(m.vert(p, n, Vec2::new(u, 1.0 - v)));
        }
        rows.push(row);
    }
    for x in 0..radial as usize {
        let a = rows[0][x];
        let b = rows[1][x];
        let c = rows[1][x + 1];
        let d = rows[0][x + 1];
        m.tri(a, b, d);
        m.tri(b, c, d);
    }
    if !open_ended {
        for top in [true, false] {
            let radius = if top { r_top } else { r_bot };
            if radius <= 0.0 {
                continue;
            }
            let sign = if top { 1.0 } else { -1.0 };
            let y = h / 2.0 * sign;
            let n = Vec3::new(0.0, sign, 0.0);
            let center0 = m.pos.len() as u32;
            for _ in 0..radial {
                m.vert(Vec3::new(0.0, y, 0.0), n, Vec2::splat(0.5));
            }
            let ring0 = m.pos.len() as u32;
            for x in 0..=radial {
                let (s, c) = (x as f32 / radial as f32 * TAU).sin_cos();
                m.vert(
                    Vec3::new(radius * s, y, radius * c),
                    n,
                    Vec2::new(c * 0.5 + 0.5, s * 0.5 * sign + 0.5),
                );
            }
            for x in 0..radial {
                let c = center0 + x;
                let i = ring0 + x;
                if top {
                    m.tri(i, i + 1, c);
                } else {
                    m.tri(i + 1, i, c);
                }
            }
        }
    }
    m
}

pub fn cone(r: f32, h: f32, radial: u32) -> CpuMesh {
    cylinder(0.0, r, h, radial, false)
}

/// XY 平面のドーナツ (軸は Z)
pub fn torus(radius: f32, tube: f32, radial: u32, tubular: u32) -> CpuMesh {
    let mut m = CpuMesh::default();
    for j in 0..=radial {
        for i in 0..=tubular {
            let u = i as f32 / tubular as f32 * TAU;
            let v = j as f32 / radial as f32 * TAU;
            let p = Vec3::new(
                (radius + tube * v.cos()) * u.cos(),
                (radius + tube * v.cos()) * u.sin(),
                tube * v.sin(),
            );
            let center = Vec3::new(radius * u.cos(), radius * u.sin(), 0.0);
            m.vert(
                p,
                (p - center).normalize(),
                Vec2::new(i as f32 / tubular as f32, j as f32 / radial as f32),
            );
        }
    }
    let t1 = tubular + 1;
    for j in 1..=radial {
        for i in 1..=tubular {
            let a = t1 * j + i - 1;
            let b = t1 * (j - 1) + i - 1;
            let c = t1 * (j - 1) + i;
            let d = t1 * j + i;
            m.tri(a, b, d);
            m.tri(b, c, d);
        }
    }
    m
}

/// XY 平面の輪 (+Z 向き)
pub fn ring(inner: f32, outer: f32, segs: u32) -> CpuMesh {
    let mut m = CpuMesh::default();
    for radius in [inner, outer] {
        for i in 0..=segs {
            let (s, c) = (i as f32 / segs as f32 * TAU).sin_cos();
            let p = Vec3::new(radius * c, radius * s, 0.0);
            m.vert(
                p,
                Vec3::Z,
                Vec2::new((p.x / outer + 1.0) / 2.0, (p.y / outer + 1.0) / 2.0),
            );
        }
    }
    for i in 0..segs {
        let a = i;
        let b = i + segs + 1;
        let c = i + segs + 2;
        let d = i + 1;
        m.tri(a, b, d);
        m.tri(b, c, d);
    }
    m
}

/// XY 平面の円板 (+Z 向き)
pub fn circle(r: f32, segs: u32) -> CpuMesh {
    let mut m = CpuMesh::default();
    m.vert(Vec3::ZERO, Vec3::Z, Vec2::splat(0.5));
    for s in 0..=segs {
        let (sn, cs) = (s as f32 / segs as f32 * TAU).sin_cos();
        m.vert(
            Vec3::new(r * cs, r * sn, 0.0),
            Vec3::Z,
            Vec2::new((cs + 1.0) / 2.0, (sn + 1.0) / 2.0),
        );
    }
    for i in 1..=segs {
        m.tri(i, i + 1, 0);
    }
    m
}

/// 2 点を結ぶ細い円柱の (位置, 向き, 長さ)
pub fn bar_placement(a: Vec3, b: Vec3) -> (Vec3, Quat, f32) {
    let len = a.distance(b);
    let rot = Quat::from_rotation_arc(Vec3::Y, (b - a).normalize());
    ((a + b) * 0.5, rot, len)
}

/// +Z を dir に向ける回転 (Three.js の Object3D.lookAt 相当、カメラ以外)
pub fn look_dir_z(dir: Vec3) -> Quat {
    let z = dir.normalize();
    let mut x = Vec3::Y.cross(z);
    if x.length_squared() < 1e-8 {
        x = Vec3::X;
    }
    let x = x.normalize();
    let y = z.cross(x);
    Quat::from_mat3(&Mat3::from_cols(x, y, z))
}

/// カメラの -Z を target へ向ける回転 (Three.js の Camera.lookAt 相当)
pub fn look_at_camera(eye: Vec3, target: Vec3) -> Quat {
    let mut z = eye - target;
    if z.length_squared() < 1e-12 {
        z = Vec3::Z;
    }
    let z = z.normalize();
    let mut x = Vec3::Y.cross(z);
    if x.length_squared() < 1e-8 {
        // 真上/真下を見るときは少しずらす
        let zz = (z + Vec3::new(0.0, 0.0, 1e-4)).normalize();
        x = Vec3::Y.cross(zz);
    }
    let x = x.normalize();
    let y = z.cross(x);
    Quat::from_mat3(&Mat3::from_cols(x, y, z))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 各三角形の巻き方向から出した法線が、頂点法線と同じ側を向いていること
    fn assert_outward(name: &str, m: &CpuMesh) {
        assert!(!m.idx.is_empty(), "{name}: no triangles");
        for t in m.idx.as_chunks::<3>().0 {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let face = (m.pos[b] - m.pos[a]).cross(m.pos[c] - m.pos[a]);
            if face.length() < 1e-9 {
                continue; // 極の縮退三角形
            }
            let vn = m.nrm[a] + m.nrm[b] + m.nrm[c];
            assert!(face.dot(vn) > 0.0, "{name}: triangle {t:?} is wound inward");
        }
    }

    #[test]
    fn primitives_are_wound_outward() {
        assert_outward("cuboid", &cuboid(1.0, 2.0, 3.0));
        assert_outward("plane", &plane(2.0, 1.0));
        assert_outward("grid", &grid(10.0, 4));
        assert_outward("sphere", &sphere(1.0, 12, 10));
        assert_outward("cylinder", &cylinder(0.3, 0.4, 3.0, 6, false));
        assert_outward("cone", &cone(2.4, 7.0, 7));
        assert_outward("torus", &torus(7.0, 0.45, 8, 32));
        assert_outward("ring", &ring(0.78, 0.9, 40));
        assert_outward("circle", &circle(0.085, 32));
    }

    #[test]
    fn look_helpers_point_the_right_axis() {
        let d = Vec3::new(1.0, 0.0, 1.0).normalize();
        assert!((look_dir_z(d) * Vec3::Z).distance(d) < 1e-5);
        let q = look_at_camera(Vec3::ZERO, Vec3::new(0.0, 0.0, -5.0));
        assert!((q * Vec3::NEG_Z).distance(Vec3::NEG_Z) < 1e-5);
        let q = look_at_camera(Vec3::ZERO, Vec3::new(3.0, -1.0, 2.0));
        assert!((q * Vec3::NEG_Z).distance(Vec3::new(3.0, -1.0, 2.0).normalize()) < 1e-5);
    }

    #[test]
    fn bar_spans_both_points() {
        let (a, b) = (Vec3::new(-0.25, 1.28, 0.42), Vec3::new(-0.12, 0.92, 0.0));
        let (mid, rot, len) = bar_placement(a, b);
        assert!((mid + rot * Vec3::new(0.0, len / 2.0, 0.0)).distance(b) < 1e-5);
    }
}
