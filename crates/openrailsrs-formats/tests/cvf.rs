use openrailsrs_formats::{
    CabControl, CabViewFile, ControlType, MstsFile, parse_from_first_paren, parse_msts_file,
};

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("fixture")
}

fn fixture_path(name: &str) -> String {
    format!("tests/fixtures/{name}")
}

#[test]
fn cabview_file_from_ast_maps_fields() {
    let src = read_fixture("minimal.cvf");
    let ast = parse_from_first_paren(&src).expect("parse");
    let cvf = CabViewFile::from_ast(&ast).expect("typed parse");
    assert_eq!(cvf.cab_view_type, Some(2));
    assert_eq!(cvf.views.len(), 1);
    assert_eq!(cvf.views[0].texture_ace, "panel.ace");
    assert!((cvf.views[0].position_m[0] - 1.0).abs() < 1e-9);
    assert_eq!(cvf.controls.len(), 2);
}

#[test]
fn cabview_multi_state_throttle_states() {
    let src = read_fixture("minimal.cvf");
    let ast = parse_from_first_paren(&src).expect("parse");
    let cvf = CabViewFile::from_ast(&ast).expect("typed parse");
    match &cvf.controls[1] {
        CabControl::MultiStateDisplay {
            control_type,
            states,
            graphic,
            ..
        } => {
            assert_eq!(*control_type, ControlType::ThrottleDisplay);
            assert_eq!(graphic, "throttle.ace");
            assert_eq!(states.len(), 2);
            assert!((states[1].switch_val - 1.0).abs() < 1e-9);
        }
        other => panic!("expected MultiStateDisplay, got {other:?}"),
    }
}

#[test]
fn parse_msts_file_dispatches_cvf() {
    let cvf = parse_msts_file(fixture_path("minimal.cvf")).expect("dispatch cvf");
    match cvf {
        MstsFile::CabView(file) => {
            assert_eq!(file.controls.len(), 2);
            let types = file.control_type_names();
            assert!(types.contains(&"DIRECTION_DISPLAY"));
            assert!(types.contains(&"THROTTLE_DISPLAY"));
        }
        other => panic!("expected CabView, got {other:?}"),
    }
}

#[test]
fn cabview_gp38_starter_route_when_present() {
    let path = "/home/cristian/repos/propios/ProyectoOpenRails/TS_STARTER_ROUTE/TRAINS/trainset/SLI_BNSF_GP38/CABVIEW/GP38-2.cvf";
    if !std::path::Path::new(path).exists() {
        return;
    }
    let cvf = parse_msts_file(path).expect("parse GP38 cvf");
    match cvf {
        MstsFile::CabView(file) => {
            assert_eq!(file.cab_view_type, Some(2));
            assert_eq!(file.views.len(), 3);
            assert!(file.controls.len() >= 10, "expected ~11 controls");
        }
        other => panic!("expected CabView, got {other:?}"),
    }
}

#[test]
fn cabview_real_fixture_when_env_set() {
    let Ok(path) = std::env::var("OPENRAILSRS_CVFFIXTURE") else {
        return;
    };
    let cvf = parse_msts_file(&path).expect("parse real cvf");
    match cvf {
        MstsFile::CabView(file) => {
            assert!(!file.controls.is_empty(), "expected controls in {path}");
        }
        other => panic!("expected CabView, got {other:?}"),
    }
}
