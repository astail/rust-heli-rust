//! DOM まわり: 要素の取得、イベントの登録、設定パネル、HUD の配置・表示。

use crate::app::{App, CAM_DEF, CAM_RANGE, CAMS, HudBox};
use crate::models;
use crate::modes::{Mode, fmt_time};
use crate::store;
use js_sys::{Array, Function, Promise, Reflect};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{
    AddEventListenerOptions, BeforeUnloadEvent, CanvasRenderingContext2d, Document, EventTarget,
    HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlSelectElement, KeyboardEvent, MouseEvent,
    PointerEvent, WheelEvent, Window,
};

pub type AppRc = Rc<RefCell<App>>;

pub const HUD_IDS: [&str; 4] = ["stats", "attWrap", "keys", "mission"];

fn window() -> Window {
    web_sys::window().expect("window")
}

fn document() -> Document {
    window().document().expect("document")
}

pub fn el(id: &str) -> Result<HtmlElement, JsValue> {
    Ok(document()
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("#{id} がありません")))?
        .dyn_into()?)
}

fn input(id: &str) -> Result<HtmlInputElement, JsValue> {
    Ok(el(id)?.dyn_into()?)
}

fn select(id: &str) -> Result<HtmlSelectElement, JsValue> {
    Ok(el(id)?.dyn_into()?)
}

fn ctx2d(c: &HtmlCanvasElement) -> Result<CanvasRenderingContext2d, JsValue> {
    Ok(c.get_context("2d")?.ok_or("2d")?.dyn_into()?)
}

fn query_all(sel: &str) -> Vec<HtmlElement> {
    let Ok(list) = document().query_selector_all(sel) else {
        return Vec::new();
    };
    (0..list.length())
        .filter_map(|i| list.get(i)?.dyn_into().ok())
        .collect()
}

pub fn viewport() -> (f64, f64) {
    let w = window();
    let f = |v: Result<JsValue, JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(1.0).max(1.0);
    (f(w.inner_width()), f(w.inner_height()))
}

fn set_style(e: &HtmlElement, k: &str, v: &str) {
    let _ = e.style().set_property(k, v);
}

/// 毎フレーム書き換える要素はまとめて持っておく
pub struct Dom {
    pub canvas: HtmlCanvasElement,
    pub menu: HtmlElement,
    pub start: HtmlElement,
    pub toast: HtmlElement,
    pub mission_text: HtmlElement,
    pub s_alt: HtmlElement,
    pub s_ceil: HtmlElement,
    pub s_cam: HtmlElement,
    pub s_spd: HtmlElement,
    pub s_vs: HtmlElement,
    pub s_att: HtmlElement,
    pub s_thr: HtmlElement,
    pub thr_fill: HtmlElement,
    pub ceil_warn: HtmlElement,
    pub k_w: HtmlElement,
    pub k_a: HtmlElement,
    pub k_s: HtmlElement,
    pub k_d: HtmlElement,
    pub k_ctrl: HtmlElement,
    pub att_ctx: CanvasRenderingContext2d,
    pub att_size: f64,
    pub pad_ctx: CanvasRenderingContext2d,
    pub pad_size: f64,
}

impl Dom {
    pub fn new() -> Result<Dom, JsValue> {
        let att: HtmlCanvasElement = el("att")?.dyn_into()?;
        let pad: HtmlCanvasElement = el("mpad")?.dyn_into()?;
        Ok(Dom {
            canvas: el("gl")?.dyn_into()?,
            menu: el("menu")?,
            start: el("start")?,
            toast: el("toast")?,
            mission_text: el("missionText")?,
            s_alt: el("sAlt")?,
            s_ceil: el("sCeil")?,
            s_cam: el("sCam")?,
            s_spd: el("sSpd")?,
            s_vs: el("sVs")?,
            s_att: el("sAtt")?,
            s_thr: el("sThr")?,
            thr_fill: el("thrfill")?,
            ceil_warn: el("ceilWarn")?,
            k_w: el("kW")?,
            k_a: el("kA")?,
            k_s: el("kS")?,
            k_d: el("kD")?,
            k_ctrl: el("kCtrl")?,
            att_ctx: ctx2d(&att)?,
            att_size: att.width() as f64,
            pad_ctx: ctx2d(&pad)?,
            pad_size: pad.width() as f64,
        })
    }
}

// ---------- ポインタロック・全画面 ----------

pub fn pointer_locked(canvas: &HtmlCanvasElement) -> bool {
    document()
        .pointer_lock_element()
        .is_some_and(|e| e.is_same_node(Some(canvas)))
}

pub fn exit_pointer_lock(canvas: &HtmlCanvasElement) {
    if pointer_locked(canvas) {
        document().exit_pointer_lock();
    }
}

pub fn leave_fullscreen() {
    let d = document();
    if d.fullscreen_element().is_some() {
        d.exit_fullscreen();
    }
}

fn keyboard() -> Option<JsValue> {
    Reflect::get(&window().navigator(), &"keyboard".into())
        .ok()
        .filter(|k| !k.is_undefined() && !k.is_null())
}

fn call_method(obj: &JsValue, name: &str, args: &Array) -> Option<JsValue> {
    let f: Function = Reflect::get(obj, &name.into()).ok()?.dyn_into().ok()?;
    f.apply(obj, args).ok()
}

/// 失敗しても何もしない (未処理の reject を出さない)
fn ignore_rejection(p: JsValue) {
    if let Ok(p) = p.dyn_into::<Promise>() {
        let noop: Closure<dyn FnMut(JsValue)> = Closure::new(|_| {});
        let _ = p.catch(&noop);
        noop.forget();
    }
}

/// Chromium だけ: Ctrl+W / Ctrl+T / Ctrl+N / Ctrl+R をブラウザでなくページが受け取れるようにする
fn lock_keys() {
    if let Some(kb) = keyboard() {
        let codes = Array::of5(
            &"KeyW".into(),
            &"KeyT".into(),
            &"KeyN".into(),
            &"KeyR".into(),
            &"KeyQ".into(),
        );
        if let Some(p) = call_method(&kb, "lock", &Array::of1(&codes)) {
            ignore_rejection(p);
        }
    }
}

fn unlock_keys() {
    if let Some(kb) = keyboard() {
        call_method(&kb, "unlock", &Array::new());
    }
}

pub fn enter_fullscreen_and_lock_keys() {
    let d = document();
    if d.fullscreen_element().is_some() {
        return;
    }
    let Some(root) = d.document_element() else {
        return;
    };
    let Some(p) = call_method(&root, "requestFullscreen", &Array::new()) else {
        return;
    };
    let Ok(p) = p.dyn_into::<Promise>() else {
        return;
    };
    let then: Closure<dyn FnMut(JsValue)> = Closure::once(|_| lock_keys());
    let catch: Closure<dyn FnMut(JsValue)> = Closure::once(|_| {});
    let _ = p.then2(&then, &catch);
    then.forget();
    catch.forget();
}

// ---------- イベント ----------

fn listen_with<E: JsCast + 'static>(
    target: &EventTarget,
    ty: &str,
    passive: Option<bool>,
    app: &AppRc,
    mut f: impl FnMut(&mut App, E) + 'static,
) {
    let app = app.clone();
    let cb: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |e: web_sys::Event| {
        // 処理中に同期で別のイベントが来たら (confirm など) 飛ばす
        if let Ok(mut a) = app.try_borrow_mut() {
            f(&mut a, e.unchecked_into());
        }
    });
    let r = match passive {
        Some(p) => {
            let o = AddEventListenerOptions::new();
            o.set_passive(p);
            target.add_event_listener_with_callback_and_add_event_listener_options(
                ty,
                cb.as_ref().unchecked_ref(),
                &o,
            )
        }
        None => target.add_event_listener_with_callback(ty, cb.as_ref().unchecked_ref()),
    };
    if r.is_err() {
        web_sys::console::warn_1(&format!("addEventListener({ty}) failed").into());
    }
    cb.forget();
}

fn listen<E: JsCast + 'static>(
    target: &EventTarget,
    ty: &str,
    app: &AppRc,
    f: impl FnMut(&mut App, E) + 'static,
) {
    listen_with(target, ty, None, app, f);
}

fn on_input(
    id: &str,
    app: &AppRc,
    mut f: impl FnMut(&mut App, f32) + 'static,
) -> Result<(), JsValue> {
    let i = input(id)?;
    let i2 = i.clone();
    listen::<web_sys::Event>(&i, "input", app, move |a, _| {
        f(a, i2.value().parse().unwrap_or(0.0))
    });
    Ok(())
}

fn on_check(
    id: &str,
    app: &AppRc,
    mut f: impl FnMut(&mut App, bool) + 'static,
) -> Result<(), JsValue> {
    let i = input(id)?;
    let i2 = i.clone();
    listen::<web_sys::Event>(&i, "change", app, move |a, _| f(a, i2.checked()));
    Ok(())
}

fn on_click(id: &str, app: &AppRc, mut f: impl FnMut(&mut App) + 'static) -> Result<(), JsValue> {
    listen::<web_sys::Event>(&el(id)?.into(), "click", app, move |a, _| f(a));
    Ok(())
}

fn fmt_m(v: f32) -> String {
    format!("{v:.2} m")
}

fn fmt_side(v: f32) -> String {
    let dir = if v > 0.0 {
        "右 "
    } else if v < 0.0 {
        "左 "
    } else {
        ""
    };
    format!("{dir}{:.2}", v.abs())
}

fn fmt_deg(v: f32) -> String {
    format!("{v:.0}°")
}

/// カメラのスライダー: (要素 ID, 書式)
/// (要素 ID, 表示の書式)
type CamSlider = (&'static str, fn(f32) -> String);
const CAM_UI: [CamSlider; 4] = [
    ("cDist", fmt_m),
    ("cHeight", fmt_m),
    ("cSide", fmt_side),
    ("cTilt", fmt_deg),
];

fn cam_field(c: &mut crate::app::CamCfg, i: usize) -> &mut f32 {
    match i {
        0 => &mut c.dist,
        1 => &mut c.height,
        2 => &mut c.side,
        _ => &mut c.tilt,
    }
}

fn save_cams(a: &App) {
    let m: std::collections::BTreeMap<String, crate::app::CamCfg> = a
        .cam_cfg
        .iter()
        .enumerate()
        .map(|(i, c)| (i.to_string(), *c))
        .collect();
    store::set("camcfg", &m);
}

/// HUD のドラッグ中の状態
pub struct Drag {
    id: &'static str,
    resize: bool,
    w0: f64,
    h0: f64,
    sx: f64,
    sy: f64,
    s0: f64,
    x0: f64,
    y0: f64,
}

impl App {
    fn set_text(&self, id: &str, s: &str) {
        if let Ok(e) = el(id) {
            e.set_text_content(Some(s));
        }
    }

    pub fn sync_cam_ui(&mut self) {
        let e = self.cam_edit;
        let (c, r) = (self.cam_cfg[e], CAM_RANGE[e]);
        if let Ok(s) = select("optCam") {
            s.set_value(&e.to_string());
        }
        for (id, m) in [("cDist", r[0]), ("cHeight", r[1]), ("cSide", r[2])] {
            if let Ok(i) = input(id) {
                i.set_min(&(-m).to_string());
                i.set_max(&m.to_string());
                i.set_step(if m > 10.0 { "0.1" } else { "0.05" });
            }
        }
        let mut c = c;
        for (k, (id, f)) in CAM_UI.iter().enumerate() {
            let v = *cam_field(&mut c, k);
            if let Ok(i) = input(id) {
                i.set_value(&v.to_string());
            }
            self.set_text(&format!("{id}V"), &f(v));
        }
    }

    pub fn apply_pivot(&mut self) {
        models::apply_pivot(&mut self.scene, &self.hn, self.opts.pivot);
        if let Ok(i) = input("optPivot") {
            i.set_value(&self.opts.pivot.to_string());
        }
        self.set_text("optPivotV", &format!("{:.1} m", self.opts.pivot));
    }

    /// 計器盤の箱の不透明度 (0 = 箱は消えて計器だけ残る)
    pub fn apply_dash(&mut self) {
        let d = self.opts.dash;
        let m = &mut self.scene.mats[self.hn.dash_mat];
        m.opacity = d;
        m.depth_write = d >= 1.0;
        self.scene.n(self.hn.dash_box).visible = d > 0.0;
        if let Ok(i) = input("optDash") {
            i.set_value(&d.to_string());
        }
        self.set_text("optDashV", &format!("{}%", (d * 100.0).round()));
    }

    pub fn refresh_best(&self) {
        for (key, id) in [
            ("best_rings", "bestRings"),
            ("best_landing", "bestLanding"),
            ("best_hover", "bestHover"),
        ] {
            let t: Option<f32> = store::get(key);
            self.set_text(
                id,
                &t.map(|t| format!("ベスト {}", fmt_time(t)))
                    .unwrap_or_default(),
            );
        }
    }

    fn vis(&self, k: &str) -> bool {
        self.hud_vis.get(k).copied().unwrap_or(true)
    }

    pub fn apply_hud_vis(&mut self) {
        let all = self.vis("all");
        for id in HUD_IDS {
            if let Ok(e) = el(id) {
                let _ = e
                    .class_list()
                    .toggle_with_force("hudoff", !all || !self.vis(id));
            }
        }
        if let Ok(i) = input("hvAll") {
            i.set_checked(all);
        }
        for cb in query_all("[data-hud]") {
            if let (Some(k), Ok(i)) = (
                cb.get_attribute("data-hud"),
                cb.dyn_into::<HtmlInputElement>(),
            ) {
                i.set_checked(self.vis(&k));
            }
        }
        for row in query_all(".hvrow") {
            let _ = row.class_list().toggle_with_force("dim", !all);
        }
        store::set("hudvis", &self.hud_vis);
    }

    pub fn apply_hud_layout(&self) {
        let (w, h) = viewport();
        for id in HUD_IDS {
            let Ok(e) = el(id) else { continue };
            let Some(l) = self.hud_layout.get(id) else {
                e.style().set_css_text("");
                continue;
            };
            set_style(&e, "left", &format!("{}px", (l.x * w).round()));
            set_style(&e, "top", &format!("{}px", (l.y * h).round()));
            set_style(&e, "right", "auto");
            set_style(&e, "bottom", "auto");
            set_style(&e, "transform", &format!("scale({})", l.s));
            set_style(&e, "transform-origin", "top left");
            // ウィンドウを小さくしても画面内に収める
            let r = e.get_bounding_client_rect();
            if r.right() > w {
                set_style(&e, "left", &format!("{}px", (w - r.width()).max(0.0)));
            }
            if r.bottom() > h {
                set_style(&e, "top", &format!("{}px", (h - r.height()).max(0.0)));
            }
        }
    }

    /// いま画面に出ている位置を、明示的な配置として固定する
    fn capture_hud(&mut self, id: &str) {
        let Ok(e) = el(id) else { return };
        let (w, h) = viewport();
        let r = e.get_bounding_client_rect();
        let s = self.hud_layout.get(id).map(|l| l.s).unwrap_or(1.0);
        self.hud_layout.insert(
            id.to_string(),
            HudBox {
                x: r.left() / w,
                y: r.top() / h,
                s,
            },
        );
        self.apply_hud_layout();
    }

    pub fn hud_done(&mut self) {
        let _ = document()
            .body()
            .map(|b| b.class_list().remove_1("hudedit"));
        self.hud_edit = false;
        self.set_menu(true);
    }

    fn init_ui(&mut self) -> Result<(), JsValue> {
        let o = &self.opts;
        input("optSens")?.set_value(&o.sens.to_string());
        self.set_text("optSensV", &format!("{:.2}", o.sens));
        select("optCeil")?.set_value(&o.ceil.to_string());
        input("optInvert")?.set_checked(o.invert);
        input("optFs")?.set_checked(o.fs);
        input("optSound")?.set_checked(o.sound);
        input("optVol")?.set_value(&o.vol.to_string());
        self.sync_cam_ui();
        self.apply_pivot();
        self.apply_dash();
        self.apply_hud_vis();
        self.apply_hud_layout();
        self.refresh_best();
        self.dom
            .start
            .set_text_content(Some("クリックしてスタート"));
        self.dom.start.remove_attribute("disabled")?;
        Ok(())
    }
}

pub fn bind(app: &AppRc) -> Result<(), JsValue> {
    app.borrow_mut().init_ui()?;
    let win: EventTarget = window().into();
    let doc: EventTarget = document().into();
    let canvas: EventTarget = app.borrow().dom.canvas.clone().into();

    // --- 設定 ---
    on_input("optSens", app, |a, v| {
        a.opts.sens = v;
        a.set_text("optSensV", &format!("{v:.2}"));
        store::set("sens2", &v);
    })?;
    let ceil = select("optCeil")?;
    let ceil2 = ceil.clone();
    listen::<web_sys::Event>(&ceil, "change", app, move |a, _| {
        a.opts.ceil = ceil2.value().parse().unwrap_or(0.0);
        store::set("ceil", &a.opts.ceil);
    });
    let cam = select("optCam")?;
    let cam2 = cam.clone();
    listen::<web_sys::Event>(&cam, "change", app, move |a, _| {
        let c: usize = cam2.value().parse().unwrap_or(6);
        a.opts.cam = c;
        store::set("cam3", &c);
        a.cam_mode = c;
        a.cam_edit = c;
        a.sync_cam_ui();
    });
    on_check("optInvert", app, |a, v| {
        a.opts.invert = v;
        store::set("invert", &v);
    })?;
    on_check("optFs", app, |a, v| {
        a.opts.fs = v;
        store::set("fs", &v);
        if !v {
            leave_fullscreen();
        }
    })?;
    on_input("optPivot", app, |a, v| {
        a.opts.pivot = v;
        store::set("pivot", &v);
        a.apply_pivot();
    })?;
    on_input("optDash", app, |a, v| {
        a.opts.dash = v;
        store::set("dashop", &v);
        a.apply_dash();
    })?;
    on_check("optSound", app, |a, v| {
        a.opts.sound = v;
        store::set("sound", &v);
        if v {
            a.init_audio();
        }
    })?;
    on_input("optVol", app, |a, v| {
        a.opts.vol = v;
        store::set("vol2", &v);
    })?;
    for (k, (id, f)) in CAM_UI.iter().enumerate() {
        let (id, f) = (*id, *f);
        on_input(id, app, move |a, v| {
            let e = a.cam_edit;
            *cam_field(&mut a.cam_cfg[e], k) = v;
            a.set_text(&format!("{id}V"), &f(v));
            save_cams(a);
        })?;
    }
    on_click("camReset", app, |a| {
        a.cam_cfg[a.cam_edit] = CAM_DEF[a.cam_edit];
        save_cams(a);
        a.sync_cam_ui();
    })?;
    // 飛行中はホイールで今の視点の距離を変える
    listen_with::<WheelEvent>(&win, "wheel", Some(true), app, |a, e| {
        if !a.running {
            return;
        }
        let (cm, m) = (a.cam_mode, CAM_RANGE[a.cam_mode][0]);
        let c = &mut a.cam_cfg[cm];
        c.dist = (c.dist + (e.delta_y().signum() as f32) * m / 25.0).clamp(-m, m);
        let dist = c.dist;
        save_cams(a);
        a.toast(
            &format!("{} 距離 {dist:.2} m", CAMS[cm]),
            "var(--fg)",
            600.0,
        );
    });

    // --- モード選択 ---
    let modes = query_all(".mode");
    for b in &modes {
        let (all, me) = (modes.clone(), b.clone());
        listen::<web_sys::Event>(b, "click", app, move |a, _| {
            for x in &all {
                let _ = x.class_list().remove_1("sel");
            }
            let _ = me.class_list().add_1("sel");
            a.mode = Mode::from_key(&me.get_attribute("data-mode").unwrap_or_default());
            a.dom.start.set_text_content(Some("クリックしてスタート"));
        });
    }

    // --- HUD の表示 ---
    on_check("hvAll", app, |a, v| {
        a.hud_vis.insert("all".into(), v);
        a.apply_hud_vis();
    })?;
    for cb in query_all("[data-hud]") {
        let key = cb.get_attribute("data-hud").unwrap_or_default();
        let i: HtmlInputElement = cb.dyn_into()?;
        let i2 = i.clone();
        listen::<web_sys::Event>(&i, "change", app, move |a, _| {
            a.hud_vis.insert(key.clone(), i2.checked());
            if i2.checked() {
                a.hud_vis.insert("all".into(), true);
            }
            a.apply_hud_vis();
        });
    }

    // --- HUD の配置 (移動・拡大縮小、保存される) ---
    for id in HUD_IDS {
        let e = el(id)?;
        let handle: HtmlElement = document().create_element("div")?.dyn_into()?;
        handle.set_class_name("rh");
        e.append_child(&handle)?;
        let (e1, h1) = (e.clone(), handle.clone());
        listen::<PointerEvent>(&e, "pointerdown", app, move |a, ev| {
            if !a.hud_edit {
                return;
            }
            ev.prevent_default();
            a.capture_hud(id);
            let l = a.hud_layout[id];
            let r = e1.get_bounding_client_rect();
            let (w, h) = viewport();
            let resize = ev
                .target()
                .is_some_and(|t| JsValue::from(t) == JsValue::from(h1.clone()));
            let _ = e1.set_pointer_capture(ev.pointer_id());
            a.drag = Some(Drag {
                id,
                resize,
                w0: r.width(),
                h0: r.height(),
                sx: ev.client_x() as f64,
                sy: ev.client_y() as f64,
                s0: l.s,
                x0: l.x * w,
                y0: l.y * h,
            });
        });
        listen::<PointerEvent>(&e, "pointermove", app, move |a, ev| {
            let Some(d) = a.drag.as_ref().filter(|d| d.id == id) else {
                return;
            };
            let (w, h) = viewport();
            let (cx, cy) = (ev.client_x() as f64, ev.client_y() as f64);
            let mut l = a.hud_layout[id];
            if d.resize {
                l.s = (d.s0 * (d.w0 + cx - d.sx) / d.w0).clamp(0.4, 3.0);
            } else {
                l.x = (d.x0 + cx - d.sx).clamp(0.0, (w - d.w0).max(0.0)) / w;
                l.y = (d.y0 + cy - d.sy).clamp(0.0, (h - d.h0).max(0.0)) / h;
            }
            a.hud_layout.insert(id.to_string(), l);
            a.apply_hud_layout();
        });
        listen::<PointerEvent>(&e, "pointerup", app, move |a, _| {
            if a.drag.as_ref().is_some_and(|d| d.id == id) {
                a.drag = None;
                store::set("hudlayout", &a.hud_layout);
            }
        });
        listen_with::<WheelEvent>(&e, "wheel", Some(false), app, move |a, ev| {
            if !a.hud_edit {
                return;
            }
            ev.prevent_default();
            a.capture_hud(id);
            let mut l = a.hud_layout[id];
            l.s = (l.s * if ev.delta_y() < 0.0 { 1.08 } else { 1.0 / 1.08 }).clamp(0.4, 3.0);
            a.hud_layout.insert(id.to_string(), l);
            a.apply_hud_layout();
            store::set("hudlayout", &a.hud_layout);
        });
    }
    // 配置モードの案内パネル自体もドラッグでどかせる
    let bar = el("hudbar")?;
    let grip: HtmlElement = bar.query_selector("p")?.ok_or("#hudbar p")?.dyn_into()?;
    let (bar1, bar2, grip1) = (bar.clone(), bar.clone(), grip.clone());
    listen::<PointerEvent>(&grip, "pointerdown", app, move |a, ev| {
        let r = bar1.get_bounding_client_rect();
        set_style(&bar1, "transform", "none");
        set_style(&bar1, "left", &format!("{}px", r.left()));
        set_style(&bar1, "top", &format!("{}px", r.top()));
        set_style(&bar1, "bottom", "auto");
        let _ = grip1.set_pointer_capture(ev.pointer_id());
        a.bar_drag = Some((
            ev.client_x() as f64 - r.left(),
            ev.client_y() as f64 - r.top(),
            r.width(),
            r.height(),
        ));
    });
    listen::<PointerEvent>(&grip, "pointermove", app, move |a, ev| {
        let Some((dx, dy, bw, bh)) = a.bar_drag else {
            return;
        };
        let (w, h) = viewport();
        set_style(
            &bar2,
            "left",
            &format!(
                "{}px",
                (ev.client_x() as f64 - dx).clamp(0.0, (w - bw).max(0.0))
            ),
        );
        set_style(
            &bar2,
            "top",
            &format!(
                "{}px",
                (ev.client_y() as f64 - dy).clamp(0.0, (h - bh).max(0.0))
            ),
        );
    });
    listen::<PointerEvent>(&grip, "pointerup", app, |a, _| a.bar_drag = None);

    on_click("hudEdit", app, |a| {
        a.set_menu(false);
        let _ = document().body().map(|b| b.class_list().add_1("hudedit"));
        a.hud_edit = true;
    })?;
    on_click("hudDone", app, |a| a.hud_done())?;
    on_click("hudReset", app, |a| {
        a.hud_layout.clear();
        store::set("hudlayout", &a.hud_layout);
        a.apply_hud_layout();
    })?;
    // confirm() の間にイベントが来ても困らないよう、ここでは App を借りない
    let reset: Closure<dyn FnMut()> = Closure::new(|| {
        let msg = "感度・視点・カメラ位置・HUD の配置と表示・音・その他の設定をすべて初期状態に戻します。\n(ベストタイムは残ります)\nよろしいですか？";
        if window().confirm_with_message(msg).unwrap_or(false) {
            store::reset_all();
            let _ = window().location().reload();
        }
    });
    el("resetAll")?.add_event_listener_with_callback("click", reset.as_ref().unchecked_ref())?;
    reset.forget();

    // --- キーボード・マウス ---
    listen::<KeyboardEvent>(&win, "keydown", app, |a, e| {
        let code = e.code();
        if code == "Escape" {
            if a.hud_edit {
                a.hud_done();
            } else if a.running {
                a.pause();
            } else {
                a.silence();
            }
            return;
        }
        a.keys.insert(code.clone());
        if a.running && (e.ctrl_key() || e.meta_key()) {
            e.prevent_default(); // 飛行中は Ctrl+S / Ctrl+D などをブラウザに渡さない
        }
        match code.as_str() {
            "KeyC" | "KeyV" => a.cycle_camera(),
            "KeyR" if a.running => a.respawn(),
            "KeyH" => {
                let all = !a.vis("all");
                a.hud_vis.insert("all".into(), all);
                a.apply_hud_vis();
                if a.running {
                    a.toast(if all { "HUD ON" } else { "HUD OFF" }, "var(--fg)", 700.0);
                }
            }
            _ => {}
        }
    });
    listen::<KeyboardEvent>(&win, "keyup", app, |a, e| {
        a.keys.remove(&e.code());
    });
    // 飛行中に閉じようとしたら確認する (キーボードロックが効かないときの Ctrl+W など)
    listen::<BeforeUnloadEvent>(&win, "beforeunload", app, |a, e| {
        if a.running {
            e.prevent_default();
            e.set_return_value("");
        }
    });
    listen::<web_sys::Event>(&win, "blur", app, |a, _| a.keys.clear());
    listen::<MouseEvent>(&doc, "mousemove", app, |a, e| {
        if pointer_locked(&a.dom.canvas) {
            let (dx, dy) = (e.movement_x() as f32, e.movement_y() as f32);
            a.mouse.0 += dx;
            a.mouse.1 += dy;
            a.pad.ax += dx as f64;
            a.pad.ay += dy as f64;
        }
    });
    listen::<web_sys::Event>(&doc, "pointerlockchange", app, |a, _| {
        if !pointer_locked(&a.dom.canvas) && a.running {
            a.pause();
        }
    });
    listen::<web_sys::Event>(&canvas, "click", app, |a, _| {
        if a.running && !pointer_locked(&a.dom.canvas) {
            a.dom.canvas.request_pointer_lock();
        }
    });

    // --- 一時停止・再開 ---
    listen::<web_sys::Event>(&doc, "visibilitychange", app, |a, _| {
        if document().hidden() {
            if a.running {
                a.pause();
            } else {
                a.silence();
            }
        }
    });
    on_click("start", app, |a| a.start())?;
    listen::<web_sys::Event>(&doc, "fullscreenchange", app, |a, _| {
        if document().fullscreen_element().is_none() {
            unlock_keys();
            if a.running {
                a.pause();
            }
        }
    });
    listen::<web_sys::Event>(&win, "resize", app, |a, _| a.resize());
    Ok(())
}
