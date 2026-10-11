//! Expected data is captured from the unmodified, pinned OR 1.6.1 DLLs.
use openrailsrs_formats::{
    PathFile, WagonFile,
    lexer::{Lexer, Token},
};

fn reference() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../oracles/openrails-parser.json")).unwrap()
}

#[test]
fn native_pdp_classification_matches_the_original_dll_properties() {
    for sample in reference()["point_flags"].as_array().unwrap() {
        let point = openrailsrs_formats::PathDataPoint {
            node_id: None,
            world: None,
            junction_flag: sample["junction_flag"].as_i64().unwrap() as i32,
            invalid_flag: sample["invalid_flag"].as_i64().unwrap() as i32,
        };
        assert_eq!(
            point.is_junction(),
            sample["is_junction"].as_bool().unwrap()
        );
        assert_eq!(point.is_invalid(), sample["is_invalid"].as_bool().unwrap());
    }
}

#[test]
fn includes_and_native_hexadecimal_path_flags_match_openrails() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("INCLUDE")).unwrap();
    std::fs::write(temp.path().join("INCLUDE/mass.inc"), "\nMass ( 42t )").unwrap();
    let wagon = temp.path().join("car.wag");
    std::fs::write(&wagon, "SIMISA@@@@@@@@@@JINX0D0t______\nWagon ( car Include ( \"/INCLUDE/mass.inc\" ) Size ( 3m 4m 20m ) )").unwrap();
    let expanded =
        openrailsrs_formats::vehicle_include::read_msts_text_with_includes(&wagon).unwrap();
    let body = expanded
        .strip_prefix("SIMISA@@@@@@@@@@JINX0D0t______\n")
        .unwrap_or(&expanded);
    let mut lexer = Lexer::new_stf(body);
    let mut tokens = vec![];
    while let Some(token) = lexer.next_token().unwrap() {
        tokens.push(match token {
            Token::LParen => "(".into(),
            Token::RParen => ")".into(),
            Token::String(s) | Token::Symbol(s) => s,
            Token::Integer(n) => n.to_string(),
            Token::Number(n) => n.to_string(),
        });
    }
    let expected = reference();
    assert_eq!(serde_json::to_value(tokens).unwrap(), expected["include"]);
    assert_eq!(
        WagonFile::from_ast(
            &openrailsrs_formats::vehicle_include::read_vehicle_ast(&wagon).unwrap()
        )
        .unwrap()
        .mass_kg,
        42000.0
    );
    let pat = temp.path().join("flags.pat");
    std::fs::write(&pat, "SIMISA@@@@@@@@@@JINX0P0t______\nTrackPDPs ( TrackPDP (0 0 0 0 0 1 1) TrackPDP (0 0 100 0 0 1 1) TrackPDP (0 0 200 0 0 1 1) ) TrackPath (TrPathName ( Flags ) TrPathNodes (3 TrPathNode (00000000 1 4294967295 0) TrPathNode (000A0001 2 4294967295 1) TrPathNode (001E0002 4294967295 4294967295 2)))").unwrap();
    assert_eq!(
        serde_json::to_value(PathFile::from_path(pat).unwrap().node_flags).unwrap(),
        expected["flags"]
    );
}

#[test]
fn original_eof_display_message_is_retained_and_other_broken_strings_are_rejected() {
    let text = "Tr_Activity ( Tr_Activity_File ( Events ( EventCategoryTime ( Outcomes ( DisplayMessage ( \"Legacy tutorial text at EOF";
    let ast = openrailsrs_formats::parse_activity_text(text).unwrap();
    let expected = reference();
    let message = expected["trailing"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    assert!(format!("{ast:?}").contains(message));
    assert!(openrailsrs_formats::parse_activity_text("Tr_Activity ( Name ( \"broken").is_err());
}

#[test]
fn a_legacy_message_consuming_a_later_quote_matches_the_original_token_stream() {
    let text = "Tr_Activity ( Name ( \"Legacy\" ) Tr_Activity_File ( Events ( EventCategoryLocation ( Outcomes ( DisplayMessage ( \"Legacy end text.\nReturn to depot.\n ) ) Name ( \"End\" ) Location (0 0 0 0 10) ) ) ) ActivityObjects ( ActivityObject ( ObjectType ( WagonsList ) Name ( \"Static\" ) ) ) )";
    let ast = openrailsrs_formats::parse_activity_text(text).unwrap();
    assert!(format!("{ast:?}").contains("Legacy end text."));
    let mut lexer = Lexer::new_stf(text);
    let mut tokens = vec![];
    while let Some(token) = lexer.next_token().unwrap() {
        tokens.push(match token {
            Token::LParen => "(".into(),
            Token::RParen => ")".into(),
            Token::String(s) | Token::Symbol(s) => s,
            Token::Integer(n) => n.to_string(),
            Token::Number(n) => n.to_string(),
        });
    }
    assert_eq!(
        serde_json::to_value(tokens).unwrap(),
        reference()["late_quote"]
    );
}
