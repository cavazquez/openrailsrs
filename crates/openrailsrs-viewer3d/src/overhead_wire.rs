//! Overhead contact wire for electrified routes (#36).
//!
//! Mirrors Open Rails `Viewer3D/Wire.cs`: generate wire over `TrackObj` / `Dyntrack`
//! when `.trk` `Electrified` is set, skipping `RoadShape` and HideWire watermarks.

use bevy::prelude::*;
use openrailsrs_formats::{OverheadWireParams, RouteFile};

use crate::shapes::RouteAssets;
use crate::viewer_log;
use crate::world::WorldObject;

pub use openrailsrs_bevy_scenery::spawn::wire::{
    OverheadWireStyle, append_overhead_wire_segment, spawn_overhead_wire_batch,
};

/// Runtime wire policy loaded from the route `.trk`.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RouteWireConfig {
    pub enabled: bool,
    pub style: OverheadWireStyle,
}

impl RouteWireConfig {
    pub fn from_params(params: OverheadWireParams) -> Self {
        Self {
            enabled: params.electrified,
            style: OverheadWireStyle {
                height_m: params.height_m,
                double_wire: params.double_wire,
                double_wire_height_m: params.double_wire_height_m,
            },
        }
    }

    pub fn load_from_route_dir(route_dir: &std::path::Path) -> Self {
        match RouteFile::from_route_dir(route_dir) {
            Ok(route) => {
                let cfg = Self::from_params(route.overhead_wire);
                if cfg.enabled {
                    viewer_log!(
                        "openrailsrs-viewer3d: overhead wire enabled height={:.2} m double={}",
                        cfg.style.height_m,
                        cfg.style.double_wire
                    );
                    if cfg.style.height_m > 100.0 {
                        viewer_log!(
                            "openrailsrs-viewer3d: OverheadWireHeight={:.0} m is unusually high (route may hide wire via height)",
                            cfg.style.height_m
                        );
                    }
                } else {
                    viewer_log!("openrailsrs-viewer3d: overhead wire disabled (Electrified=false)");
                }
                cfg
            }
            Err(err) => {
                viewer_log!("openrailsrs-viewer3d: overhead wire: no .trk ({err}); disabled");
                Self::default()
            }
        }
    }
}

/// HideWire workaround: MSTS `Tr_Watermark` levels 2 and 3 omit wire
/// (see <http://msts.steam4me.net/tutorials/hidewire.html>).
pub fn is_hide_wire_detail_level(level: u32) -> bool {
    level == 2 || level == 3
}

/// Whether this WORLD object should receive procedural overhead wire.
pub fn should_draw_wire_for(
    obj: &WorldObject,
    assets: &RouteAssets,
    config: &RouteWireConfig,
) -> bool {
    if !config.enabled {
        return false;
    }
    if obj.kind != "TrackObj" && obj.kind != "Dyntrack" {
        return false;
    }
    if is_hide_wire_detail_level(obj.static_detail_level) {
        return false;
    }
    if obj.kind == "TrackObj" {
        if let Some(idx) = obj.section_idx {
            if assets.tsection().is_road_shape(idx) {
                return false;
            }
        }
        if obj
            .shape_file
            .as_deref()
            .is_some_and(is_likely_road_shape_file)
        {
            return false;
        }
    }
    true
}

fn is_likely_road_shape_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("road") || lower.contains("hwy") || lower.contains("street")
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::OverheadWireParams;

    #[test]
    fn hide_wire_levels() {
        assert!(!is_hide_wire_detail_level(0));
        assert!(!is_hide_wire_detail_level(1));
        assert!(is_hide_wire_detail_level(2));
        assert!(is_hide_wire_detail_level(3));
        assert!(!is_hide_wire_detail_level(4));
    }

    #[test]
    fn config_from_params_respects_electrified() {
        let on = RouteWireConfig::from_params(OverheadWireParams {
            electrified: true,
            height_m: 7.23,
            double_wire: false,
            double_wire_height_m: 1.0,
        });
        assert!(on.enabled);
        assert!((on.style.height_m - 7.23).abs() < 1e-3);

        let off = RouteWireConfig::from_params(OverheadWireParams {
            electrified: false,
            ..Default::default()
        });
        assert!(!off.enabled);
    }
}
