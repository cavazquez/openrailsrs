use openrailsrs_train::ConsistAuditor;
use std::path::PathBuf;
#[test]
fn extended_pullman_resolves_the_models_already_shipped_with_the_program() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = repo.join("examples/chiltern_extended/consists/birmingham_pullman.con");
    let result = ConsistAuditor::new(vec![]).inspect(&path);
    assert!(
        !result.player_ready(),
        "La cabina original no se incluye en el paquete de ejemplos"
    );
    assert_eq!(
        result.missing_resources.len(),
        1,
        "{:?}",
        result.missing_resources
    );
    assert_eq!(result.missing_resources[0].name, "PULLMAN_GR.s");
    assert!(result.missing_resources[0].required);
    assert_eq!(result.vehicles, 8);
    assert!(
        result
            .dependencies
            .iter()
            .any(|p| p.ends_with("SHAPES/RF_WP_DMBSA.s"))
    );
    assert!(
        result
            .compatibility
            .iter()
            .all(|c| !c.stock_path.as_os_str().is_empty())
    );
}
#[test]
#[ignore = "requires the original author package via OPENRAILSRS_NATIVE_ROUTE"]
fn extended_service_is_ready_when_the_author_package_supplies_the_cab() {
    let native =
        PathBuf::from(std::env::var_os("OPENRAILSRS_NATIVE_ROUTE").expect("Original route"));
    let content = native.parent().unwrap().parent().unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let con = repo.join("examples/chiltern_extended/consists/birmingham_pullman.con");
    let result = ConsistAuditor::new(vec![content.join("TRAINS/TRAINSET")]).inspect(&con);
    assert!(
        result.player_ready(),
        "{}\n{:?}",
        result.label(),
        result.missing_resources
    );
    assert!(result.cab_3d);
}
#[test]
fn missing_snap_resource_never_instructs_the_player_to_modify_the_package() {
    let missing = openrailsrs_train::MissingResource {
        name: "body.s".into(),
        referenced_by: "/snap/openrailsrs/6/share/model.eng".into(),
        destinations: vec!["/snap/openrailsrs/6/share/body.s".into()],
        required: true,
    };
    let guidance = missing.guidance();
    assert!(guidance.contains("sólo lectura"));
    assert!(!guidance.contains("Colocar en"));
}
