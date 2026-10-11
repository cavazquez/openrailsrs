//! Captured from the unchanged OR 1.6.1 DLLs, not generated from Rust.
use openrailsrs_track::sigscript::{
    SignalContext, SignalProgram, activity_call_on, aggregate_signal_aspects,
};
use serde_json::Value;
fn reference() -> Value {
    serde_json::from_str(include_str!("../../../oracles/openrails-signals.json")).unwrap()
}
fn number(row: &Value, key: &str) -> u8 {
    row[key].as_u64().unwrap() as u8
}
#[test]
fn original_signal_function_aggregates() {
    let reference = reference();
    for row in reference["aggregate"].as_array().unwrap() {
        let function = number(row, "function");
        let heads = [
            (function, number(row, "a")),
            (function, number(row, "b")),
            ((function + 1) % 5, 0),
        ];
        let least = aggregate_signal_aspects(heads, function, false);
        let most = aggregate_signal_aspects(heads, function, true);
        assert_eq!(least, number(row, "least"), "{row}");
        assert_eq!(most, number(row, "most"), "{row}");
        let name = ["NORMAL", "DISTANCE", "REPEATER", "SHUNTING", "INFO"][function as usize];
        let program =
            SignalProgram::compile(&format!("state = NEXT_SIG_MR(SIGFN_{name});")).unwrap();
        let mut c = SignalContext::default();
        c.next_mr[function as usize] = most;
        assert_eq!(program.evaluate(c).unwrap().aspect, most);
    }
    assert_eq!(aggregate_signal_aspects([], 0, true), 0);
}
#[test]
fn original_activity_call_on_permissions() {
    for row in reference()["call_on"].as_array().unwrap() {
        let b = |key: &str| row[key].as_bool().unwrap();
        let result = activity_call_on(
            b("enabled"),
            b("first_signal"),
            b("route_exists"),
            b("leads_into_platform"),
            b("manually_allowed"),
            b("allow_non_platform"),
        );
        assert_eq!(result, b("result"), "{row}");
        let function = if b("allow_non_platform") {
            "TRAINHASCALLON"
        } else {
            "TRAINHASCALLON_RESTRICTED"
        };
        let program = SignalProgram::compile(&format!(
            "if ({function}()) state=SIGASP_CLEAR_2; else state=SIGASP_STOP;"
        ))
        .unwrap();
        let c = SignalContext {
            call_on: result,
            call_on_restricted: result,
            ..Default::default()
        };
        assert_eq!(
            program.evaluate(c).unwrap().aspect,
            if result { 7 } else { 0 }
        );
    }
}
#[test]
fn original_optional_heads_and_switch_stands() {
    let reference = reference();
    for row in reference["heads"].as_array().unwrap() {
        let program = SignalProgram::compile(&format!(
            "if (HASHEAD({})) state=SIGASP_CLEAR_2; else state=SIGASP_STOP;",
            row["index"]
        ))
        .unwrap();
        let result = program
            .evaluate(SignalContext {
                head_flags: Some(row["mask"].as_u64().unwrap() as u32),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(result.aspect, if row["result"] == 1 { 7 } else { 0 });
    }
    for row in reference["switches"].as_array().unwrap() {
        let program = SignalProgram::compile("state=SWITCHSTAND(6,2);").unwrap();
        let result = program
            .evaluate(SignalContext {
                switch_diverging: row["route"].as_i64().unwrap() > 0,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(result.aspect, number(row, "result"));
    }
}

#[test]
fn original_basic_signal_declared_aspects() {
    for row in reference()["basic"].as_array().unwrap() {
        let mask = row["mask"].as_u64().unwrap();
        let declared = (0..8u8)
            .filter(|aspect| mask & (1 << aspect) != 0)
            .collect::<Vec<_>>();
        let least = declared.iter().copied().max().unwrap_or(0);
        let most = declared.iter().copied().min().unwrap_or(0);
        let program = SignalProgram::compile(&format!(
            "if (block_state()==BLOCK_CLEAR) state={least}; else state={most};"
        ))
        .unwrap();
        for clear in [false, true] {
            let result = program
                .evaluate(SignalContext {
                    block_clear: clear,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(
                result.aspect,
                number(row, if clear { "least" } else { "most" }),
                "{row}"
            );
        }
    }
}
