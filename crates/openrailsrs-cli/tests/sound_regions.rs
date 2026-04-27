//! Integration tests for the runtime sound-region tracker.
//!
//! These tests exercise [`openrailsrs_cli::sound_regions::RegionTracker`]
//! directly without needing an audio device, so they remain green in CI.

use openrailsrs_cli::sound_regions::{RegionTracker, RegionTransition};
use openrailsrs_scenarios::SoundRegionDef;

fn region(id: &str, edge: &str, pos: f64, radius: f64, kind: &str) -> SoundRegionDef {
    SoundRegionDef {
        id: id.into(),
        edge_id: edge.into(),
        position_m: pos,
        radius_m: radius,
        kind: kind.into(),
        base_volume: 0.4,
    }
}

#[test]
fn empty_tracker_never_emits_transitions() {
    let mut tracker = RegionTracker::new(Vec::new());
    assert!(tracker.step("e1", 0.0).is_empty());
    assert!(tracker.step("e1", 100.0).is_empty());
}

#[test]
fn enter_then_leave_along_the_same_edge() {
    let mut tracker = RegionTracker::new(vec![region("sr1", "e5", 100.0, 25.0, "tunnel")]);
    let approach = tracker.step("e5", 50.0);
    assert!(approach.is_empty());

    let entered = tracker.step("e5", 95.0);
    assert_eq!(entered.len(), 1);
    match &entered[0] {
        RegionTransition::Enter { id, kind, .. } => {
            assert_eq!(id, "sr1");
            assert_eq!(kind, "tunnel");
        }
        other => panic!("unexpected transition: {other:?}"),
    }

    let inside = tracker.step("e5", 110.0);
    assert!(inside.is_empty(), "no re-enter while still inside");

    let left = tracker.step("e5", 200.0);
    assert_eq!(left.len(), 1);
    assert!(matches!(&left[0], RegionTransition::Leave { id } if id == "sr1"));
}

#[test]
fn switching_edges_leaves_previous_region_and_enters_new_one() {
    let mut tracker = RegionTracker::new(vec![
        region("sr1", "e1", 100.0, 30.0, "depot"),
        region("sr2", "e2", 50.0, 10.0, "tunnel"),
    ]);
    tracker.step("e1", 100.0);
    let evs = tracker.step("e2", 50.0);
    let mut sorted: Vec<String> = evs
        .iter()
        .map(|t| match t {
            RegionTransition::Enter { id, .. } => format!("enter:{id}"),
            RegionTransition::Leave { id } => format!("leave:{id}"),
        })
        .collect();
    sorted.sort();
    assert_eq!(sorted, vec!["enter:sr2", "leave:sr1"]);
}
