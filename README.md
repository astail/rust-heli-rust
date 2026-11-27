# Rust Heli Trainer (Rust 版)

ゲーム「Rust」の Minicopter 風ヘリコプター操作をブラウザで練習できる非公式シミュレーター [rust-heli](https://github.com/astail/rust-heli) を、Rust 言語 (WebAssembly) で書き直したものです。

**▶ プレイ:** https://astail.github.io/rust-heli-rust/

元の JavaScript 版: https://astail.github.io/rust-heli/

## 何を Rust にしたか

元の JS (Three.js) でやっていたことは、すべて Rust で書いています。

| 部分 | 元の JS 版 | Rust 版 |
|---|---|---|
| 物理・接地・墜落判定 | `stepPhysics` | `src/physics.rs` (係数・サブステップも同じ) |
| モード (リング / 着陸 / ホバリング) | `updateMode` など | `src/modes.rs` |
| 地形・街・木の配置 | 乱数の種 1337 | `src/world.rs` (同じ乱数・同じ消費順) |
| 3D 描画 | Three.js r128 | `src/gfx.rs` (WebGL2 を直接。Lambert + 半球光 + PCF の影 + フォグ) |
| 形状 (箱・球・円柱・トーラスなど) | Three.js の Geometry | `src/geom.rs` |
| 機体・機内・計器の文字盤 | Three.js + Canvas 2D | `src/models.rs` |
| カメラ 7 視点 | `updateCamera` | `src/app.rs` |
| HUD (姿勢計・マウスパッド) | Canvas 2D | `src/hud.rs` |
| ローター音 | Web Audio | `src/audio.rs` (同じ合成) |
| 設定・HUD の配置編集・メニュー | DOM | `src/ui.rs`, `src/store.rs` |

HTML と CSS (メニューや HUD の見た目) は元のものをそのまま使い、`index.html` の JS は wasm を読み込む数行だけです。

設定とベストタイムは元の JS 版と共有します (同じ `astail.github.io` 上で、同じキー `rhtr_…` に同じ形で保存します)。どちらで変えた設定・出したベストタイムも、もう片方に反映されます。以前は `rhrs_…` に分けて保存していたので、その値は起動時に一度だけ共有のキーへ移します (両方にあるときは、設定は JS 版のほうを、ベストタイムは速いほうを残します)。

## 操作

| キー | 動作 |
|---|---|
| W / S | スロットル上昇 / 下降 |
| Ctrl | 押している間は揚力が抜ける (空中なら落ちる)。地上では Ctrl+W で前進 / Ctrl+S で後退 / A・D で旋回 |
| A / D | ヨー (その場で機首を回す + 傾く。傾きはマウスで直す) |
| マウス前後 | ピッチ |
| マウス左右 | ロール |
| C / V | 視点切替 (パイロット / 機内 / コックピット / 近接 / TPS / チェイス / 俯瞰) |
| R | リスポーン |
| H | HUD 表示 ON/OFF |
| Esc | メニュー |

## ビルド

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # Cargo.toml の wasm-bindgen と同じ版
./build.sh            # dist/ に index.html と pkg/ ができる (--dev でデバッグビルド)
python3 -m http.server -d dist 8000
```

ブラウザで http://localhost:8000/ を開きます。

## テスト

物理・モード・地形・形状はブラウザに依存しないので、ネイティブで動きます。

```sh
cargo test
```

`main` に push すると GitHub Actions がテストとビルドをして GitHub Pages に公開します。

Facepunch Studios とは無関係のファンメイドです。
