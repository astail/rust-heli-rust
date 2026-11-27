//! Rust Heli Trainer の Rust (WebAssembly) 版。
//! 物理・描画 (WebGL2)・音 (Web Audio)・UI の制御をすべて Rust で書いている。

pub mod geom;
pub mod modes;
pub mod physics;
pub mod world;

mod app;
mod audio;
mod gfx;
mod hud;
mod models;
mod scene;
mod store;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// 毎フレーム自分自身を予約し直すコールバック
type FrameCb = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

fn request_frame(f: &Closure<dyn FnMut(f64)>) {
    if let Some(w) = web_sys::window() {
        let _ = w.request_animation_frame(f.as_ref().unchecked_ref());
    }
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let app = match app::App::new() {
        Ok(a) => Rc::new(RefCell::new(a)),
        Err(e) => {
            // 起動できなかった理由をスタートボタンに出す (WebGL2 が無いなど)
            if let Ok(b) = ui::el("start") {
                b.set_text_content(Some(&format!(
                    "起動できませんでした: {}",
                    e.as_string().unwrap_or_default()
                )));
            }
            return Err(e);
        }
    };
    ui::bind(&app)?;

    let next: FrameCb = Rc::new(RefCell::new(None));
    let first = next.clone();
    *first.borrow_mut() = Some(Closure::new(move |t: f64| {
        if let Some(f) = next.borrow().as_ref() {
            request_frame(f);
        }
        if let Ok(mut a) = app.try_borrow_mut() {
            a.frame(t);
        }
    }));
    if let Some(f) = first.borrow().as_ref() {
        request_frame(f);
    }
    Ok(())
}
