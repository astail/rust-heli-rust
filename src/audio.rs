//! ローター音。音声ファイルは使わず Web Audio で合成する (元の JS と同じ構成)。

use wasm_bindgen::prelude::*;
use web_sys::{
    AudioBufferSourceNode, AudioContext, BiquadFilterNode, BiquadFilterType, GainNode,
    OscillatorNode, OscillatorType,
};

pub struct Rotor {
    ctx: AudioContext,
    master: GainNode,
    lfo: OscillatorNode,
    chop_lp: BiquadFilterNode,
    chop_depth: GainNode,
    thump_osc: OscillatorNode,
    eng: OscillatorNode,
    eng_lp: BiquadFilterNode,
    _noise: AudioBufferSourceNode,
}

/// 音の高さや強さを決める値
pub struct Drive {
    pub on: bool,
    pub vol: f32,
    pub spin: f32,
    pub throttle: f32,
    pub speed: f32,
}

impl Rotor {
    pub fn new() -> Result<Rotor, JsValue> {
        let ctx = AudioContext::new()?;
        let master = ctx.create_gain()?;
        master.gain().set_value(0.0);
        master.connect_with_audio_node(&ctx.destination())?;
        // 共有のホワイトノイズ (2 秒をループ)
        let rate = ctx.sample_rate();
        let len = (rate * 2.0) as u32;
        let buf = ctx.create_buffer(1, len, rate)?;
        let d: Vec<f32> = (0..len)
            .map(|_| (js_sys::Math::random() * 2.0 - 1.0) as f32)
            .collect();
        buf.copy_to_channel(&d, 0)?;
        let noise = ctx.create_buffer_source()?;
        noise.set_buffer(Some(&buf));
        noise.set_loop(true);
        noise.start()?;
        // ブレードが空気を叩く周期のパルス (「バタバタ」)
        let lfo = ctx.create_oscillator()?;
        let mut real = [0.0f32, 1.0, 0.85, 0.65, 0.45, 0.3, 0.18];
        let mut imag = [0.0f32; 7];
        lfo.set_periodic_wave(&ctx.create_periodic_wave(&mut real, &mut imag)?);
        lfo.frequency().set_value(4.0);
        lfo.start()?;
        // ローターのチョップ: ノイズ → ローパス → lfo で揺らすゲイン
        let chop_lp = ctx.create_biquad_filter()?;
        chop_lp.set_type(BiquadFilterType::Lowpass);
        chop_lp.frequency().set_value(450.0);
        chop_lp.q().set_value(0.7);
        let chop = ctx.create_gain()?;
        chop.gain().set_value(0.25);
        let chop_depth = ctx.create_gain()?;
        chop_depth.gain().set_value(0.22);
        lfo.connect_with_audio_node(&chop_depth)?;
        chop_depth.connect_with_audio_param(&chop.gain())?;
        noise.connect_with_audio_node(&chop_lp)?;
        chop_lp.connect_with_audio_node(&chop)?;
        chop.connect_with_audio_node(&master)?;
        // 低いドンドンという音
        let thump_osc = ctx.create_oscillator()?;
        thump_osc.set_type(OscillatorType::Sine);
        thump_osc.frequency().set_value(55.0);
        thump_osc.start()?;
        let thump = ctx.create_gain()?;
        thump.gain().set_value(0.12);
        let thump_depth = ctx.create_gain()?;
        thump_depth.gain().set_value(0.12);
        lfo.connect_with_audio_node(&thump_depth)?;
        thump_depth.connect_with_audio_param(&thump.gain())?;
        thump_osc.connect_with_audio_node(&thump)?;
        thump.connect_with_audio_node(&master)?;
        // エンジン / タービンのうなり
        let eng = ctx.create_oscillator()?;
        eng.set_type(OscillatorType::Sawtooth);
        eng.frequency().set_value(70.0);
        eng.start()?;
        let eng_lp = ctx.create_biquad_filter()?;
        eng_lp.set_type(BiquadFilterType::Lowpass);
        eng_lp.frequency().set_value(380.0);
        let eng_g = ctx.create_gain()?;
        eng_g.gain().set_value(0.05);
        eng.connect_with_audio_node(&eng_lp)?;
        eng_lp.connect_with_audio_node(&eng_g)?;
        eng_g.connect_with_audio_node(&master)?;
        Ok(Rotor {
            ctx,
            master,
            lfo,
            chop_lp,
            chop_depth,
            thump_osc,
            eng,
            eng_lp,
            _noise: noise,
        })
    }

    pub fn resume(&self) {
        let _ = self.ctx.resume();
    }

    /// フェードせずにすぐ止め、音声のクロックも止める
    pub fn silence(&self) {
        let t = self.ctx.current_time();
        let g = self.master.gain();
        let _ = g.cancel_scheduled_values(t);
        let _ = g.set_value_at_time(0.0, t);
        let _ = self.ctx.suspend();
    }

    pub fn update(&self, d: &Drive) {
        let t = self.ctx.current_time();
        let k = 0.08;
        let rate = d.spin * (0.8 + 0.35 * d.throttle) + (d.speed / 300.0).min(0.15); // ローターの回転
        let set = |p: web_sys::AudioParam, v: f32, tc: f64| {
            let _ = p.set_target_at_time(v, t, tc);
        };
        set(
            self.master.gain(),
            if d.on {
                d.vol * (0.35 + 0.65 * d.spin)
            } else {
                0.0
            },
            0.1,
        );
        set(self.lfo.frequency(), 3.0 + 13.0 * rate, k); // ブレードの周期
        set(self.chop_lp.frequency(), 300.0 + 500.0 * rate, k);
        set(self.chop_depth.gain(), 0.14 + 0.16 * d.throttle, k); // 負荷が大きいほど重いチョップ
        set(self.thump_osc.frequency(), 45.0 + 20.0 * rate, k);
        set(self.eng.frequency(), 55.0 + 70.0 * rate + d.speed * 0.5, k);
        set(self.eng_lp.frequency(), 300.0 + 400.0 * d.throttle, k);
    }
}
