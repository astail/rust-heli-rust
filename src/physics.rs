//! ヘリの物理。元の JS の stepPhysics をそのまま移したもの (係数も同じ)。

use crate::world::{WORLD, World};
use glam::{Mat3, Quat, Vec3};

pub const G: f32 = 9.81;
const DEG: f32 = 57.3;

#[derive(Default, Clone, Copy, Debug)]
pub struct Input {
    pub w: bool,
    pub s: bool,
    pub a: bool,
    pub d: bool,
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    pub sens: f32,
    pub invert: bool,
    /// 高度制限 (0 = なし)
    pub ceil: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Crash(&'static str),
    Touchdown { solid: Option<u32>, impact: f32 },
}

#[derive(Clone, Debug)]
pub struct Heli {
    pub pos: Vec3,
    pub vel: Vec3,
    pub q: Quat,
    pub ang_vel: Vec3,
    pub throttle: f32,
    pub rotor_spin: f32,
    pub lift_cut: f32,
    pub crashed: bool,
    pub grounded: bool,
    /// マウスが入れる回転の目標値 (x = ピッチ, z = ロール)
    pub ctrl: Vec3,
    pub turb_t: f32,
    /// A/D で付く傾き。見た目・カメラ・姿勢計には出るが推力の向きは変えない
    pub bank: f32,
}

impl Default for Heli {
    fn default() -> Self {
        Heli {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            q: Quat::IDENTITY,
            ang_vel: Vec3::ZERO,
            throttle: 0.0,
            rotor_spin: 0.0,
            lift_cut: 1.0,
            crashed: false,
            grounded: true,
            ctrl: Vec3::ZERO,
            turb_t: 0.0,
            bank: 0.0,
        }
    }
}

/// JS の Math.sign
pub fn sign(v: f32) -> f32 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Three.js の Euler 'YXZ' への分解 (x, y, z)
pub fn euler_yxz(q: Quat) -> Vec3 {
    let m = Mat3::from_quat(q);
    let (m11, m13) = (m.x_axis.x, m.z_axis.x);
    let (m21, m22, m23) = (m.x_axis.y, m.y_axis.y, m.z_axis.y);
    let (m31, m33) = (m.x_axis.z, m.z_axis.z);
    let x = (-m23.clamp(-1.0, 1.0)).asin();
    if m23.abs() < 0.9999999 {
        Vec3::new(x, m13.atan2(m33), m21.atan2(m22))
    } else {
        Vec3::new(x, (-m31).atan2(m11), 0.0)
    }
}

pub fn quat_yxz(e: Vec3) -> Quat {
    Quat::from_rotation_y(e.y) * Quat::from_rotation_x(e.x) * Quat::from_rotation_z(e.z)
}

/// 水平線に対するピッチ / ロール (度)
pub fn attitude(q: Quat) -> (f32, f32) {
    let fwd = q * Vec3::NEG_Z;
    let right = q * Vec3::X;
    let pitch = fwd.y.clamp(-1.0, 1.0).asin();
    let roll = (-right.y).clamp(-1.0, 1.0).asin();
    (pitch * DEG, roll * DEG)
}

impl Heli {
    pub fn respawn(&mut self, pos: Vec3, yaw: f32) {
        self.pos = pos;
        self.vel = Vec3::ZERO;
        self.ang_vel = Vec3::ZERO;
        self.q = Quat::from_rotation_y(yaw);
        self.ctrl = Vec3::ZERO;
        self.bank = 0.0;
        self.throttle = 0.0;
        self.lift_cut = 1.0;
        self.crashed = false;
        self.grounded = true;
    }

    /// 見た目の姿勢 (物理の姿勢 + A/D の傾き)
    pub fn vis_q(&self) -> Quat {
        self.q * Quat::from_axis_angle(Vec3::Z, self.bank)
    }

    pub fn up_y(&self) -> f32 {
        (self.q * Vec3::Y).y
    }

    /// ローターの回転の速さ (音と計器で使う)
    pub fn rotor_rate(&self) -> f32 {
        self.rotor_spin * (0.8 + 0.35 * self.throttle)
    }

    fn crash(&mut self, reason: &'static str) -> Option<Event> {
        self.crashed = true;
        Some(Event::Crash(reason))
    }

    /// 1 サブステップ進める。mouse は使った分を 0 に戻す
    pub fn step(
        &mut self,
        dt: f32,
        inp: Input,
        mouse: &mut (f32, f32),
        t: &Tuning,
        world: &World,
    ) -> Option<Event> {
        if self.crashed {
            self.vel = Vec3::ZERO;
            return None;
        }
        // --- スロットル (W/S): W で上昇、S で下降、離すとゆっくり沈む ---
        let (up, down) = (inp.w, inp.s);
        let mut target = if up && !down {
            1.0
        } else if down && !up {
            0.0
        } else {
            0.3 // W を離すと 30% に落ち着く
        };
        if self.grounded && !up {
            target = 0.0;
        }
        // Ctrl を押している間は揚力が抜ける (Ctrl+W では離陸しない、空中なら落ちる)
        let cut = inp.ctrl;
        if cut {
            target = 0.0;
        }
        self.lift_cut += ((if cut { 0.15 } else { 1.0 }) - self.lift_cut)
            * (dt * if cut { 3.0 } else { 1.5 }).min(1.0);
        self.throttle += (target - self.throttle) * (dt * 0.7).min(1.0); // 重いローターのように遅れて効く
        let spin_target = if self.grounded && self.throttle < 0.05 && !cut {
            0.25
        } else {
            1.0
        };
        self.rotor_spin += (spin_target - self.rotor_spin) * (dt * 0.6).min(1.0); // エンジンの回り始め

        // --- 回転の入力 ---
        let sens = 0.0035 * t.sens;
        let inv = if t.invert { -1.0 } else { 1.0 };
        // マウスを前に出す (dy が負) と機首が下がる
        let pitch_in = mouse.1 * sens * inv;
        let roll_in = -mouse.0 * sens;
        *mouse = (0.0, 0.0);
        let yaw_in = (inp.a as i32 - inp.d as i32) as f32;

        // マウスは回転の目標値を動かし、機体の慣性がそれに遅れてついてくる
        if !self.grounded {
            self.ctrl.x += pitch_in * 3.0;
            self.ctrl.z += roll_in * 3.0;
        }
        self.ctrl.x = self.ctrl.x.clamp(-0.6, 0.6);
        self.ctrl.z = self.ctrl.z.clamp(-0.6, 0.6);
        let cd = (-dt * 1.25).exp();
        self.ctrl.x *= cd;
        self.ctrl.z *= cd;
        let inertia = (dt * 0.75).min(1.0); // 重い機体: 回り始めも止まるのも遅い
        self.ang_vel.x += (self.ctrl.x - self.ang_vel.x) * inertia;
        self.ang_vel.z += (self.ctrl.z - self.ang_vel.z) * inertia;
        // 地上では Ctrl を押しているときだけ向きを変えられる (地上走行)
        let yaw_target = yaw_in * 0.6 * if self.grounded && !cut { 0.0 } else { 1.0 };
        self.ang_vel.y += (yaw_target - self.ang_vel.y) * (dt * 0.45).min(1.0);
        // A/D で旋回方向へ傾く (最大 20° ほど)。自動では戻らない
        if !self.grounded && yaw_in != 0.0 {
            self.bank = (self.bank + yaw_in * 0.2 * dt).clamp(-0.35, 0.35);
        }
        // 軽い乱気流
        if !self.grounded {
            self.turb_t += dt;
            let tt = self.turb_t;
            self.ctrl.x += (tt * 1.7).sin() * (tt * 0.63).sin() * dt * 0.05;
            self.ctrl.z += (tt * 1.3 + 2.0).sin() * (tt * 0.41).sin() * dt * 0.05;
        }

        // ピッチは機体の x 軸、ロールは z 軸、ヨーは世界の上向き軸で回す。
        // 傾きと逆向きのマウスロールは、まず傾きを戻すのに使い、余りだけが本当の姿勢を回す
        let mut roll_step = self.ang_vel.z * dt;
        if self.bank != 0.0 && roll_step != 0.0 && sign(roll_step) != sign(self.bank) {
            let nb = self.bank + roll_step;
            if sign(nb) == sign(self.bank) {
                self.bank = nb;
                roll_step = 0.0;
            } else {
                roll_step = nb;
                self.bank = 0.0;
            }
        }
        self.q *= Quat::from_rotation_x(self.ang_vel.x * dt) * Quat::from_rotation_z(roll_step);
        self.q = (Quat::from_rotation_y(self.ang_vel.y * dt) * self.q).normalize();

        // --- 力 ---
        // 推力 (重力比): 0 → 0.6g、0.5 → 0.95g (ゆっくり沈む)、1 → 1.25g (ゆるい上昇)
        let thrust_g = if self.throttle < 0.5 {
            0.6 + self.throttle * 0.7
        } else {
            0.95 + (self.throttle - 0.5) * 0.6
        };
        let mut thrust = thrust_g * G * self.rotor_spin * self.lift_cut;
        // 高度制限: 最後の 15 m で揚力が弱まり、超えると押し戻される
        if t.ceil > 0.0 {
            let over = self.pos.y - (t.ceil - 15.0);
            if over > 0.0 {
                thrust *= (1.0 - over / 15.0 * 0.45).max(0.6);
            }
            if self.pos.y > t.ceil && self.vel.y > 0.0 {
                self.vel.y *= (-dt * 6.0).exp();
            }
        }
        let mut acc = (self.q * Vec3::Y) * thrust;
        acc.y -= G;
        // 抵抗
        let hv = Vec3::new(self.vel.x, 0.0, self.vel.z);
        acc += hv * (-0.025 - 0.0035 * hv.length());
        acc.y -= self.vel.y * 0.24;
        if self.grounded && acc.y < 0.0 {
            acc.y = 0.0;
        }
        self.vel += acc * dt;
        self.pos += self.vel * dt;

        // --- 衝突 ---
        let gnd = world.ground_at(self.pos.x, self.pos.z, 1.0);
        let tilt_up = self.up_y();
        let p = self.pos;
        // 建物の側面にぶつかった (胴体が屋上より下で建物の中に入った)
        for s in &world.solids {
            if (p.x - s.x).abs() < s.hw
                && (p.z - s.z).abs() < s.hd
                && p.y < s.top - 0.6
                && p.y > s.base
            {
                return self.crash("建物に衝突");
            }
        }
        // 近くの建物にローターが当たった
        for s in &world.solids {
            let (dx, dz) = ((p.x - s.x).abs(), (p.z - s.z).abs());
            if dx < s.hw + 3.4
                && dz < s.hd + 3.4
                && p.y + 2.0 > s.base
                && p.y + 1.6 < s.top
                && !(dx < s.hw + 1.0 && dz < s.hd + 1.0)
            {
                return self.crash("ローターが接触");
            }
        }
        let mut event = None;
        // 地上走行中の下り坂では、段差ごとに跳ねずに地面に張り付く
        let glue = self.grounded && cut && self.pos.y - gnd.h < 0.6 && self.vel.y < 1.0;
        if self.pos.y <= gnd.h || glue {
            let impact = -self.vel.y;
            let hs = hv.length();
            if !self.grounded && (impact > 7.0 || hs > 12.0 || tilt_up < 0.8) {
                return self.crash(if impact > 7.0 {
                    "強く接地しすぎ"
                } else if tilt_up < 0.8 {
                    "傾いたまま接地"
                } else {
                    "速度超過で接地"
                });
            }
            self.pos.y = gnd.h;
            if self.vel.y < 0.0 {
                self.vel.y = 0.0;
            }
            // 地上で Ctrl+W / Ctrl+S: 浮かずに前進 / ゆっくり後退
            let taxi = if cut {
                (if inp.w { 1.0 } else { 0.0 }) - (if inp.s { 0.4 } else { 0.0 })
            } else {
                0.0
            };
            // 進行方向と右方向の地面の傾き (前後 1 m ずつ測る)
            let mut fw = self.q * Vec3::NEG_Z;
            fw.y = 0.0;
            let fw = fw.normalize_or_zero();
            let rt = Vec3::new(-fw.z, 0.0, fw.x);
            let h_at = |dx: f32, dz: f32| world.ground_at(self.pos.x + dx, self.pos.z + dz, 1.0).h;
            let slope_f = ((h_at(fw.x, fw.z) - h_at(-fw.x, -fw.z)) / 2.0).atan(); // + = 前が上り
            let slope_r = ((h_at(rt.x, rt.z) - h_at(-rt.x, -rt.z)) / 2.0).atan(); // + = 右が高い
            if taxi != 0.0 {
                // 急なほど登りが遅くなり、28° くらいからは登れない
                let up = slope_f * sign(taxi) * DEG;
                let climb = if up > 0.0 {
                    (1.0 - (up / 28.0).powf(1.5)).max(0.0)
                } else {
                    1.0
                };
                let k = (dt * 1.2).min(1.0);
                let tv = 6.0 * taxi * climb; // 平地で約 22 km/h
                self.vel.x += (fw.x * tv - self.vel.x) * k;
                self.vel.z += (fw.z * tv - self.vel.z) * k;
            } else {
                let f = (-dt * 4.0).exp();
                self.vel.x *= f;
                self.vel.z *= f;
            }
            if !self.grounded {
                event = Some(Event::Touchdown {
                    solid: gnd.solid,
                    impact,
                });
            }
            self.grounded = true;
            // 地面の傾きに沿って座る (上り坂で機首上げ、横の傾斜でロール)
            let mut e = euler_yxz(self.q);
            let (lim, ks) = (0.45, 1.0 - (-dt * 8.0).exp());
            e.x += (slope_f.clamp(-lim, lim) - e.x) * ks;
            e.z += (slope_r.clamp(-lim, lim) - e.z) * ks;
            self.q = quat_yxz(e);
            self.ang_vel.x = 0.0;
            self.ang_vel.z = 0.0;
            self.ctrl.x = 0.0;
            self.ctrl.z = 0.0;
            self.bank = 0.0;
        } else if self.pos.y > gnd.h + 0.15 {
            self.grounded = false;
        }
        if tilt_up < -0.2 && self.pos.y - gnd.h < 3.0 {
            return self.crash("横転");
        }
        // 世界の端
        let lim = WORLD / 2.0 - 30.0;
        if self.pos.x.abs() > lim || self.pos.z.abs() > lim {
            self.pos.x = self.pos.x.clamp(-lim, lim);
            self.pos.z = self.pos.z.clamp(-lim, lim);
            self.vel *= 0.3;
        }
        if self.pos.y > 400.0 {
            self.pos.y = 400.0;
            self.vel.y = self.vel.y.min(0.0);
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TUNE: Tuning = Tuning {
        sens: 0.5,
        invert: false,
        ceil: 150.0,
    };

    /// 元のフレームループと同じく、1 フレームを 4 サブステップで回す
    fn run(h: &mut Heli, w: &World, inp: Input, secs: f32) -> Vec<Event> {
        let dt = 1.0 / 60.0;
        let mut ev = Vec::new();
        for _ in 0..(secs / dt) as usize {
            let mut mouse = (0.0, 0.0);
            for _ in 0..4 {
                if let Some(e) = h.step(dt / 4.0, inp, &mut mouse, &TUNE, w) {
                    ev.push(e);
                }
            }
        }
        ev
    }

    fn on_home(w: &World) -> Heli {
        let mut h = Heli::default();
        h.respawn(Vec3::new(0.0, w.home_top(), 4.0), 0.0);
        h
    }

    #[test]
    fn idle_on_the_pad_stays_put() {
        let w = World::build();
        let mut h = on_home(&w);
        let ev = run(&mut h, &w, Input::default(), 3.0);
        assert!(ev.is_empty());
        assert!(h.grounded);
        assert!((h.pos.y - w.home_top()).abs() < 1e-4);
        assert!(h.pos.distance(Vec3::new(0.0, w.home_top(), 4.0)) < 1e-3);
    }

    #[test]
    fn holding_w_takes_off_and_climbs() {
        let w = World::build();
        let mut h = on_home(&w);
        run(
            &mut h,
            &w,
            Input {
                w: true,
                ..Default::default()
            },
            8.0,
        );
        assert!(!h.grounded);
        assert!(h.pos.y > w.home_top() + 3.0, "y = {}", h.pos.y);
        assert!(h.vel.y > 0.0);
    }

    #[test]
    fn ctrl_w_does_not_take_off_but_taxis_forward() {
        let w = World::build();
        let mut h = Heli::default();
        h.respawn(Vec3::new(0.0, 0.0, 30.0), 0.0); // ホームパッドの外の平地
        run(
            &mut h,
            &w,
            Input {
                w: true,
                ctrl: true,
                ..Default::default()
            },
            3.0,
        );
        assert!(h.grounded);
        assert!(h.pos.z < 30.0 - 5.0, "z = {}", h.pos.z); // 機首は -Z
        assert!(h.pos.y.abs() < 1e-3);
    }

    #[test]
    fn dropping_with_lift_cut_crashes() {
        let w = World::build();
        let mut h = Heli::default();
        h.respawn(Vec3::new(0.0, 30.0, 30.0), 0.0);
        h.grounded = false;
        h.rotor_spin = 1.0;
        let ev = run(
            &mut h,
            &w,
            Input {
                ctrl: true,
                ..Default::default()
            },
            6.0,
        );
        assert_eq!(ev, vec![Event::Crash("強く接地しすぎ")]);
        assert!(h.crashed);
    }

    #[test]
    fn soft_landing_on_the_pad_is_a_touchdown() {
        let w = World::build();
        let mut h = on_home(&w);
        h.pos.y += 1.5;
        h.grounded = false;
        h.rotor_spin = 1.0;
        h.throttle = 0.45;
        let ev = run(&mut h, &w, Input::default(), 4.0);
        assert!(
            matches!(ev.as_slice(), [Event::Touchdown { solid: Some(id), .. }] if *id == w.home),
            "{ev:?}"
        );
    }

    #[test]
    fn euler_yxz_round_trips() {
        let e = Vec3::new(0.2, -1.1, 0.3);
        let back = euler_yxz(quat_yxz(e));
        assert!(back.distance(e) < 1e-5, "{back:?}");
    }

    #[test]
    fn level_attitude_is_zero() {
        let (p, r) = attitude(Quat::from_rotation_y(0.7));
        assert!(p.abs() < 1e-4 && r.abs() < 1e-4);
        let (p, _) = attitude(Quat::from_rotation_x(-0.2)); // 機首下げ
        assert!(p < -10.0);
    }
}
