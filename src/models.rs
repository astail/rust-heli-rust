//! シーンの組み立て: 地形、建物、パッド、木、リング、ヘリ (外装 + 機内 + 計器)。

use crate::geom::{self, CpuMesh, bar_placement, look_dir_z};
use crate::gfx::{Gpu, Material, hex};
use crate::scene::{MatId, NodeId, Scene};
use crate::world::{WORLD, World, terrain_h};
use glam::{Mat4, Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, PI};
use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

pub const DEBRIS_POOL: usize = 72;
pub const DEBRIS_PER_CRASH: usize = 24;

pub struct WorldNodes {
    /// 着陸パッドの白いマーク (pads と同じ順)
    pub pad_mats: Vec<MatId>,
    pub ring_group: NodeId,
    pub ring_mats: Vec<MatId>,
    pub hover_zone: NodeId,
    pub hover_mat: MatId,
    pub marker: NodeId,
    pub debris: Vec<NodeId>,
}

pub struct HeliNodes {
    pub root: NodeId,
    pub front: NodeId,
    pub hull: NodeId,
    pub canopy: NodeId,
    pub engine: NodeId,
    pub mast: NodeId,
    pub skid_rails: Vec<NodeId>,
    pub struts: Vec<NodeId>,
    pub rotor: NodeId,
    pub blade_mats: Vec<MatId>,
    pub tail_rotor: NodeId,
    pub cabin: NodeId,
    pub torso: NodeId,
    pub head: NodeId,
    pub dash_box: NodeId,
    pub dash_mat: MatId,
    pub needles: Vec<NodeId>,
}

fn flat() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

/// パッドの「H」と輪のマーク。色をまとめて変えられるように 1 つのマテリアルを共有する
fn pad_marking(gpu: &mut Gpu, sc: &mut Scene, x: f32, y: f32, z: f32, r: f32, color: u32) -> MatId {
    let mat = sc.mat(Material::basic(color));
    let g = sc.group(None);
    sc.at(g, x, y + 0.03, z);
    let ring = sc.mesh(Some(g), gpu.upload(&geom::ring(r * 0.78, r * 0.9, 40)), mat);
    sc.n(ring).rot = flat();
    for (w, d, px) in [
        (r * 0.12, r * 0.9, -r * 0.3),
        (r * 0.12, r * 0.9, r * 0.3),
        (r * 0.6, r * 0.12, 0.0),
    ] {
        let b = sc.mesh(Some(g), gpu.upload(&geom::plane(w, d)), mat);
        sc.n(b).rot = flat();
        sc.n(b).pos.x = px;
    }
    mat
}

pub fn build_world(gpu: &mut Gpu, sc: &mut Scene, w: &World, rings: &[(Vec3, Vec3)]) -> WorldNodes {
    // 地形 (頂点カラーで草地 → 乾いた土 → 岩)
    let mut t = geom::grid(WORLD, 220);
    let (grass, dry, rock) = (hex(0x6f8a4a), hex(0x9c8f5e), hex(0x857d74));
    for p in &mut t.pos {
        p.y = terrain_h(p.x, p.z);
        let mut c = grass.lerp(dry, ((p.y + 5.0) / 25.0).clamp(0.0, 1.0));
        if p.y > 35.0 {
            c = c.lerp(rock, ((p.y - 35.0) / 30.0).min(1.0));
        }
        t.col.push(c);
    }
    t.compute_normals();
    let terrain_mat = sc.mat(Material {
        vertex_colors: true,
        ..Material::lambert(0xffffff)
    });
    let terrain = sc.mesh(None, gpu.upload(&t), terrain_mat);
    sc.n(terrain).recv = true;

    // 建物とパッドの箱
    for s in &w.solids {
        let h = s.top - s.base;
        let m = sc.mat(Material::lambert(s.color));
        let id = sc.mesh(
            None,
            gpu.upload(&geom::cuboid(s.hw * 2.0, h, s.hd * 2.0)),
            m,
        );
        sc.at(id, s.x, s.base + h / 2.0, s.z);
        sc.n(id).cast = true;
        sc.n(id).recv = true;
    }
    let home = w.solid(w.home);
    pad_marking(gpu, sc, 0.0, home.top, 0.0, 6.0, 0xffd23f);
    let pad_mats = w
        .pads
        .iter()
        .map(|p| {
            let s = w.solid(p.solid);
            pad_marking(gpu, sc, s.x, s.top, s.z, p.mark_r, 0xffffff)
        })
        .collect();

    // 木 (幹と葉をそれぞれ 1 つのメッシュにまとめる)
    let (trunk, leaf) = (
        geom::cylinder(0.3, 0.4, 3.0, 6, false),
        geom::cone(2.4, 7.0, 7),
    );
    let (mut trunks, mut leaves) = (CpuMesh::default(), CpuMesh::default());
    for tr in &w.trees {
        let s = Vec3::splat(tr.scale);
        trunks.append(
            &trunk
                .clone()
                .transformed(Mat4::from_scale_rotation_translation(
                    s,
                    Quat::IDENTITY,
                    tr.pos + Vec3::new(0.0, 1.5 * tr.scale, 0.0),
                )),
        );
        leaves.append(
            &leaf
                .clone()
                .transformed(Mat4::from_scale_rotation_translation(
                    s,
                    Quat::IDENTITY,
                    tr.pos + Vec3::new(0.0, 6.0 * tr.scale, 0.0),
                )),
        );
    }
    let trunk_mat = sc.mat(Material::lambert(0x5b4330));
    sc.mesh(None, gpu.upload(&trunks), trunk_mat);
    let leaf_mat = sc.mat(Material::lambert(0x3f5f32));
    let l = sc.mesh(None, gpu.upload(&leaves), leaf_mat);
    sc.n(l).cast = true;

    // リング
    let ring_group = sc.group(None);
    let torus = gpu.upload(&geom::torus(7.0, 0.45, 8, 32));
    let ring_mats = rings
        .iter()
        .map(|(pos, dir)| {
            let m = sc.mat(Material::basic(0x777777));
            let r = sc.mesh(Some(ring_group), torus, m);
            sc.n(r).pos = *pos;
            sc.n(r).rot = look_dir_z(*dir);
            m
        })
        .collect();
    sc.n(ring_group).visible = false;

    // ホバリングの円柱
    let hover_mat = sc.mat(
        Material::basic(0x4fc3ff)
            .translucent(0.18)
            .double_sided()
            .no_depth_write(),
    );
    let hover_zone = sc.mesh(
        None,
        gpu.upload(&geom::cylinder(6.0, 6.0, 8.0, 32, true)),
        hover_mat,
    );
    sc.n(hover_zone).visible = false;

    // 目標の上の矢印
    let marker_mat = sc.mat(Material::basic(0xff7a2f));
    let marker = sc.mesh(None, gpu.upload(&geom::cone(1.6, 3.5, 4)), marker_mat);
    sc.n(marker).rot = Quat::from_rotation_x(PI);
    sc.n(marker).visible = false;

    // 墜落時の破片
    let cube = gpu.upload(&geom::cuboid(0.3, 0.3, 0.3));
    let (dark, orange) = (
        sc.mat(Material::basic(0x333333)),
        sc.mat(Material::basic(0xff8a2a)),
    );
    let debris = (0..DEBRIS_POOL)
        .map(|i| {
            let id = sc.mesh(
                None,
                cube,
                if !(i % DEBRIS_PER_CRASH).is_multiple_of(3) {
                    dark
                } else {
                    orange
                },
            );
            sc.n(id).visible = false;
            id
        })
        .collect();

    WorldNodes {
        pad_mats,
        ring_group,
        ring_mats,
        hover_zone,
        hover_mat,
        marker,
        debris,
    }
}

struct Gauge {
    label: &'static str,
    a0: f64,
    a1: f64,
    n: usize,
    labels: &'static [&'static str],
    full: bool,
    vsi: bool,
}

const GAUGES: [Gauge; 5] = [
    Gauge {
        label: "SPD km/h",
        a0: -135.0,
        a1: 135.0,
        n: 11,
        labels: &["0", "", "40", "", "80", "", "120", "", "160", "", "200"],
        full: false,
        vsi: false,
    },
    Gauge {
        label: "ALT x10m",
        a0: 0.0,
        a1: 360.0,
        n: 10,
        labels: &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"],
        full: true,
        vsi: false,
    },
    Gauge {
        label: "V/S m/s",
        a0: -165.0,
        a1: -15.0,
        n: 0,
        labels: &[],
        full: false,
        vsi: true,
    },
    Gauge {
        label: "ROTOR %",
        a0: -135.0,
        a1: 135.0,
        n: 11,
        labels: &["0", "", "20", "", "40", "", "60", "", "80", "", "100"],
        full: false,
        vsi: false,
    },
    Gauge {
        label: "HDG",
        a0: 0.0,
        a1: 360.0,
        n: 12,
        labels: &["N", "", "", "E", "", "", "S", "", "", "W", "", ""],
        full: true,
        vsi: false,
    },
];

/// 計器の文字盤を 2D キャンバスに描く
fn dial_canvas(g: &Gauge) -> Result<HtmlCanvasElement, JsValue> {
    let doc = web_sys::window().unwrap().document().unwrap();
    let c: HtmlCanvasElement = doc.create_element("canvas")?.dyn_into()?;
    c.set_width(256);
    c.set_height(256);
    let x: CanvasRenderingContext2d = c.get_context("2d")?.ok_or("2d")?.dyn_into()?;
    let cc = 128.0;
    let rad = |deg: f64| (deg - 90.0).to_radians();
    x.set_fill_style_str("#1b1e22");
    x.begin_path();
    x.arc(cc, cc, 126.0, 0.0, 7.0)?;
    x.fill();
    x.set_fill_style_str("#e9ecef");
    x.begin_path();
    x.arc(cc, cc, 116.0, 0.0, 7.0)?;
    x.fill();
    x.set_stroke_style_str("#222");
    x.set_fill_style_str("#222");
    x.set_text_align("center");
    x.set_text_baseline("middle");
    let tick = |deg: f64, len: f64, w: f64| {
        let r = rad(deg);
        x.set_line_width(w);
        x.begin_path();
        x.move_to(cc + r.cos() * 108.0, cc + r.sin() * 108.0);
        x.line_to(cc + r.cos() * (108.0 - len), cc + r.sin() * (108.0 - len));
        x.stroke();
    };
    if g.vsi {
        // 昇降計: 9 時の位置が 0、時計回りが上昇、反時計回りが降下 (±10 m/s)
        for v in -10i32..=10 {
            let minor = v % 5 != 0;
            tick(
                -90.0 + v as f64 * 15.0,
                if minor { 10.0 } else { 20.0 },
                if minor { 2.0 } else { 5.0 },
            );
        }
        x.set_font("bold 26px sans-serif");
        for v in [-10i32, -5, 0, 5, 10] {
            let r = rad(-90.0 + v as f64 * 15.0);
            x.fill_text(
                &v.abs().to_string(),
                cc + r.cos() * 72.0,
                cc + r.sin() * 72.0,
            )?;
        }
        x.set_font("bold 18px sans-serif");
        x.fill_text("UP", cc + 30.0, cc - 40.0)?;
        x.fill_text("DN", cc + 30.0, cc + 40.0)?;
    } else {
        let steps = if g.full { g.n } else { g.n - 1 } as f64;
        for i in 0..g.n {
            let d = g.a0 + (g.a1 - g.a0) * i as f64 / steps;
            tick(d, 20.0, 5.0);
            if i < g.n - 1 || g.full {
                tick(d + (g.a1 - g.a0) / steps / 2.0, 10.0, 2.0);
            }
            let label = g.labels[i];
            if !label.is_empty() {
                let r = rad(d);
                x.set_font(if label.len() > 2 {
                    "bold 22px sans-serif"
                } else {
                    "bold 28px sans-serif"
                });
                x.fill_text(label, cc + r.cos() * 70.0, cc + r.sin() * 70.0)?;
            }
        }
    }
    x.set_font("bold 17px sans-serif");
    x.set_fill_style_str("#555");
    x.fill_text(g.label, cc, cc + 52.0)?;
    Ok(c)
}

#[allow(clippy::too_many_arguments)]
fn put(
    gpu: &mut Gpu,
    sc: &mut Scene,
    parent: NodeId,
    m: &CpuMesh,
    mat: MatId,
    x: f32,
    y: f32,
    z: f32,
) -> NodeId {
    let id = sc.mesh(Some(parent), gpu.upload(m), mat);
    sc.at(id, x, y, z)
}

pub fn build_heli(gpu: &mut Gpu, sc: &mut Scene) -> Result<HeliNodes, JsValue> {
    let root = sc.group(None);
    // ローター軸が回転の中心。コックピット (胴体・キャノピー・座席・計器) は軸より前にある
    let front = sc.group(Some(root));
    let body = sc.mat(Material::lambert(0x5c6b4e));
    let dark = sc.mat(Material::lambert(0x2b2f33));
    let glass = sc.mat(Material::lambert(0x9fc4d8).translucent(0.55));
    let add = |gpu: &mut Gpu,
               sc: &mut Scene,
               m: CpuMesh,
               mat: MatId,
               x: f32,
               y: f32,
               z: f32,
               parent: NodeId| {
        let id = put(gpu, sc, parent, &m, mat, x, y, z);
        sc.n(id).cast = true;
        id
    };
    let hull = add(
        gpu,
        sc,
        geom::cuboid(1.4, 1.1, 2.2),
        body,
        0.0,
        0.95,
        0.0,
        front,
    );
    let canopy = add(
        gpu,
        sc,
        geom::sphere(0.75, 12, 10),
        glass,
        0.0,
        1.05,
        -1.05,
        front,
    );
    sc.n(canopy).scale = Vec3::new(0.95, 0.85, 0.8);
    let engine = add(
        gpu,
        sc,
        geom::cuboid(0.9, 0.8, 1.0),
        body,
        0.0,
        1.1,
        0.0,
        root,
    ); // apply_pivot で前後に伸ばす
    add(
        gpu,
        sc,
        geom::cuboid(0.25, 0.25, 3.2),
        body,
        0.0,
        1.15,
        2.6,
        root,
    );
    add(
        gpu,
        sc,
        geom::cuboid(0.08, 0.8, 0.5),
        body,
        0.0,
        1.45,
        4.1,
        root,
    );
    let mast = add(
        gpu,
        sc,
        geom::cylinder(0.08, 0.08, 0.6, 6, false),
        dark,
        0.0,
        1.75,
        0.0,
        root,
    );
    let skid_rails = [-0.75, 0.75]
        .into_iter()
        .map(|sx| {
            add(
                gpu,
                sc,
                geom::cuboid(0.08, 0.08, 1.0),
                dark,
                sx,
                0.08,
                0.0,
                root,
            )
        })
        .collect();
    let mut struts = Vec::new();
    for _ in 0..2 {
        for sx in [-0.55, 0.55] {
            struts.push(add(
                gpu,
                sc,
                geom::cuboid(0.06, 0.45, 0.06),
                dark,
                sx,
                0.3,
                0.0,
                root,
            ));
        }
    }

    let rotor = sc.group(Some(root));
    sc.at(rotor, 0.0, 2.05, 0.0);
    let blade = gpu.upload(&geom::cuboid(0.22, 0.04, 7.6));
    let blade_mats = (0..2)
        .map(|i| {
            let m = sc.mat(Material::lambert(0x222222));
            let b = sc.mesh(Some(rotor), blade, m);
            sc.n(b).rot = Quat::from_rotation_y(i as f32 * FRAC_PI_2);
            m
        })
        .collect();
    let tail_mat = sc.mat(Material::lambert(0x222222));
    let tail_rotor = sc.mesh(
        Some(root),
        gpu.upload(&geom::cuboid(0.04, 1.1, 0.12)),
        tail_mat,
    );
    sc.at(tail_rotor, 0.12, 1.45, 4.2);

    // 機内 (機内視点とパイロット視点のときだけ見せる)
    let cabin = sc.group(Some(front));
    sc.n(cabin).visible = false;
    let body = sc.mat(Material::lambert(0x5c6b4e));
    let dark = sc.mat(Material::lambert(0x24282c));
    let metal = sc.mat(Material::lambert(0x3a3f45));
    let suit = sc.mat(Material::lambert(0x4a5a3a));
    let seat = sc.mat(Material::lambert(0x6b4f35));
    let bx = |gpu: &mut Gpu,
              sc: &mut Scene,
              w: f32,
              h: f32,
              d: f32,
              m: MatId,
              x: f32,
              y: f32,
              z: f32| { put(gpu, sc, cabin, &geom::cuboid(w, h, d), m, x, y, z) };
    bx(gpu, sc, 1.4, 0.06, 2.3, metal, 0.0, 0.42, 0.0); // 床
    bx(gpu, sc, 0.05, 0.45, 2.0, body, -0.7, 0.66, 0.05); // ドアの下枠
    bx(gpu, sc, 0.05, 0.45, 2.0, body, 0.7, 0.66, 0.05);
    bx(gpu, sc, 1.4, 0.06, 0.25, body, 0.0, 0.9, -1.15); // 機首のふち
    bx(gpu, sc, 1.4, 0.8, 0.06, body, 0.0, 0.82, 1.15); // 後ろの壁
    bx(gpu, sc, 1.2, 0.05, 0.62, body, 0.0, 1.8, -0.11); // 天井 (前)
    let dash_mat = sc.mat(Material {
        transparent: true,
        ..Material::lambert(0x24282c)
    });
    let dash_box = bx(gpu, sc, 1.15, 0.28, 0.3, dash_mat, 0.0, 0.98, -0.85);
    sc.n(dash_box).rot = Quat::from_rotation_x(-0.35);
    // 席に座ったときに読める計器
    let needle_mat = sc.mat(Material::basic(0xd04020));
    let hub_mat = sc.mat(Material::basic(0x222222));
    let (dial_mesh, needle_mesh, hub_mesh) = (
        gpu.upload(&geom::circle(0.085, 32)),
        gpu.upload(&geom::plane(0.008, 0.07)),
        gpu.upload(&geom::circle(0.009, 12)),
    );
    let mut needles = Vec::new();
    for (i, g) in GAUGES.iter().enumerate() {
        let tilt = sc.group(Some(cabin));
        sc.at(tilt, (i as f32 - 2.0) * 0.205, 1.06, -0.69);
        sc.n(tilt).rot = Quat::from_rotation_x(-0.35);
        let tex = gpu.texture_from_canvas(&dial_canvas(g)?)?;
        let face = sc.mat(Material {
            map: Some(tex),
            ..Material::basic(0xffffff)
        });
        sc.mesh(Some(tilt), dial_mesh, face);
        let spin = sc.group(Some(tilt));
        sc.n(spin).pos.z = 0.004;
        let n = sc.mesh(Some(spin), needle_mesh, needle_mat);
        sc.n(n).pos.y = 0.03;
        let hub = sc.mesh(Some(spin), hub_mesh, hub_mat);
        sc.n(hub).pos.z = 0.002;
        needles.push(spin);
    }
    let bar =
        |gpu: &mut Gpu, sc: &mut Scene, m: MatId, a: [f32; 3], b: [f32; 3], r: f32| -> NodeId {
            let (mid, rot, len) = bar_placement(Vec3::from(a), Vec3::from(b));
            let id = sc.mesh(
                Some(cabin),
                gpu.upload(&geom::cylinder(r, r, len, 6, false)),
                m,
            );
            sc.n(id).pos = mid;
            sc.n(id).rot = rot;
            id
        };
    // キャノピーの枠
    for sx in [-1.0, 1.0] {
        bar(
            gpu,
            sc,
            dark,
            [sx * 0.66, 0.9, -1.15],
            [sx * 0.6, 1.8, -0.42],
            0.035,
        ); // A ピラー
        bar(
            gpu,
            sc,
            dark,
            [sx * 0.68, 0.9, 0.45],
            [sx * 0.6, 1.8, 0.45],
            0.035,
        ); // ドアの柱
    }
    bar(gpu, sc, dark, [-0.6, 1.8, -0.42], [0.6, 1.8, -0.42], 0.035);
    bar(gpu, sc, dark, [0.0, 0.95, -1.15], [0.0, 1.8, -0.42], 0.025); // 中央の支柱
    let ws_mat = sc.mat(
        Material::basic(0xbfe0f0)
            .translucent(0.08)
            .double_sided()
            .no_depth_write(),
    );
    let ws = sc.mesh(Some(cabin), gpu.upload(&geom::plane(1.3, 1.15)), ws_mat);
    sc.at(ws, 0.0, 1.35, -0.8);
    sc.n(ws).rot = Quat::from_rotation_x(-0.9);
    // 座席とパイロット
    bx(gpu, sc, 0.55, 0.12, 0.5, seat, 0.0, 0.72, 0.35);
    bx(gpu, sc, 0.55, 0.7, 0.1, seat, 0.0, 1.05, 0.62);
    let torso = bx(gpu, sc, 0.46, 0.55, 0.28, suit, 0.0, 1.08, 0.42); // 胴
    bx(gpu, sc, 0.16, 0.12, 0.5, suit, -0.17, 0.82, 0.15); // 太もも
    bx(gpu, sc, 0.16, 0.12, 0.5, suit, 0.17, 0.82, 0.15);
    bar(gpu, sc, suit, [-0.25, 1.28, 0.42], [-0.12, 0.92, 0.0], 0.06); // 操縦桿へ伸びる腕
    bar(gpu, sc, suit, [0.25, 1.28, 0.42], [0.06, 0.92, 0.0], 0.06);
    bar(gpu, sc, dark, [0.0, 0.45, -0.05], [-0.03, 0.92, 0.0], 0.02); // サイクリック
    let helmet_mat = sc.mat(Material::lambert(0x2e3a2a));
    let head = sc.mesh(
        Some(cabin),
        gpu.upload(&geom::sphere(0.15, 14, 10)),
        helmet_mat,
    );
    sc.at(head, 0.0, 1.5, 0.4);

    Ok(HeliNodes {
        root,
        front,
        hull,
        canopy,
        engine,
        mast,
        skid_rails,
        struts,
        rotor,
        blade_mats,
        tail_rotor,
        cabin,
        torso,
        head,
        dash_box,
        dash_mat,
        needles,
    })
}

/// pivot = パイロットの頭から何 m 後ろに回転軸 (マスト) があるか
pub fn apply_pivot(sc: &mut Scene, h: &HeliNodes, pivot: f32) {
    let fz = -(pivot + 0.4); // ポッドの中でパイロットの頭は z = 0.4
    sc.n(h.front).pos.z = fz;
    let (pod_front, pod_rear) = (fz - 1.1, fz + 1.1);
    let (ef, er) = ((pod_rear - 0.2).min(-0.5), 1.0); // エンジン部がポッドとテールブームをつなぐ
    sc.n(h.engine).scale.z = er - ef;
    sc.n(h.engine).pos.z = (ef + er) / 2.0;
    let (sf, sr) = ((pod_front + 0.4).min(-1.4), 1.4); // スキッドはポッドの下まで届く
    for &r in &h.skid_rails {
        sc.n(r).scale.z = sr - sf;
        sc.n(r).pos.z = (sf + sr) / 2.0;
    }
    for (i, &o) in h.struts.iter().enumerate() {
        sc.n(o).pos.z = if i < 2 { sf + 0.7 } else { sr - 0.7 };
    }
}
