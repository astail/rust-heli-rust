//! ゲーム全体の状態と 1 フレームの処理 (物理 → モード → カメラ → HUD → 音 → 描画)。

use crate::audio::{Drive, Rotor};
use crate::geom::look_at_camera;
use crate::gfx::{Gpu, Lights, hex};
use crate::hud::{self, MousePad};
use crate::models::{self, DEBRIS_PER_CRASH, HeliNodes, WorldNodes};
use crate::modes::{HoverState, Mission, Mode, Outcome, fmt_time};
use crate::physics::{self, Event, G, Heli, Input, Tuning};
use crate::scene::Scene;
use crate::store;
use crate::ui::{self, Dom};
use crate::world::World;
use glam::camera::rh::{proj::opengl as proj_gl, view as view_rh};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use wasm_bindgen::prelude::*;

pub const BG: u32 = 0xa9c2d6;

/// 視点ごとのカメラ位置  (0 TPS, 1 チェイス, 2 コックピット, 3 俯瞰, 4 近接, 5 機内, 6 パイロット)
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CamCfg {
    pub dist: f32,
    pub height: f32,
    pub side: f32,
    pub tilt: f32,
}

const fn cam(dist: f32, height: f32, side: f32, tilt: f32) -> CamCfg {
    CamCfg {
        dist,
        height,
        side,
        tilt,
    }
}

pub const CAM_DEF: [CamCfg; 7] = [
    cam(8.0, 2.4, 0.0, 0.0),
    cam(13.0, 4.8, 0.0, 0.0),
    cam(-0.6, 1.35, 0.0, 0.0),
    cam(22.0, 26.0, 0.0, 0.0),
    cam(4.2, 3.3, 0.0, 0.0),
    cam(1.45, 2.0, 0.34, 11.0),
    cam(0.4, 1.5, 0.0, 5.0),
];
/// スライダーの範囲 (± の対称): [距離, 高さ, 左右]
pub const CAM_RANGE: [[f32; 3]; 7] = [
    [20.0, 20.0, 6.0],
    [30.0, 30.0, 10.0],
    [3.5, 3.5, 1.5],
    [50.0, 50.0, 20.0],
    [8.0, 8.0, 3.0],
    [3.5, 3.5, 1.5],
    [3.5, 3.5, 1.5],
];
pub const CAMS: [&str; 7] = [
    "TPS",
    "チェイス",
    "コックピット",
    "俯瞰",
    "近接",
    "機内",
    "パイロット",
];
/// パイロット → 機内 → コックピット → 近接 → TPS → チェイス → 俯瞰
pub const CAM_ORDER: [usize; 7] = [6, 5, 2, 4, 0, 1, 3];
/// 追従カメラ: [前方を見る距離, 見上げる高さ, 追従の硬さ]。2/5/6 は無い (墜落時はチェイスで見る)
const FOLLOW: [Option<[f32; 3]>; 7] = [
    Some([8.0, 3.2, 5.0]),
    Some([0.0, 1.8, 3.5]),
    None,
    Some([4.0, 0.0, 4.0]),
    Some([16.0, 2.2, 10.0]),
    None,
    None,
];

pub struct Opts {
    pub sens: f32,
    pub invert: bool,
    pub ceil: f32,
    pub cam: usize,
    pub pivot: f32,
    pub fs: bool,
    pub sound: bool,
    pub vol: f32,
    pub dash: f32,
}

impl Opts {
    pub fn load() -> Opts {
        let mut cam = store::get_or("cam3", 6usize);
        if cam == 2 || cam > 6 {
            cam = 6; // コックピットはデフォルトにできない
        }
        Opts {
            sens: store::get_or("sens2", 0.5),
            invert: store::get_or("invert", false),
            ceil: store::get_or("ceil", 150.0),
            cam,
            pivot: store::get_or("pivot", 1.0),
            fs: store::get_or("fs", true),
            sound: store::get_or("sound", true),
            vol: store::get_or("vol2", 0.25),
            dash: store::get_or("dashop", 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct HudBox {
    pub x: f64,
    pub y: f64,
    pub s: f64,
}

struct Debris {
    node: usize,
    v: Vec3,
    t: f32,
    spin: f32,
}

pub struct App {
    pub gpu: Gpu,
    pub scene: Scene,
    pub world: World,
    pub wn: WorldNodes,
    pub hn: HeliNodes,
    pub heli: Heli,
    pub mission: Mission,
    pub mode: Mode,
    pub last_mode: Option<Mode>,
    pub opts: Opts,
    pub cam_cfg: [CamCfg; 7],
    pub cam_mode: usize,
    pub cam_edit: usize,
    pub keys: HashSet<String>,
    pub mouse: (f32, f32),
    pub pad: MousePad,
    pub running: bool,
    pub menu_open: bool,
    pub hud_edit: bool,
    pub hud_layout: BTreeMap<String, HudBox>,
    pub hud_vis: BTreeMap<String, bool>,
    pub audio: Option<Rotor>,
    pub dom: Dom,
    pub drag: Option<ui::Drag>,
    pub bar_drag: Option<(f64, f64, f64, f64)>,
    crash_timer: f32,
    cam_pos: Vec3,
    cam_q: Quat,
    eye: Vec3,
    eye_q: Quat,
    needle: [f32; 5],
    rotor_angle: f32,
    tail_angle: f32,
    marker_angle: f32,
    debris: Vec<Debris>,
    debris_next: usize,
    toast_until: f64,
    last_time: f64,
    last_msg: String,
}

pub fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

impl App {
    pub fn new() -> Result<App, JsValue> {
        let dom = Dom::new()?;
        let mut gpu = Gpu::new(dom.canvas.clone(), hex(BG))?;
        let mut scene = Scene::default();
        scene.mat(crate::gfx::Material::basic(0xffffff)); // 0 番 = グループ用のダミー
        let world = World::build();
        let mission = Mission::new();
        let wn = models::build_world(&mut gpu, &mut scene, &world, &mission.rings);
        let hn = models::build_heli(&mut gpu, &mut scene)?;
        let opts = Opts::load();

        let saved: BTreeMap<String, CamCfg> = store::get_or("camcfg", BTreeMap::new());
        let mut cam_cfg = CAM_DEF;
        for (i, c) in cam_cfg.iter_mut().enumerate() {
            if let Some(s) = saved.get(&i.to_string()) {
                *c = *s;
            }
        }
        let mut hud_vis: BTreeMap<String, bool> = ["all", "stats", "attWrap", "keys", "mission"]
            .iter()
            .map(|k| (k.to_string(), true))
            .collect();
        hud_vis.extend(store::get_or::<BTreeMap<String, bool>>(
            "hudvis",
            BTreeMap::new(),
        ));

        let t = now();
        let cam_mode = opts.cam;
        let mut app = App {
            gpu,
            scene,
            world,
            wn,
            hn,
            heli: Heli::default(),
            mission,
            mode: Mode::Free,
            last_mode: None,
            opts,
            cam_cfg,
            cam_mode,
            cam_edit: cam_mode,
            keys: HashSet::new(),
            mouse: (0.0, 0.0),
            pad: MousePad::new(t),
            running: false,
            menu_open: true,
            hud_edit: false,
            hud_layout: store::get_or("hudlayout", BTreeMap::new()),
            hud_vis,
            audio: None,
            dom,
            drag: None,
            bar_drag: None,
            crash_timer: 0.0,
            cam_pos: Vec3::new(0.0, 8.0, 14.0),
            cam_q: Quat::IDENTITY,
            eye: Vec3::ZERO,
            eye_q: Quat::IDENTITY,
            needle: [0.0; 5],
            rotor_angle: 0.0,
            tail_angle: 0.0,
            marker_angle: 0.0,
            debris: Vec::new(),
            debris_next: 0,
            toast_until: 0.0,
            last_time: t,
            last_msg: String::new(),
        };
        app.resize();
        app.respawn();
        Ok(app)
    }

    pub fn ctrl_held(&self) -> bool {
        self.keys.contains("ControlLeft") || self.keys.contains("ControlRight")
    }

    fn input(&self) -> Input {
        Input {
            w: self.keys.contains("KeyW"),
            s: self.keys.contains("KeyS"),
            a: self.keys.contains("KeyA"),
            d: self.keys.contains("KeyD"),
            ctrl: self.ctrl_held(),
        }
    }

    pub fn resize(&mut self) {
        let (w, h) = ui::viewport();
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0);
        self.gpu.resize(w, h, dpr);
        self.apply_hud_layout();
    }

    pub fn toast(&mut self, html: &str, color: &str, ms: f64) {
        let t = &self.dom.toast;
        t.set_inner_html(html);
        let _ = t.style().set_property("color", color);
        let _ = t.style().set_property("opacity", "1");
        self.toast_until = now() + ms;
    }

    // ---------- モード ----------

    pub fn respawn(&mut self) {
        let spawn = Vec3::new(0.0, self.world.home_top(), 4.0);
        self.heli.respawn(spawn, 0.0);
        self.crash_timer = 0.0;
        self.scene.n(self.hn.root).visible = true;
        self.start_mode();
    }

    fn start_mode(&mut self) {
        self.mission.start(self.mode, self.heli.pos);
        let m = self.mode;
        self.scene.n(self.wn.ring_group).visible = m == Mode::Rings;
        self.scene.n(self.wn.hover_zone).visible = m == Mode::Hover;
        for &pm in &self.wn.pad_mats {
            self.scene.mats[pm].color = hex(0xffffff);
        }
        match m {
            Mode::Rings => self.highlight_rings(),
            Mode::Hover => {
                self.scene.n(self.wn.hover_zone).pos = self.mission.hover_center;
                self.scene.mats[self.wn.hover_mat].color = hex(0x4fc3ff);
            }
            Mode::Landing => self.highlight_pad(),
            Mode::Free => {}
        }
        self.scene.n(self.wn.marker).visible = m != Mode::Free;
    }

    fn highlight_rings(&mut self) {
        let idx = self.mission.idx;
        for (i, &m) in self.wn.ring_mats.iter().enumerate() {
            let c = if i < idx {
                0x44aa55
            } else if i == idx {
                0xff7a2f
            } else if i == idx + 1 {
                0xffd27a
            } else {
                0x888888
            };
            self.scene.mats[m].color = hex(c);
        }
    }

    fn highlight_pad(&mut self) {
        let target = self.mission.target_pad();
        for (i, &m) in self.wn.pad_mats.iter().enumerate() {
            self.scene.mats[m].color = hex(if Some(i) == target {
                0xff7a2f
            } else {
                0xffffff
            });
        }
    }

    fn outcome(&mut self, o: Outcome) {
        match o {
            Outcome::Landed { soft, done, total } => {
                let s = if soft {
                    "ソフトランディング!"
                } else {
                    "着陸OK"
                };
                self.toast(&format!("{s} ({done}/{total})"), "var(--good)", 1800.0);
                self.highlight_pad();
            }
            Outcome::Finished { key, label, time } => {
                let prev: Option<f32> = store::get(key);
                let best = prev.is_none_or(|p| time < p);
                if best {
                    store::set(key, &time);
                }
                let tail = match prev {
                    Some(p) if !best => format!(" (ベスト {})", fmt_time(p)),
                    _ => " — ベスト更新!".into(),
                };
                self.toast(
                    &format!("{label}<br>{}{tail}", fmt_time(time)),
                    "var(--good)",
                    4000.0,
                );
                self.refresh_best();
                self.scene.n(self.wn.marker).visible = false;
            }
        }
    }

    fn update_mode(&mut self, dt: f32) {
        let before = self.mission.idx;
        let out = self.mission.update(dt, &self.heli, &self.world);
        if self.mode == Mode::Rings && self.mission.idx != before && !self.mission.done {
            self.highlight_rings();
        }
        if self.mode == Mode::Hover && !self.heli.crashed {
            let c = match self.mission.hover {
                HoverState::Outside => 0x4fc3ff,
                HoverState::Moving => 0xffc04f,
                HoverState::Still => 0x5fbf6a,
            };
            self.scene.mats[self.wn.hover_mat].color = hex(c);
        }
        if let Some(p) = self.mission.marker(&self.world) {
            self.scene.n(self.wn.marker).pos = p;
        }
        if let Some(o) = out {
            self.outcome(o);
        }
        if self.mission.msg != self.last_msg {
            self.dom.mission_text.set_inner_html(&self.mission.msg);
            self.last_msg = self.mission.msg.clone();
        }
    }

    fn crash(&mut self, reason: &str) {
        self.crash_timer = 0.7;
        self.toast(&format!("CRASH! {reason}"), "var(--bad)", 1800.0);
        // 破片を飛ばす
        let p = self.heli.pos + Vec3::Y;
        for i in 0..DEBRIS_PER_CRASH {
            let node = self.wn.debris[(self.debris_next + i) % self.wn.debris.len()];
            self.debris.retain(|d| d.node != node);
            let r = || js_sys::Math::random() as f32;
            let v = Vec3::new((r() - 0.5) * 14.0, r() * 12.0, (r() - 0.5) * 14.0);
            self.scene.n(node).pos = p;
            self.scene.n(node).rot = Quat::IDENTITY;
            self.scene.n(node).visible = true;
            self.debris.push(Debris {
                node,
                v,
                t: 2.0,
                spin: 0.0,
            });
        }
        self.debris_next = (self.debris_next + DEBRIS_PER_CRASH) % self.wn.debris.len();
        self.scene.n(self.hn.root).visible = false;
    }

    // ---------- カメラ ----------

    fn update_camera(&mut self, dt: f32) {
        let vis_q = self.heli.vis_q();
        let cc = self.cam_cfg[self.cam_mode];
        let tilt_q = Quat::from_rotation_x(-cc.tilt / 57.3);
        let front_z = self.scene.nodes[self.hn.front].pos.z;
        let crashed = self.heli.crashed;
        let hn = &self.hn;
        // 座席まわりの視点: 機体に固定した位置 (コックピットのポッドの前後位置を足す)
        let seat = vis_q * Vec3::new(cc.side, cc.height, cc.dist + front_z);
        if self.cam_mode == 2 && !crashed {
            self.eye = seat + self.heli.pos;
            self.eye_q = vis_q * tilt_q;
            // 目が胴体の中にあるときだけ機体を隠す
            let inside = cc.side.abs() < 0.75
                && cc.dist > -1.3
                && cc.dist < 1.2
                && cc.height > 0.3
                && cc.height < 1.6;
            self.scene.n(hn.root).visible = !inside;
            self.scene.n(hn.cabin).visible = false;
            for id in [hn.hull, hn.canopy, hn.mast, hn.engine] {
                self.scene.n(id).visible = true;
            }
            return;
        }
        if !crashed {
            self.scene.n(hn.root).visible = true;
        }
        let in_cabin = (self.cam_mode == 5 || self.cam_mode == 6) && !crashed;
        self.scene.n(hn.cabin).visible = in_cabin;
        for id in [hn.hull, hn.canopy, hn.mast, hn.engine] {
            self.scene.n(id).visible = !in_cabin;
        }
        let pilot_view = self.cam_mode == 6; // パイロット視点: 自分がパイロット
        self.scene.n(hn.torso).visible = !pilot_view;
        self.scene.n(hn.head).visible = !pilot_view;
        if in_cabin {
            for &m in &hn.blade_mats {
                self.scene.mats[m].transparent = true;
                self.scene.mats[m].opacity = 0.15;
            }
            self.eye = seat + self.heli.pos;
            // 頭は機体に少し遅れてついていく
            self.cam_q = self.cam_q.slerp(vis_q * tilt_q, 1.0 - (-dt * 9.0).exp());
            self.eye_q = self.cam_q;
            self.cam_pos = self.eye;
            return;
        }
        self.cam_q = vis_q;
        let blade_op = if self.cam_mode == 4 { 0.22 } else { 1.0 };
        for &m in &hn.blade_mats {
            self.scene.mats[m].transparent = blade_op < 1.0;
            self.scene.mats[m].opacity = blade_op;
        }
        let mut fwd = self.heli.q * Vec3::NEG_Z;
        fwd.y = 0.0;
        if fwd.length_squared() < 1e-4 {
            fwd = Vec3::NEG_Z;
        }
        let fwd = fwd.normalize();
        let (cfg, fc) = match FOLLOW[self.cam_mode] {
            Some(f) => (f, cc),
            None => (FOLLOW[1].unwrap(), self.cam_cfg[1]),
        };
        let right = Vec3::new(-fwd.z, 0.0, fwd.x);
        let pos = self.heli.pos;
        let mut desired = pos - fwd * fc.dist + right * fc.side + Vec3::new(0.0, fc.height, 0.0);
        let gh = self.world.ground_at(desired.x, desired.z, 0.0).h + 1.0;
        if desired.y < gh {
            desired.y = gh;
        }
        self.cam_pos = self.cam_pos.lerp(desired, 1.0 - (-dt * cfg[2]).exp());
        self.eye = self.cam_pos;
        let mut look = pos + fwd * cfg[0] + right * fc.side;
        look.y += cfg[1];
        self.eye_q = look_at_camera(self.eye, look);
        if FOLLOW[self.cam_mode].is_some() {
            self.eye_q *= tilt_q;
        }
    }

    fn update_gauges(&mut self, dt: f32) {
        if !self.scene.nodes[self.hn.cabin].visible {
            return;
        }
        let h = &self.heli;
        let fwd = h.q * Vec3::NEG_Z;
        let hdg = (fwd.x.atan2(-fwd.z) * 57.3 + 360.0) % 360.0;
        let rpm = (h.rotor_rate() / 1.15).min(1.0);
        let deg = [
            -135.0 + (h.vel.length() * 3.6 / 200.0).clamp(0.0, 1.0) * 270.0,
            (h.pos.y / 100.0) * 360.0,
            -90.0 + h.vel.y.clamp(-10.0, 10.0) * 15.0,
            -135.0 + rpm * 270.0,
            hdg,
        ];
        // 針には少し機械的な遅れがある
        for (i, target) in deg.into_iter().enumerate() {
            let mut d = -target / 57.3 - self.needle[i];
            if i == 4 {
                d = d.sin().atan2(d.cos()); // コンパスは近い方へ回る
            }
            self.needle[i] += d * (dt * 10.0).min(1.0);
            self.scene.n(self.hn.needles[i]).rot = Quat::from_rotation_z(self.needle[i]);
        }
    }

    fn update_hud(&mut self, t: f64) {
        let h = &self.heli;
        let (pitch, roll) = physics::attitude(h.vis_q());
        // JS の toFixed と同じく -0 は 0 と出す (+0.0 で -0.0 が 0.0 になる)
        let (pitch, roll) = (pitch + 0.0, roll + 0.0);
        let agl = h.pos.y - self.world.ground_at(h.pos.x, h.pos.z, 1.0).h + 0.0;
        let d = &self.dom;
        let ceil = self.opts.ceil;
        d.s_alt.set_text_content(Some(&format!("{agl:.1} m")));
        d.s_spd
            .set_text_content(Some(&format!("{:.0} km/h", h.vel.length() * 3.6)));
        let lim = if ceil > 0.0 {
            format!("{ceil} m")
        } else {
            "—".into()
        };
        d.s_ceil
            .set_text_content(Some(&format!("{:.0} / {lim}", h.pos.y)));
        d.s_cam.set_text_content(Some(CAMS[self.cam_mode]));
        let warn = ceil > 0.0 && h.pos.y > ceil - 15.0;
        let _ = d
            .ceil_warn
            .style()
            .set_property("display", if warn { "block" } else { "none" });
        let vs = h.vel.y + 0.0;
        d.s_vs.set_text_content(Some(&format!(
            "{}{vs:.1} m/s",
            if vs >= 0.0 { "+" } else { "" }
        )));
        d.s_att
            .set_text_content(Some(&format!("{pitch:.0}° / {roll:.0}°")));
        d.s_thr
            .set_text_content(Some(&format!("{}%", (h.throttle * 100.0).round())));
        let _ = d
            .thr_fill
            .style()
            .set_property("width", &format!("{}%", h.throttle * 100.0));
        for (k, el) in [
            ("KeyW", &d.k_w),
            ("KeyA", &d.k_a),
            ("KeyS", &d.k_s),
            ("KeyD", &d.k_d),
        ] {
            let _ = el
                .class_list()
                .toggle_with_force("on", self.keys.contains(k));
        }
        let _ = d
            .k_ctrl
            .class_list()
            .toggle_with_force("on", self.ctrl_held());
        self.pad
            .draw(&d.pad_ctx, d.pad_size, t, self.opts.sens as f64);
        hud::draw_attitude(&d.att_ctx, d.att_size, pitch as f64, roll as f64);
    }

    // ---------- フレーム ----------

    pub fn frame(&mut self, t: f64) {
        let dt = ((t - self.last_time) / 1000.0).clamp(0.0, 0.05) as f32;
        self.last_time = t;
        if self.running {
            let tune = Tuning {
                sens: self.opts.sens,
                invert: self.opts.invert,
                ceil: self.opts.ceil,
            };
            let inp = self.input();
            for _ in 0..4 {
                match self
                    .heli
                    .step(dt / 4.0, inp, &mut self.mouse, &tune, &self.world)
                {
                    Some(Event::Crash(r)) => self.crash(r),
                    Some(Event::Touchdown { solid, impact }) => {
                        if let Some(o) = self.mission.touchdown(solid, impact, &self.world) {
                            self.outcome(o);
                        }
                    }
                    None => {}
                }
            }
            self.update_mode(dt);
            if self.heli.crashed {
                self.crash_timer -= dt;
                if self.crash_timer <= 0.0 {
                    self.respawn();
                }
            }
        } else {
            self.mouse = (0.0, 0.0);
        }
        let sc = &mut self.scene;
        self.debris.retain_mut(|d| {
            d.v.y -= G * dt;
            let n = &mut sc.nodes[d.node];
            n.pos += d.v * dt;
            d.spin += dt * 5.0;
            n.rot = Quat::from_rotation_x(d.spin);
            d.t -= dt;
            if d.t <= 0.0 {
                n.visible = false;
            }
            d.t > 0.0
        });
        let h = &self.heli;
        self.scene.n(self.hn.root).pos = h.pos;
        self.scene.n(self.hn.root).rot = h.vis_q();
        self.rotor_angle += dt * 40.0 * h.rotor_spin * (0.3 + h.throttle);
        self.tail_angle += dt * 60.0 * h.rotor_spin;
        self.marker_angle += dt * 2.0;
        self.scene.n(self.hn.rotor).rot = Quat::from_rotation_y(self.rotor_angle);
        self.scene.n(self.hn.tail_rotor).rot = Quat::from_rotation_x(self.tail_angle);
        self.scene.n(self.wn.marker).rot =
            Quat::from_rotation_x(std::f32::consts::PI) * Quat::from_rotation_y(self.marker_angle);
        self.update_camera(dt);
        self.update_hud(t);
        self.update_gauges(dt);
        self.update_audio();
        if self.toast_until > 0.0 && now() > self.toast_until {
            let _ = self.dom.toast.style().set_property("opacity", "0");
            self.toast_until = 0.0;
        }
        self.render();
    }

    fn update_audio(&self) {
        if let Some(a) = &self.audio {
            a.update(&Drive {
                on: self.opts.sound && self.running && !self.heli.crashed,
                vol: self.opts.vol,
                spin: self.heli.rotor_spin,
                throttle: self.heli.throttle,
                speed: self.heli.vel.length(),
            });
        }
    }

    fn render(&mut self) {
        self.scene.update();
        let (w, h) = ui::viewport();
        let mut proj = proj_gl::perspective(70f32.to_radians(), (w / h) as f32, 0.1, 2000.0);
        // サイドメニューが開いている間は、右の空いている所の真ん中に機体が来るように画面をずらす
        let menu_w = if self.menu_open {
            self.dom.menu.offset_width() as f64
        } else {
            0.0
        };
        if menu_w > 0.0 && w - menu_w > 200.0 {
            proj.z_axis.x -= (menu_w / w) as f32;
        }
        let view = Mat4::from_rotation_translation(self.eye_q, self.eye).inverse();
        // 太陽は機体についてくる (影の範囲を機体のまわりに保つ)
        let target = self.heli.pos;
        let sun_pos = target + Vec3::new(120.0, 200.0, 80.0);
        let light_view = view_rh::look_at_mat4(sun_pos, target, Vec3::Y);
        let light_proj = proj_gl::orthographic(-80.0, 80.0, -80.0, 80.0, 1.0, 500.0);
        let lights = Lights {
            sun_dir: (sun_pos - target).normalize(),
            sun_color: hex(0xfff2dd) * 0.9,
            sky: hex(0xdfeaff) * 0.75,
            ground: hex(0x5a5040) * 0.75,
            fog_color: hex(BG),
            fog_near: 150.0,
            fog_far: 900.0,
            light_vp: light_proj * light_view,
        };
        let items = self.scene.draw_items();
        self.gpu
            .render(&items, &self.scene.mats, view, proj, &lights);
    }

    // ---------- メニュー ----------

    pub fn init_audio(&mut self) {
        match &self.audio {
            Some(a) => a.resume(),
            None => match Rotor::new() {
                Ok(a) => self.audio = Some(a),
                Err(e) => web_sys::console::warn_2(&"audio init failed".into(), &e),
            },
        }
    }

    pub fn silence(&self) {
        if let Some(a) = &self.audio {
            a.silence();
        }
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.set_menu(true);
        self.dom.start.set_text_content(Some("再開 (クリック)"));
        self.keys.clear();
        self.silence();
        ui::exit_pointer_lock(&self.dom.canvas);
        ui::leave_fullscreen(); // メニューが全画面のまま残らないようにする
    }

    pub fn start(&mut self) {
        if self.opts.sound {
            self.init_audio();
        }
        self.set_menu(false);
        self.running = true;
        if self.last_mode != Some(self.mode) {
            self.last_mode = Some(self.mode);
            self.respawn();
        }
        if self.opts.fs {
            ui::enter_fullscreen_and_lock_keys();
        }
        self.dom.canvas.request_pointer_lock();
    }

    pub fn set_menu(&mut self, open: bool) {
        self.menu_open = open;
        let _ = self
            .dom
            .menu
            .style()
            .set_property("display", if open { "flex" } else { "none" });
    }

    pub fn cycle_camera(&mut self) {
        let i = CAM_ORDER
            .iter()
            .position(|&c| c == self.cam_mode)
            .unwrap_or(0);
        self.cam_mode = CAM_ORDER[(i + 1) % CAM_ORDER.len()];
        if self.running {
            self.toast(
                &format!("視点: {}", CAMS[self.cam_mode]),
                "var(--fg)",
                900.0,
            );
        }
    }
}
