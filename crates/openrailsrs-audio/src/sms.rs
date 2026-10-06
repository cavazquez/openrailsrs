//! MSTS sound programs. The AST retains stream order: several threshold
//! crossings in one simulation step must execute in authored order.
use openrailsrs_formats::{Ast, Atom, parse_named_stf, read_msts_file_to_string};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Speed,
    Distance,
    Variable1,
    Variable2,
    Variable3,
    BrakeCylinder,
}

impl Control {
    fn parse(name: &str) -> Option<Self> {
        let name = name
            .to_ascii_lowercase()
            .replace("_inc_past", "")
            .replace("_dec_past", "");
        if name.starts_with("speed") {
            Some(Self::Speed)
        } else if name.starts_with("distance") {
            Some(Self::Distance)
        } else if name.starts_with("variable1") && !name.starts_with("variable1_") {
            Some(Self::Variable1)
        } else if name.starts_with("variable2") {
            Some(Self::Variable2)
        } else if name.starts_with("variable3") {
            Some(Self::Variable3)
        } else if name.starts_with("brakecyl") {
            Some(Self::BrakeCylinder)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct Curve {
    pub control: Control,
    pub points: Vec<(f32, f32)>,
}
impl Curve {
    pub fn value(&self, x: f32) -> f32 {
        let Some(&(first_x, first_y)) = self.points.first() else {
            return 1.0;
        };
        if self.points.len() == 1 || x < first_x {
            return first_y;
        }
        let &(last_x, last_y) = self.points.last().unwrap();
        if x > last_x {
            return last_y;
        }
        for pair in self.points.windows(2) {
            let [(x0, y0), (x1, y1)] = pair else {
                unreachable!()
            };
            if x <= *x1 {
                return y0 + (y1 - y0) * ((x - x0) / (x1 - x0).max(f32::EPSILON)).clamp(0.0, 1.0);
            }
        }
        self.points.last().unwrap().1
    }
}

#[derive(Clone, Debug)]
pub enum TriggerKind {
    Initial,
    Discrete(u32),
    Variable {
        control: Control,
        increasing: bool,
        threshold: f32,
    },
    Random {
        min_s: f32,
        max_s: f32,
    },
    Distance {
        min_m: f32,
        max_m: f32,
    },
}
#[derive(Clone, Debug)]
pub enum Command {
    Play {
        files: Vec<String>,
        looping: bool,
        release: bool,
        random: bool,
    },
    Release {
        jump: bool,
    },
    Volume(f32),
    Enable(usize),
    Disable(usize),
}
#[derive(Clone, Debug)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub commands: Vec<Command>,
}
#[derive(Clone, Debug, Default)]
pub struct Stream {
    pub volume: f32,
    pub volumes: Vec<Curve>,
    pub frequency: Option<Curve>,
    pub triggers: Vec<Trigger>,
}
#[derive(Clone, Debug)]
pub struct CameraActivation {
    pub cab: bool,
    pub passenger: bool,
    pub exterior: bool,
}
#[derive(Clone, Debug)]
pub struct SmsProgram {
    pub volume: f32,
    pub stereo: bool,
    pub ignore_3d: bool,
    /// Activation and deactivation form a hysteresis band in native SMS.
    /// Zero means no distance condition; OR still limits sources to 2 km.
    pub distance_m: f32,
    pub deactivation_distance_m: f32,
    pub cameras: Option<CameraActivation>,
    pub streams: Vec<Stream>,
    pub warnings: Vec<String>,
}

pub(crate) fn text(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(Atom::Symbol(s) | Atom::String(s)) => Some(s.clone()),
        Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
        Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}
fn num(ast: &Ast) -> Option<f32> {
    text(ast)?
        .trim_end_matches(',')
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
}
pub(crate) fn block(ast: &Ast, name: &str) -> bool {
    matches!(ast, Ast::List(items) if items.first().and_then(text).is_some_and(|s| s.eq_ignore_ascii_case(name)))
}
pub(crate) fn items(ast: &Ast) -> &[Ast] {
    if let Ast::List(items) = ast {
        items
    } else {
        &[]
    }
}
pub(crate) fn child<'a>(ast: &'a Ast, name: &str) -> Option<&'a Ast> {
    items(ast).iter().find(|a| block(a, name))
}
fn scalar(ast: &Ast, name: &str, default: f32) -> f32 {
    child(ast, name)
        .and_then(|a| items(a).get(1))
        .and_then(num)
        .unwrap_or(default)
}
fn flag(ast: &Ast, name: &str) -> bool {
    child(ast, name).is_some_and(|a| {
        items(a)
            .get(1)
            .and_then(text)
            .is_none_or(|s| s != "0" && !s.eq_ignore_ascii_case("false"))
    })
}
fn pair(ast: &Ast, name: &str, default: (f32, f32)) -> (f32, f32) {
    let Some(a) = child(ast, name) else {
        return default;
    };
    (
        items(a).get(1).and_then(num).unwrap_or(default.0),
        items(a).get(2).and_then(num).unwrap_or(default.1),
    )
}
fn curve(ast: &Ast, warnings: &mut Vec<String>) -> Option<Curve> {
    let name = items(ast).get(1).and_then(text)?;
    let Some(control) = Control::parse(&name) else {
        warnings.push(format!("Unsupported sound curve: {name}"));
        return None;
    };
    let points = child(ast, "CurvePoints")?;
    let count = items(points).get(1).and_then(num)? as usize;
    let values: Vec<_> = items(points).iter().skip(2).filter_map(num).collect();
    let points: Vec<_> = values
        .as_chunks::<2>()
        .0
        .iter()
        .take(count)
        .map(|p| {
            (
                if control == Control::Distance {
                    p[0] * p[0].abs()
                } else {
                    p[0]
                },
                p[1],
            )
        })
        .collect();
    // OR 1.6.1 Sound.cs::Interpolate retains the authored order and checks
    // the first/last bounds before interpolating. Negative descending tables
    // are present in real LUR/steam SMS files and must keep those semantics.
    if points.len() != count {
        warnings.push(format!(
            "Invalid sound curve: {name} ({count} points expected, got {points:?})"
        ));
        return None;
    }
    Some(Curve { control, points })
}

impl SmsProgram {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let source = read_msts_file_to_string(path).map_err(|e| e.to_string())?;
        Self::parse(&source)
    }
    pub fn parse(source: &str) -> Result<Self, String> {
        let ast = parse_named_stf(source).map_err(|e| e.to_string())?;
        let root = if block(&ast, "Tr_SMS") {
            &ast
        } else {
            child(&ast, "Tr_SMS").ok_or("Missing Tr_SMS")?
        };
        let group = items(root)
            .iter()
            .filter(|a| block(a, "ScalabiltyGroup") || block(a, "ScalabilityGroup"))
            .max_by_key(|a| items(a).get(1).and_then(num).unwrap_or(0.0) as i32)
            .ok_or("Missing ScalabiltyGroup")?;
        let mut result = Self {
            volume: scalar(group, "Volume", 1.0),
            stereo: flag(group, "Stereo"),
            ignore_3d: flag(group, "Ignore3D"),
            distance_m: child(group, "Activation").map_or(0.0, |a| scalar(a, "Distance", 1000.0)),
            deactivation_distance_m: child(group, "Deactivation")
                .map_or(0.0, |a| scalar(a, "Distance", 1000.0)),
            cameras: child(group, "Activation").map(|a| CameraActivation {
                cab: flag(a, "CabCam"),
                passenger: flag(a, "PassengerCam"),
                exterior: flag(a, "ExternalCam"),
            }),
            streams: vec![],
            warnings: vec![],
        };
        let streams = child(group, "Streams").ok_or("Missing Streams")?;
        for source in items(streams).iter().filter(|a| block(a, "Stream")) {
            let mut stream = Stream {
                volume: scalar(source, "Volume", 1.0),
                ..Default::default()
            };
            for entry in items(source) {
                if block(entry, "VolumeCurve") {
                    if let Some(c) = curve(entry, &mut result.warnings) {
                        stream.volumes.push(c);
                    }
                } else if block(entry, "FrequencyCurve") {
                    stream.frequency = curve(entry, &mut result.warnings);
                }
            }
            if let Some(triggers) = child(source, "Triggers") {
                for trigger in items(triggers).iter().filter(|a| matches!(a, Ast::List(_))) {
                    let Some(name) = items(trigger).first().and_then(text) else {
                        continue;
                    };
                    let kind = match name.to_ascii_lowercase().as_str() {
                        "initial_trigger" => Some(TriggerKind::Initial),
                        "discrete_trigger" => items(trigger)
                            .get(1)
                            .and_then(num)
                            .map(|n| TriggerKind::Discrete(n as u32)),
                        "variable_trigger" => {
                            let name = items(trigger).get(1).and_then(text).unwrap_or_default();
                            Control::parse(&name)
                                .zip(items(trigger).get(2).and_then(num))
                                .map(|(control, threshold)| TriggerKind::Variable {
                                    control,
                                    increasing: name.to_ascii_lowercase().contains("inc_past"),
                                    // SoundManagmentFile.Variable_Trigger stores
                                    // Distance thresholds squared, as it does
                                    // for DistanceControlled curve coordinates.
                                    threshold: if control == Control::Distance {
                                        threshold * threshold
                                    } else {
                                        threshold
                                    },
                                })
                        }
                        "random_trigger" => {
                            let (min_s, max_s) = pair(trigger, "Delay_Min_Max", (80.0, 100.0));
                            Some(TriggerKind::Random { min_s, max_s })
                        }
                        "dist_travelled_trigger" => {
                            let (min_m, max_m) = pair(trigger, "Dist_Min_Max", (80.0, 100.0));
                            Some(TriggerKind::Distance { min_m, max_m })
                        }
                        "skip" | "comment" => continue,
                        _ => None,
                    };
                    let Some(kind) = kind else {
                        result
                            .warnings
                            .push(format!("Unsupported sound trigger: {name}"));
                        continue;
                    };
                    let mut commands = vec![];
                    for cmd in items(trigger).iter().filter(|a| matches!(a, Ast::List(_))) {
                        let name = items(cmd)
                            .first()
                            .and_then(text)
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        match name.as_str() {
                            "playoneshot" | "startloop" | "startlooprelease" => {
                                let files = items(cmd)
                                    .iter()
                                    .filter(|a| block(a, "File"))
                                    .filter_map(|a| items(a).get(1).and_then(text))
                                    .collect();
                                let random = child(cmd, "SelectionMethod")
                                    .and_then(|a| items(a).get(1))
                                    .and_then(text)
                                    .is_some_and(|s| s.eq_ignore_ascii_case("RandomSelection"));
                                commands.push(Command::Play {
                                    files,
                                    looping: name != "playoneshot",
                                    release: name == "startlooprelease",
                                    random,
                                });
                            }
                            "releaselooprelease" | "releaseloopreleasewithjump" => {
                                commands.push(Command::Release {
                                    jump: name.ends_with("withjump"),
                                })
                            }
                            "setvolume" | "setstreamvolume" => commands.push(Command::Volume(
                                items(cmd).get(1).and_then(num).unwrap_or(1.0),
                            )),
                            "enabletrigger" | "disabletrigger" => {
                                if let Some(index) = items(cmd).get(1).and_then(num) {
                                    commands.push(if name == "enabletrigger" {
                                        Command::Enable((index as usize).saturating_sub(1))
                                    } else {
                                        Command::Disable((index as usize).saturating_sub(1))
                                    })
                                }
                            }
                            "skip" | "comment" | "delay_min_max" | "dist_min_max"
                            | "volume_min_max" => {}
                            _ => result
                                .warnings
                                .push(format!("Unsupported sound command: {name}")),
                        }
                    }
                    stream.triggers.push(Trigger { kind, commands });
                }
            }
            result.streams.push(stream);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_distance_and_boolean_defaults_match_the_pinned_parser() {
        let fixture = include_str!("../../../oracles/fixtures/audio-distance.sms");
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-audio.json")).unwrap();
        let highest = SmsProgram::parse(fixture).unwrap();
        let expected = &reference["groups"][0];
        assert_eq!(
            highest.distance_m,
            expected["activation_m"].as_f64().unwrap() as f32
        );
        assert_eq!(
            highest.deactivation_distance_m,
            expected["deactivation_m"].as_f64().unwrap() as f32
        );
        assert_eq!(highest.stereo, expected["stereo"].as_bool().unwrap());
        assert_eq!(highest.ignore_3d, expected["ignore_3d"].as_bool().unwrap());
        assert!(!highest.cameras.unwrap().cab);
        assert!(highest.warnings.is_empty(), "{:?}", highest.warnings);
        for (trigger, threshold) in highest.streams[0]
            .triggers
            .iter()
            .zip(reference["distance_thresholds_squared"].as_array().unwrap())
        {
            assert!(
                matches!(trigger.kind, TriggerKind::Variable { control: Control::Distance, threshold: actual, .. } if actual == threshold.as_f64().unwrap() as f32)
            );
        }
        let defaults = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 4 Stereo () Ignore3D () Activation ( ExternalCam () ) Deactivation () Streams (0) ) )").unwrap();
        let expected = &reference["groups"][1];
        assert_eq!(
            defaults.distance_m,
            expected["activation_m"].as_f64().unwrap() as f32
        );
        assert_eq!(
            defaults.deactivation_distance_m,
            expected["deactivation_m"].as_f64().unwrap() as f32
        );
        assert_eq!(defaults.stereo, expected["stereo"].as_bool().unwrap());
        assert_eq!(defaults.ignore_3d, expected["ignore_3d"].as_bool().unwrap());
    }
    #[test]
    fn native_negative_tables_keep_openrails_endpoint_semantics() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( VolumeCurve ( Variable2Controlled CurvePoints ( 3 0 0 -5 .05 -100 .8 ) ) FrequencyCurve ( Variable1Controlled CurvePoints ( 3 0 11025 -35 11025 -50 14000 ) ) Triggers ( 0 ) ) ) ) )").unwrap();
        assert!(program.warnings.is_empty());
        let volume = &program.streams[0].volumes[0];
        assert_eq!(volume.value(-1.0), 0.0);
        assert_eq!(volume.value(0.0), 0.8);
        assert_eq!(volume.value(50.0), 0.8);
        assert_eq!(
            program.streams[0].frequency.as_ref().unwrap().value(10.0),
            14000.0
        );
    }
    #[test]
    fn native_comma_separators_preserve_all_curve_points() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( VolumeCurve ( SpeedControlled CurvePoints ( 3 0, 0 .1, .7 20, 1 ) ) FrequencyCurve ( SpeedControlled CurvePoints ( 3 -50.0, 13000 0.0, 9500 50.0, 13000 ) ) Triggers ( 0 ) ) ) ) )").unwrap();
        assert!(program.warnings.is_empty(), "{:?}", program.warnings);
        assert_eq!(program.streams[0].volumes[0].value(0.1), 0.7);
        assert_eq!(
            program.streams[0].frequency.as_ref().unwrap().value(0.0),
            9500.0
        );
    }
    #[test]
    fn highest_detail_curves_and_horn_events_keep_authored_order() {
        let program=SmsProgram::parse(r#"Tr_SMS ( ScalabiltyGroup ( 1 Streams ( 0 ) ) ScalabiltyGroup ( 5 Volume ( .25 ) Streams ( 1 Stream ( VolumeCurve ( SpeedControlled CurvePoints ( 3 0 0 10 .5 20 1 ) ) Triggers ( 3 Initial_Trigger ( StartLoop ( 1 File ( "idle.wav" -1 ) ) ) Discrete_Trigger ( 8 StartLoopRelease ( 1 File ( "horn.wav" -1 ) ) ) Discrete_Trigger ( 9 ReleaseLoopReleaseWithJump ( ) ) ) ) ) ) )"#).unwrap();
        assert_eq!(program.volume, 0.25);
        assert_eq!(program.streams.len(), 1);
        assert_eq!(program.streams[0].volumes[0].value(15.0), 0.75);
        assert!(matches!(
            program.streams[0].triggers[2].commands[0],
            Command::Release { jump: true }
        ));
    }
}
