//! HUD のキャンバス描画: 姿勢計と、マウスの動きを見せるパッド。

use std::collections::VecDeque;
use std::f64::consts::TAU;
use web_sys::CanvasRenderingContext2d as Ctx;

pub fn draw_attitude(x: &Ctx, w: f64, pitch: f64, roll: f64) {
    let r = w / 2.0;
    x.clear_rect(0.0, 0.0, w, w);
    x.save();
    x.begin_path();
    let _ = x.arc(r, r, r - 4.0, 0.0, TAU);
    x.clip();
    let _ = x.translate(r, r);
    let _ = x.rotate(-roll / 57.3);
    let off = pitch * 3.0;
    x.set_fill_style_str("#3c7fb8");
    x.fill_rect(-w, -w * 2.0 + off, w * 2.0, w * 2.0);
    x.set_fill_style_str("#7a5a36");
    x.fill_rect(-w, off, w * 2.0, w * 2.0);
    x.set_stroke_style_str("#fff");
    x.set_line_width(3.0);
    x.begin_path();
    x.move_to(-w, off);
    x.line_to(w, off);
    x.stroke();
    x.set_line_width(2.0);
    x.set_font("20px system-ui");
    x.set_fill_style_str("#fff");
    x.set_text_align("left");
    for p in (-60i32..=60).step_by(10) {
        if p == 0 {
            continue;
        }
        let y = off - p as f64 * 3.0;
        let lw = if p % 20 != 0 { 30.0 } else { 55.0 };
        x.begin_path();
        x.move_to(-lw, y);
        x.line_to(lw, y);
        x.stroke();
        if p % 20 == 0 {
            let _ = x.fill_text(&p.abs().to_string(), lw + 6.0, y + 7.0);
        }
    }
    x.restore();
    x.set_stroke_style_str("#ffb020");
    x.set_line_width(6.0);
    x.begin_path();
    x.move_to(r - 70.0, r);
    x.line_to(r - 25.0, r);
    x.line_to(r - 12.0, r + 14.0);
    x.move_to(r + 70.0, r);
    x.line_to(r + 25.0, r);
    x.line_to(r + 12.0, r + 14.0);
    x.stroke();
    x.begin_path();
    let _ = x.arc(r, r, 5.0, 0.0, TAU);
    x.set_fill_style_str("#ffb020");
    x.fill();
}

/// マウスの「スティック」テスター: 点がマウスの速さと向きを示し、止めると中心へ戻る
pub struct MousePad {
    pub ax: f64,
    pub ay: f64,
    vx: f64,
    vy: f64,
    last: f64,
    trail: VecDeque<(f64, f64)>,
}

impl MousePad {
    pub fn new(now: f64) -> Self {
        MousePad {
            ax: 0.0,
            ay: 0.0,
            vx: 0.0,
            vy: 0.0,
            last: now,
            trail: VecDeque::new(),
        }
    }

    pub fn draw(&mut self, x: &Ctx, size: f64, now: f64, sens: f64) {
        let dt = ((now - self.last) / 1000.0).max(1e-3);
        self.last = now;
        let k = (dt * 14.0).min(1.0);
        self.vx += (self.ax / dt - self.vx) * k;
        self.vy += (self.ay / dt - self.vy) * k;
        self.ax = 0.0;
        self.ay = 0.0;
        let (s, c) = (size, size / 2.0);
        let r = c - 12.0;
        let scale = 600.0 / sens.max(0.05); // フルに振れる px/s
        let mag = self.vx.hypot(self.vy);
        let n = (mag / scale).tanh() * r;
        let (px, py) = if mag > 0.0 {
            (c + self.vx / mag * n, c + self.vy / mag * n)
        } else {
            (c, c)
        };
        self.trail.push_back((px, py));
        if self.trail.len() > 10 {
            self.trail.pop_front();
        }
        x.clear_rect(0.0, 0.0, s, s);
        x.set_stroke_style_str("rgba(255,255,255,.18)");
        x.set_line_width(2.0);
        x.stroke_rect(6.0, 6.0, s - 12.0, s - 12.0);
        x.begin_path();
        x.move_to(c, 6.0);
        x.line_to(c, s - 6.0);
        x.move_to(6.0, c);
        x.line_to(s - 6.0, c);
        x.stroke();
        x.begin_path();
        let _ = x.arc(c, c, r * 0.5, 0.0, TAU);
        x.stroke();
        let len = self.trail.len() as f64;
        for (i, &(tx, ty)) in self.trail.iter().enumerate() {
            x.set_fill_style_str(&format!(
                "rgba(224,102,44,{})",
                (i as f64 + 1.0) / len * 0.35
            ));
            x.begin_path();
            let _ = x.arc(tx, ty, 5.0, 0.0, TAU);
            x.fill();
        }
        x.set_stroke_style_str("#e0662c");
        x.set_line_width(3.0);
        x.begin_path();
        x.move_to(c, c);
        x.line_to(px, py);
        x.stroke();
        x.set_fill_style_str("#ff8a4a");
        x.begin_path();
        let _ = x.arc(px, py, 9.0, 0.0, TAU);
        x.fill();
    }
}
