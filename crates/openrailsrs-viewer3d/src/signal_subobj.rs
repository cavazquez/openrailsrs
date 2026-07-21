//! WORLD Signal mesh sub-object visibility from `SignalSubObj` + sigcfg (#80).
//!
//! Port of Open Rails `SignalShape` constructor in `Signals.cs` (SubObjVisible).

use openrailsrs_formats::{ShapeFile, SignalShapeDef};

/// Per-sub-object visibility for a signal shape instance (LOD0 / first distance level).
///
/// Returns `None` when the shape has no sub-objects (caller keeps all parts).
pub fn signal_subobj_visible(
    shape: &ShapeFile,
    sig_shape: &SignalShapeDef,
    signal_sub_obj: u32,
) -> Option<Vec<bool>> {
    let level = shape.lod_controls.first()?.distance_levels.first()?;
    let n = level.sub_objects.len();
    if n == 0 {
        return None;
    }

    let matrix_names: Vec<String> = shape
        .matrices
        .iter()
        .map(|m| m.name.to_ascii_uppercase())
        .collect();
    let mut visible_matrix = vec![false; matrix_names.len().max(1)];
    if !visible_matrix.is_empty() {
        visible_matrix[0] = true;
    }

    for (i, sub) in sig_shape.sub_objs.iter().enumerate() {
        if ((signal_sub_obj >> i) & 1) == 0 {
            continue;
        }
        let want = sub.matrix_name.to_ascii_uppercase();
        if let Some(idx) = matrix_names.iter().position(|n| n == &want) {
            if idx < visible_matrix.len() {
                visible_matrix[idx] = true;
            }
        }
    }

    let root = shape.root_sub_object_index().min(n.saturating_sub(1));
    let mut sub_visible = vec![false; n];
    sub_visible[0] = true;
    sub_visible[root] = true;

    for (i, sub) in level.sub_objects.iter().enumerate().skip(1) {
        if i == root {
            continue;
        }
        if sub.primitives.is_empty() {
            continue;
        }
        let hierarchy = &level.hierarchy;
        let first_matrix = matrix_idx_for_prim(shape, sub.primitives[0].prim_state_idx);
        let first_hi = hierarchy.get(first_matrix).copied().unwrap_or(-1);
        let mut min_hi_lev_index = 0usize;
        if first_hi > 0 {
            let mut min_hi = 999i32;
            for (j, prim) in sub.primitives.iter().enumerate() {
                let mi = matrix_idx_for_prim(shape, prim.prim_state_idx);
                let hi = hierarchy.get(mi).copied().unwrap_or(999);
                if hi < min_hi {
                    min_hi = hi;
                    min_hi_lev_index = j;
                }
            }
        }
        let matrix_idx =
            matrix_idx_for_prim(shape, sub.primitives[min_hi_lev_index].prim_state_idx);
        sub_visible[i] = visible_matrix.get(matrix_idx).copied().unwrap_or(false);
    }

    Some(sub_visible)
}

fn matrix_idx_for_prim(shape: &ShapeFile, prim_state_idx: i32) -> usize {
    shape
        .prim_states
        .get(prim_state_idx.max(0) as usize)
        .and_then(|ps| shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize))
        .map(|vs| vs.matrix_idx.max(0) as usize)
        .unwrap_or(0)
}

/// Whether a baked part (`sub_object_idx`) should draw for this signal instance.
pub fn signal_part_visible(subobj_visible: &[bool], sub_object_idx: u32) -> bool {
    if sub_object_idx == u32::MAX {
        // Merged across sub-objects — cannot cull; keep (rare for signals with keep_sub_objects).
        return true;
    }
    subobj_visible
        .get(sub_object_idx as usize)
        .copied()
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{
        DistanceLevel, LodControl, Matrix43, NamedMatrix, PrimState, Primitive,
        SignalShapeSubObjDef, SubObject, VtxState,
    };

    fn identity() -> Matrix43 {
        Matrix43 {
            rows: [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
            ],
        }
    }

    fn two_head_signal_shape() -> ShapeFile {
        // Matrices: MAIN(0), HEAD1(1), HEAD2(2). Subobj 0 root, 1→HEAD1, 2→HEAD2.
        ShapeFile {
            matrices: vec![
                NamedMatrix {
                    name: "MAIN".into(),
                    matrix: identity(),
                },
                NamedMatrix {
                    name: "HEAD1".into(),
                    matrix: identity(),
                },
                NamedMatrix {
                    name: "HEAD2".into(),
                    matrix: identity(),
                },
            ],
            vtx_states: vec![
                VtxState {
                    matrix_idx: 0,
                    ..Default::default()
                },
                VtxState {
                    matrix_idx: 1,
                    ..Default::default()
                },
                VtxState {
                    matrix_idx: 2,
                    ..Default::default()
                },
            ],
            prim_states: vec![
                PrimState {
                    vertex_state_idx: 0,
                    ..Default::default()
                },
                PrimState {
                    vertex_state_idx: 1,
                    ..Default::default()
                },
                PrimState {
                    vertex_state_idx: 2,
                    ..Default::default()
                },
            ],
            lod_controls: vec![LodControl {
                distance_levels: vec![DistanceLevel {
                    selection_m: 200.0,
                    hierarchy: vec![-1, 0, 0],
                    sub_objects: vec![
                        SubObject {
                            geometry_node_map: vec![0, 1, 2],
                            primitives: vec![Primitive {
                                prim_state_idx: 0,
                                vertex_indices: vec![0, 1, 2],
                            }],
                            ..Default::default()
                        },
                        SubObject {
                            geometry_node_map: vec![-1, 0, -1],
                            primitives: vec![Primitive {
                                prim_state_idx: 1,
                                vertex_indices: vec![0, 1, 2],
                            }],
                            ..Default::default()
                        },
                        SubObject {
                            geometry_node_map: vec![-1, -1, 0],
                            primitives: vec![Primitive {
                                prim_state_idx: 2,
                                vertex_indices: vec![0, 1, 2],
                            }],
                            ..Default::default()
                        },
                    ],
                }],
            }],
            ..Default::default()
        }
    }

    fn sig_def() -> SignalShapeDef {
        SignalShapeDef {
            shape_file: "TEST.S".into(),
            description: "test".into(),
            sub_objs: vec![
                SignalShapeSubObjDef {
                    index: 0,
                    matrix_name: "HEAD1".into(),
                    signal_sub_type: None,
                    signal_type_name: Some("TYPE_A".into()),
                },
                SignalShapeSubObjDef {
                    index: 1,
                    matrix_name: "HEAD2".into(),
                    signal_sub_type: None,
                    signal_type_name: Some("TYPE_B".into()),
                },
            ],
        }
    }

    #[test]
    fn mask_hides_unselected_head_subobject() {
        let shape = two_head_signal_shape();
        assert_eq!(shape.root_sub_object_index(), 0);
        // Only first SignalSubObj bit → HEAD1 visible, HEAD2 hidden.
        let vis = signal_subobj_visible(&shape, &sig_def(), 0b01).expect("vis");
        assert_eq!(vis.len(), 3);
        assert!(vis[0], "subobj 0 always visible");
        assert!(vis[1], "HEAD1 selected");
        assert!(!vis[2], "HEAD2 not in mask");
        assert!(signal_part_visible(&vis, 1));
        assert!(!signal_part_visible(&vis, 2));
    }

    #[test]
    fn both_heads_visible_when_mask_has_both_bits() {
        let shape = two_head_signal_shape();
        let vis = signal_subobj_visible(&shape, &sig_def(), 0b11).expect("vis");
        assert!(vis[1] && vis[2]);
    }
}
