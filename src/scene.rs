//! 親子関係のあるノードの木 (Three.js の Object3D / Group / Mesh 相当)。

use crate::gfx::{DrawItem, Material, MeshId};
use glam::{Mat4, Quat, Vec3};

pub type NodeId = usize;
pub type MatId = usize;

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
    pub visible: bool,
    pub mesh: Option<MeshId>,
    pub mat: MatId,
    pub cast: bool,
    pub recv: bool,
}

#[derive(Default)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub mats: Vec<Material>,
    world: Vec<Mat4>,
    shown: Vec<bool>,
}

impl Scene {
    pub fn mat(&mut self, m: Material) -> MatId {
        self.mats.push(m);
        self.mats.len() - 1
    }

    fn push(&mut self, parent: Option<NodeId>, mesh: Option<MeshId>, mat: MatId) -> NodeId {
        self.nodes.push(Node {
            parent,
            pos: Vec3::ZERO,
            rot: Quat::IDENTITY,
            scale: Vec3::ONE,
            visible: true,
            mesh,
            mat,
            cast: false,
            recv: false,
        });
        self.nodes.len() - 1
    }

    pub fn group(&mut self, parent: Option<NodeId>) -> NodeId {
        self.push(parent, None, 0)
    }

    pub fn mesh(&mut self, parent: Option<NodeId>, mesh: MeshId, mat: MatId) -> NodeId {
        self.push(parent, Some(mesh), mat)
    }

    pub fn n(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }

    pub fn at(&mut self, id: NodeId, x: f32, y: f32, z: f32) -> NodeId {
        self.nodes[id].pos = Vec3::new(x, y, z);
        id
    }

    /// ワールド行列と、親を含めた表示状態を計算する。親は必ず子より先に追加されている
    pub fn update(&mut self) {
        self.world.resize(self.nodes.len(), Mat4::IDENTITY);
        self.shown.resize(self.nodes.len(), true);
        for i in 0..self.nodes.len() {
            let n = &self.nodes[i];
            let local = Mat4::from_scale_rotation_translation(n.scale, n.rot, n.pos);
            let (pw, pv) = match n.parent {
                Some(p) => (self.world[p], self.shown[p]),
                None => (Mat4::IDENTITY, true),
            };
            self.world[i] = pw * local;
            self.shown[i] = pv && n.visible;
        }
    }

    pub fn draw_items(&self) -> Vec<DrawItem> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| self.shown[*i])
            .filter_map(|(i, n)| {
                n.mesh.map(|mesh| DrawItem {
                    model: self.world[i],
                    mesh,
                    mat: n.mat,
                    cast: n.cast,
                    recv: n.recv,
                })
            })
            .collect()
    }
}
