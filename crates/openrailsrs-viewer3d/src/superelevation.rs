//! Deform original single-path track meshes once per streamed instance / LOD.
//! The train and rail vertices sample the same compiled native cant profile.
use crate::{
    shapes::RouteAssets,
    world::{RouteFocus, WorldObject},
};
use bevy::{math::DVec3, mesh::VertexAttributeValues, prelude::*};
use openrailsrs_bevy_scenery::spawn::tdb_track::TrackVectorPath;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct TrackBankDeformation {
    pub path: Arc<TrackVectorPath>,
    pub range: (f64, f64),
    frame: DVec3,
    placement: Transform,
}
impl TrackBankDeformation {
    pub fn for_object(
        object: &WorldObject,
        focus: &RouteFocus,
        assets: &RouteAssets,
    ) -> Option<Arc<Self>> {
        if object.kind != "TrackObj" || object.linear.is_some() || object.scale != Vec3::ONE {
            return None;
        }
        let shape = object.section_idx?;
        let frame = DVec3::new(
            focus.center.x as f64,
            focus.height_origin as f64,
            focus.center.z as f64,
        );
        let placement = Transform {
            translation: object.render_position(focus),
            rotation: object.rotation,
            scale: object.scale,
        };
        let mut best = None;
        let mut distance = 9.0;
        for (path, range) in assets.bank_index.get(&shape)? {
            if !path.has_automatic_cant() {
                continue;
            }
            let pose = path.pose_in_frame(range.0, frame);
            let d = (pose.position - placement.translation)
                .xz()
                .length_squared();
            if d < distance && (pose.position.y - placement.translation.y).abs() < 1.0 {
                distance = d;
                best = Some(Self {
                    path: path.clone(),
                    range: *range,
                    frame,
                    placement,
                });
            }
        }
        best.map(Arc::new)
    }
    pub fn deform(&self, source: &Mesh) -> Mesh {
        let mut mesh = source.clone();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        else {
            return mesh;
        };
        let inverse = self.placement.rotation.inverse();
        let mut rotations = Vec::with_capacity(positions.len());
        for p in positions.iter_mut() {
            let world = self.placement.transform_point(Vec3::from_array(*p));
            let (banked, rotation) = self.path.bank_point_in_frame(world, self.frame, self.range);
            *p = (inverse * (banked - self.placement.translation)).to_array();
            rotations.push(inverse * rotation * self.placement.rotation);
        }
        if let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
        {
            for (normal, rotation) in normals.iter_mut().zip(rotations) {
                *normal = (rotation * Vec3::from_array(*normal))
                    .normalize_or_zero()
                    .to_array();
            }
        }
        // Native rail texture coordinates are deliberately preserved.
        mesh
    }
}
