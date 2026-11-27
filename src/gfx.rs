//! WebGL2 の小さなレンダラー。Three.js r128 の MeshLambertMaterial / MeshBasicMaterial、
//! 半球光 + 平行光 (PCF の影)、線形フォグ (smoothstep)、半透明の奥から順の描画を再現する。

use crate::geom::CpuMesh;
use glam::{Mat3, Mat4, Vec3};
use wasm_bindgen::prelude::*;
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext as GL, WebGlFramebuffer, WebGlProgram, WebGlShader,
    WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
};

pub type MeshId = usize;
pub type TexId = usize;

pub const SHADOW_SIZE: i32 = 2048;

pub fn hex(c: u32) -> Vec3 {
    Vec3::new(
        ((c >> 16) & 255) as f32,
        ((c >> 8) & 255) as f32,
        (c & 255) as f32,
    ) / 255.0
}

#[derive(Clone, Debug)]
pub struct Material {
    /// true = Lambert (光の影響を受ける), false = Basic
    pub lit: bool,
    pub color: Vec3,
    pub opacity: f32,
    pub transparent: bool,
    pub vertex_colors: bool,
    pub map: Option<TexId>,
    pub double_side: bool,
    pub depth_write: bool,
}

impl Material {
    pub fn lambert(c: u32) -> Self {
        Material {
            lit: true,
            color: hex(c),
            opacity: 1.0,
            transparent: false,
            vertex_colors: false,
            map: None,
            double_side: false,
            depth_write: true,
        }
    }
    pub fn basic(c: u32) -> Self {
        Material {
            lit: false,
            ..Self::lambert(c)
        }
    }
    pub fn translucent(mut self, opacity: f32) -> Self {
        self.transparent = true;
        self.opacity = opacity;
        self
    }
    pub fn double_sided(mut self) -> Self {
        self.double_side = true;
        self
    }
    pub fn no_depth_write(mut self) -> Self {
        self.depth_write = false;
        self
    }
}

pub struct DrawItem {
    pub model: Mat4,
    pub mesh: MeshId,
    pub mat: usize,
    pub cast: bool,
    pub recv: bool,
}

pub struct Lights {
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    pub sky: Vec3,
    pub ground: Vec3,
    pub fog_color: Vec3,
    pub fog_near: f32,
    pub fog_far: f32,
    /// 影を描く範囲の (光の視点 × 正射影)
    pub light_vp: Mat4,
}

struct GpuMesh {
    vao: WebGlVertexArrayObject,
    count: i32,
}

struct MainProg {
    prog: WebGlProgram,
    model: Option<WebGlUniformLocation>,
    nmat: Option<WebGlUniformLocation>,
    view: Option<WebGlUniformLocation>,
    proj: Option<WebGlUniformLocation>,
    shadow_mat: Option<WebGlUniformLocation>,
    color: Option<WebGlUniformLocation>,
    opacity: Option<WebGlUniformLocation>,
    lit: Option<WebGlUniformLocation>,
    vcol: Option<WebGlUniformLocation>,
    use_map: Option<WebGlUniformLocation>,
    recv: Option<WebGlUniformLocation>,
    double: Option<WebGlUniformLocation>,
    sun_dir: Option<WebGlUniformLocation>,
    sun_col: Option<WebGlUniformLocation>,
    sky: Option<WebGlUniformLocation>,
    ground: Option<WebGlUniformLocation>,
    fog_col: Option<WebGlUniformLocation>,
    fog: Option<WebGlUniformLocation>,
    texel: Option<WebGlUniformLocation>,
}

struct DepthProg {
    prog: WebGlProgram,
    model: Option<WebGlUniformLocation>,
    light_vp: Option<WebGlUniformLocation>,
}

pub struct Gpu {
    pub gl: GL,
    pub canvas: HtmlCanvasElement,
    main: MainProg,
    depth: DepthProg,
    meshes: Vec<GpuMesh>,
    textures: Vec<WebGlTexture>,
    shadow_fb: WebGlFramebuffer,
    shadow_tex: WebGlTexture,
    aniso: bool,
    pub clear: Vec3,
}

const MAIN_VS: &str = r#"#version 300 es
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_nrm;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in vec3 a_col;
uniform mat4 u_model, u_view, u_proj, u_shadowMat;
uniform mat3 u_nmat;
out vec3 v_nrm;
out vec2 v_uv;
out vec3 v_col;
out vec4 v_shadow;
out float v_depth;
void main() {
  vec4 wp = u_model * vec4(a_pos, 1.0);
  vec4 vp = u_view * wp;
  v_depth = -vp.z;
  v_nrm = u_nmat * a_nrm;
  v_uv = a_uv;
  v_col = a_col;
  v_shadow = u_shadowMat * wp;
  gl_Position = u_proj * vp;
}
"#;

const MAIN_FS: &str = r#"#version 300 es
precision highp float;
precision highp sampler2DShadow;
in vec3 v_nrm;
in vec2 v_uv;
in vec3 v_col;
in vec4 v_shadow;
in float v_depth;
uniform vec3 u_color;
uniform float u_opacity;
uniform bool u_lit, u_vcol, u_useMap, u_recv, u_double;
uniform sampler2D u_map;
uniform sampler2DShadow u_shadowMap;
uniform vec3 u_sunDir, u_sunCol, u_sky, u_ground, u_fogCol;
uniform vec2 u_fog;
uniform float u_texel;
out vec4 o;
float shadowMask() {
  vec3 c = v_shadow.xyz / v_shadow.w * 0.5 + 0.5;
  if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0 || c.z > 1.0) return 1.0;
  float s = 0.0;
  for (int i = -1; i <= 1; i++)
    for (int j = -1; j <= 1; j++)
      s += texture(u_shadowMap, vec3(c.xy + vec2(float(i), float(j)) * u_texel, c.z - 0.0004));
  return s / 9.0;
}
void main() {
  vec3 base = u_color;
  float a = u_opacity;
  if (u_vcol) base *= v_col;
  if (u_useMap) { vec4 t = texture(u_map, v_uv); base *= t.rgb; a *= t.a; }
  vec3 rgb = base;
  if (u_lit) {
    vec3 n = normalize(v_nrm);
    if (u_double && !gl_FrontFacing) n = -n;
    vec3 irr = mix(u_ground, u_sky, 0.5 * n.y + 0.5);
    float nl = max(dot(n, u_sunDir), 0.0);
    irr += u_sunCol * nl * (u_recv ? shadowMask() : 1.0);
    rgb = base * irr;
  }
  rgb = mix(rgb, u_fogCol, smoothstep(u_fog.x, u_fog.y, v_depth));
  o = vec4(rgb, a);
}
"#;

const DEPTH_VS: &str = r#"#version 300 es
layout(location = 0) in vec3 a_pos;
uniform mat4 u_model, u_lightVP;
void main() { gl_Position = u_lightVP * u_model * vec4(a_pos, 1.0); }
"#;

const DEPTH_FS: &str = r#"#version 300 es
precision mediump float;
void main() {}
"#;

fn compile(gl: &GL, kind: u32, src: &str) -> Result<WebGlShader, JsValue> {
    let sh = gl.create_shader(kind).ok_or("create_shader")?;
    gl.shader_source(&sh, src);
    gl.compile_shader(&sh);
    if gl
        .get_shader_parameter(&sh, GL::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(sh)
    } else {
        Err(gl.get_shader_info_log(&sh).unwrap_or_default().into())
    }
}

fn link(gl: &GL, vs: &str, fs: &str) -> Result<WebGlProgram, JsValue> {
    let p = gl.create_program().ok_or("create_program")?;
    gl.attach_shader(&p, &compile(gl, GL::VERTEX_SHADER, vs)?);
    gl.attach_shader(&p, &compile(gl, GL::FRAGMENT_SHADER, fs)?);
    gl.link_program(&p);
    if gl
        .get_program_parameter(&p, GL::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(p)
    } else {
        Err(gl.get_program_info_log(&p).unwrap_or_default().into())
    }
}

fn f32s(v: impl IntoIterator<Item = f32>) -> js_sys::Float32Array {
    let v: Vec<f32> = v.into_iter().collect();
    js_sys::Float32Array::from(v.as_slice())
}

impl Gpu {
    pub fn new(canvas: HtmlCanvasElement, clear: Vec3) -> Result<Gpu, JsValue> {
        let attrs = web_sys::WebGlContextAttributes::new();
        attrs.set_antialias(true);
        attrs.set_alpha(false);
        let gl: GL = canvas
            .get_context_with_context_options("webgl2", &attrs)?
            .ok_or("WebGL2 が使えません")?
            .dyn_into()?;

        let mp = link(&gl, MAIN_VS, MAIN_FS)?;
        let u = |n: &str| gl.get_uniform_location(&mp, n);
        let main = MainProg {
            model: u("u_model"),
            nmat: u("u_nmat"),
            view: u("u_view"),
            proj: u("u_proj"),
            shadow_mat: u("u_shadowMat"),
            color: u("u_color"),
            opacity: u("u_opacity"),
            lit: u("u_lit"),
            vcol: u("u_vcol"),
            use_map: u("u_useMap"),
            recv: u("u_recv"),
            double: u("u_double"),
            sun_dir: u("u_sunDir"),
            sun_col: u("u_sunCol"),
            sky: u("u_sky"),
            ground: u("u_ground"),
            fog_col: u("u_fogCol"),
            fog: u("u_fog"),
            texel: u("u_texel"),
            prog: mp.clone(),
        };
        gl.use_program(Some(&mp));
        gl.uniform1i(u("u_map").as_ref(), 0);
        gl.uniform1i(u("u_shadowMap").as_ref(), 1);

        let dp = link(&gl, DEPTH_VS, DEPTH_FS)?;
        let depth = DepthProg {
            model: gl.get_uniform_location(&dp, "u_model"),
            light_vp: gl.get_uniform_location(&dp, "u_lightVP"),
            prog: dp,
        };

        // 影用の深度テクスチャ (比較モードでハードウェア PCF)
        let shadow_tex = gl.create_texture().ok_or("create_texture")?;
        gl.bind_texture(GL::TEXTURE_2D, Some(&shadow_tex));
        gl.tex_storage_2d(
            GL::TEXTURE_2D,
            1,
            GL::DEPTH_COMPONENT24,
            SHADOW_SIZE,
            SHADOW_SIZE,
        );
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_MIN_FILTER, GL::LINEAR as i32);
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_MAG_FILTER, GL::LINEAR as i32);
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_WRAP_S, GL::CLAMP_TO_EDGE as i32);
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_WRAP_T, GL::CLAMP_TO_EDGE as i32);
        gl.tex_parameteri(
            GL::TEXTURE_2D,
            GL::TEXTURE_COMPARE_MODE,
            GL::COMPARE_REF_TO_TEXTURE as i32,
        );
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_COMPARE_FUNC, GL::LEQUAL as i32);
        let shadow_fb = gl.create_framebuffer().ok_or("create_framebuffer")?;
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&shadow_fb));
        gl.framebuffer_texture_2d(
            GL::FRAMEBUFFER,
            GL::DEPTH_ATTACHMENT,
            GL::TEXTURE_2D,
            Some(&shadow_tex),
            0,
        );
        gl.draw_buffers(&js_sys::Array::of1(&JsValue::from(GL::NONE)));
        gl.read_buffer(GL::NONE);
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);
        // 影のテクスチャはユニット 1 専用。ユニット 0 (u_map) に残さない
        gl.bind_texture(GL::TEXTURE_2D, None);

        let aniso = gl
            .get_extension("EXT_texture_filter_anisotropic")
            .ok()
            .flatten()
            .is_some();
        gl.enable(GL::DEPTH_TEST);
        gl.depth_func(GL::LEQUAL);

        Ok(Gpu {
            gl,
            canvas,
            main,
            depth,
            meshes: Vec::new(),
            textures: Vec::new(),
            shadow_fb,
            shadow_tex,
            aniso,
            clear,
        })
    }

    pub fn upload(&mut self, m: &CpuMesh) -> MeshId {
        let gl = &self.gl;
        let vao = gl.create_vertex_array().expect("vao");
        gl.bind_vertex_array(Some(&vao));
        let attr = |loc: u32, size: i32, data: js_sys::Float32Array| {
            let b = gl.create_buffer().expect("buffer");
            gl.bind_buffer(GL::ARRAY_BUFFER, Some(&b));
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &data, GL::STATIC_DRAW);
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_with_i32(loc, size, GL::FLOAT, false, 0, 0);
        };
        attr(0, 3, f32s(m.pos.iter().flat_map(|p| p.to_array())));
        attr(1, 3, f32s(m.nrm.iter().flat_map(|p| p.to_array())));
        attr(2, 2, f32s(m.uv.iter().flat_map(|p| p.to_array())));
        if !m.col.is_empty() {
            attr(3, 3, f32s(m.col.iter().flat_map(|p| p.to_array())));
        }
        let ib = gl.create_buffer().expect("index buffer");
        gl.bind_buffer(GL::ELEMENT_ARRAY_BUFFER, Some(&ib));
        gl.buffer_data_with_array_buffer_view(
            GL::ELEMENT_ARRAY_BUFFER,
            &js_sys::Uint32Array::from(m.idx.as_slice()),
            GL::STATIC_DRAW,
        );
        gl.bind_vertex_array(None);
        self.meshes.push(GpuMesh {
            vao,
            count: m.idx.len() as i32,
        });
        self.meshes.len() - 1
    }

    /// 2D キャンバスの絵をテクスチャにする (計器の文字盤)
    pub fn texture_from_canvas(&mut self, c: &HtmlCanvasElement) -> Result<TexId, JsValue> {
        let gl = &self.gl;
        let t = gl.create_texture().ok_or("create_texture")?;
        gl.active_texture(GL::TEXTURE0);
        gl.bind_texture(GL::TEXTURE_2D, Some(&t));
        gl.pixel_storei(GL::UNPACK_FLIP_Y_WEBGL, 1);
        gl.tex_image_2d_with_u32_and_u32_and_html_canvas_element(
            GL::TEXTURE_2D,
            0,
            GL::RGBA as i32,
            GL::RGBA,
            GL::UNSIGNED_BYTE,
            c,
        )?;
        gl.pixel_storei(GL::UNPACK_FLIP_Y_WEBGL, 0);
        gl.generate_mipmap(GL::TEXTURE_2D);
        gl.tex_parameteri(
            GL::TEXTURE_2D,
            GL::TEXTURE_MIN_FILTER,
            GL::LINEAR_MIPMAP_LINEAR as i32,
        );
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_MAG_FILTER, GL::LINEAR as i32);
        if self.aniso {
            const TEXTURE_MAX_ANISOTROPY_EXT: u32 = 0x84FE;
            gl.tex_parameterf(GL::TEXTURE_2D, TEXTURE_MAX_ANISOTROPY_EXT, 4.0);
        }
        self.textures.push(t);
        Ok(self.textures.len() - 1)
    }

    /// キャンバスの画素数を CSS サイズ × devicePixelRatio (最大 2) に合わせる
    pub fn resize(&self, css_w: f64, css_h: f64, dpr: f64) {
        let pr = dpr.min(2.0);
        self.canvas.set_width((css_w * pr).floor() as u32);
        self.canvas.set_height((css_h * pr).floor() as u32);
    }

    fn cull(&self, m: &Material, shadow_pass: bool) {
        let gl = &self.gl;
        if m.double_side {
            gl.disable(GL::CULL_FACE);
        } else {
            gl.enable(GL::CULL_FACE);
            // Three.js と同じく、影は裏面で描いてシャドウアクネを避ける
            gl.cull_face(if shadow_pass { GL::FRONT } else { GL::BACK });
        }
    }

    fn draw_mesh(&self, id: MeshId) {
        let m = &self.meshes[id];
        self.gl.bind_vertex_array(Some(&m.vao));
        self.gl
            .draw_elements_with_i32(GL::TRIANGLES, m.count, GL::UNSIGNED_INT, 0);
    }

    pub fn render(
        &self,
        items: &[DrawItem],
        mats: &[Material],
        view: Mat4,
        proj: Mat4,
        l: &Lights,
    ) {
        let gl = &self.gl;

        // --- 影 ---
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&self.shadow_fb));
        gl.viewport(0, 0, SHADOW_SIZE, SHADOW_SIZE);
        gl.depth_mask(true);
        gl.disable(GL::BLEND);
        gl.clear(GL::DEPTH_BUFFER_BIT);
        gl.use_program(Some(&self.depth.prog));
        gl.uniform_matrix4fv_with_f32_array(
            self.depth.light_vp.as_ref(),
            false,
            &l.light_vp.to_cols_array(),
        );
        for it in items.iter().filter(|i| i.cast) {
            self.cull(&mats[it.mat], true);
            gl.uniform_matrix4fv_with_f32_array(
                self.depth.model.as_ref(),
                false,
                &it.model.to_cols_array(),
            );
            self.draw_mesh(it.mesh);
        }

        // --- 本描画 ---
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);
        gl.viewport(
            0,
            0,
            self.canvas.width() as i32,
            self.canvas.height() as i32,
        );
        gl.clear_color(self.clear.x, self.clear.y, self.clear.z, 1.0);
        gl.depth_mask(true);
        gl.clear(GL::COLOR_BUFFER_BIT | GL::DEPTH_BUFFER_BIT);
        let p = &self.main;
        gl.use_program(Some(&p.prog));
        gl.active_texture(GL::TEXTURE1);
        gl.bind_texture(GL::TEXTURE_2D, Some(&self.shadow_tex));
        gl.uniform_matrix4fv_with_f32_array(p.view.as_ref(), false, &view.to_cols_array());
        gl.uniform_matrix4fv_with_f32_array(p.proj.as_ref(), false, &proj.to_cols_array());
        gl.uniform_matrix4fv_with_f32_array(
            p.shadow_mat.as_ref(),
            false,
            &l.light_vp.to_cols_array(),
        );
        gl.uniform3fv_with_f32_array(p.sun_dir.as_ref(), &l.sun_dir.to_array());
        gl.uniform3fv_with_f32_array(p.sun_col.as_ref(), &l.sun_color.to_array());
        gl.uniform3fv_with_f32_array(p.sky.as_ref(), &l.sky.to_array());
        gl.uniform3fv_with_f32_array(p.ground.as_ref(), &l.ground.to_array());
        gl.uniform3fv_with_f32_array(p.fog_col.as_ref(), &l.fog_color.to_array());
        gl.uniform2f(p.fog.as_ref(), l.fog_near, l.fog_far);
        gl.uniform1f(p.texel.as_ref(), 1.0 / SHADOW_SIZE as f32);

        // 不透明 → 半透明 (奥から手前へ)
        let (mut opaque, mut trans): (Vec<&DrawItem>, Vec<&DrawItem>) =
            items.iter().partition(|i| !mats[i.mat].transparent);
        let depth_of = |i: &DrawItem| (view * i.model.w_axis).z;
        opaque.sort_by(|a, b| depth_of(b).total_cmp(&depth_of(a)));
        trans.sort_by(|a, b| depth_of(a).total_cmp(&depth_of(b)));

        gl.disable(GL::BLEND);
        for it in opaque {
            self.draw_item(it, &mats[it.mat]);
        }
        gl.enable(GL::BLEND);
        gl.blend_func_separate(
            GL::SRC_ALPHA,
            GL::ONE_MINUS_SRC_ALPHA,
            GL::ONE,
            GL::ONE_MINUS_SRC_ALPHA,
        );
        for it in trans {
            self.draw_item(it, &mats[it.mat]);
        }
        gl.depth_mask(true);
        gl.disable(GL::BLEND);
    }

    fn draw_item(&self, it: &DrawItem, m: &Material) {
        let gl = &self.gl;
        let p = &self.main;
        self.cull(m, false);
        gl.depth_mask(m.depth_write);
        let nmat = Mat3::from_mat4(it.model).inverse().transpose();
        gl.uniform_matrix4fv_with_f32_array(p.model.as_ref(), false, &it.model.to_cols_array());
        gl.uniform_matrix3fv_with_f32_array(p.nmat.as_ref(), false, &nmat.to_cols_array());
        gl.uniform3fv_with_f32_array(p.color.as_ref(), &m.color.to_array());
        gl.uniform1f(
            p.opacity.as_ref(),
            if m.transparent { m.opacity } else { 1.0 },
        );
        gl.uniform1i(p.lit.as_ref(), m.lit as i32);
        gl.uniform1i(p.vcol.as_ref(), m.vertex_colors as i32);
        gl.uniform1i(p.recv.as_ref(), (it.recv && m.lit) as i32);
        gl.uniform1i(p.double.as_ref(), m.double_side as i32);
        match m.map {
            Some(t) => {
                gl.active_texture(GL::TEXTURE0);
                gl.bind_texture(GL::TEXTURE_2D, Some(&self.textures[t]));
                gl.uniform1i(p.use_map.as_ref(), 1);
            }
            None => gl.uniform1i(p.use_map.as_ref(), 0),
        }
        self.draw_mesh(it.mesh);
    }
}
