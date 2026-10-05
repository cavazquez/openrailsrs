//! Optional out-of-process OR-style C# TCS. JSONL v1, SI units, one bounded RPC per tick.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{EtcsSupervision, EtcsTcsStatus, TextMessage};

const MAX_LINE: u64 = 65536;

#[derive(Clone, Debug)]
pub struct ScriptHostConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub script: PathBuf,
    pub type_name: String,
    pub timeout: Duration,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TcsInput {
    Acknowledge { message: String },
    Menu { action: String },
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ScriptContext {
    pub time_s: f64,
    pub dt_s: f64,
    pub speed_mps: f64,
    pub speed_limit_mps: f64,
    pub next_signal_distance_m: Option<f64>,
    pub next_signal_stop: bool,
    pub next_stop_distance_m: Option<f64>,
    pub train_max_speed_mps: f64,
    pub current_post_speed_limit_mps: f64,
    pub signals: Vec<ScriptSignal>,
    pub distance_signal: Option<ScriptSignal>,
    pub speed_posts: Vec<ScriptSpeedPost>,
}

/// Native SIGASP values, not the differently ordered OR TCS `Aspect` enum.
#[derive(Clone, Debug, Serialize)]
pub struct ScriptSignal {
    pub distance_m: f64,
    pub aspect: u8,
}
#[derive(Clone, Debug, Serialize)]
pub struct ScriptSpeedPost {
    pub distance_m: f64,
    pub speed_limit_mps: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    version: u32,
    seq: u64,
    status: Option<ScriptOutput>,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptOutput {
    allowed_mps: f64,
    next_limit_mps: Option<f64>,
    intervention_mps: f64,
    emergency_brake: bool,
    full_brake: bool,
    messages: Vec<ScriptMessage>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptMessage {
    text: String,
    acknowledgeable: bool,
    acknowledged: bool,
}

pub struct ScriptTcsHost {
    child: Child,
    requests: SyncSender<Vec<u8>>,
    writes: Mutex<Receiver<Result<(), String>>>,
    replies: Mutex<Receiver<Result<String, String>>>,
    timeout: Duration,
    seq: u64,
    last: Option<ScriptOutput>,
    error: Option<String>,
    events: Vec<TcsInput>,
}

impl ScriptTcsHost {
    pub fn launch(config: &ScriptHostConfig, context: &ScriptContext) -> Result<Self, String> {
        if config.timeout.is_zero() || config.timeout > Duration::from_secs(10) {
            return Err("TCS timeout must be in (0, 10s]".into());
        }
        let script = config
            .script
            .canonicalize()
            .map_err(|e| format!("TCS script: {e}"))?;
        let mut child = Command::new(&config.executable)
            .args(&config.arguments)
            .args([
                "--script",
                script.to_str().ok_or("Non-UTF8 script path")?,
                "--type",
                &config.type_name,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("TCS host launch: {e}"))?;
        let mut input = child.stdin.take().ok_or("TCS stdin missing")?;
        let output = child.stdout.take().ok_or("TCS stdout missing")?;
        // A stalled script may stop consuming stdin as well as stop replying.
        // Keep pipe writes off the simulation thread so its deadline covers both.
        let (requests, incoming) = mpsc::sync_channel::<Vec<u8>>(1);
        let (written, writes) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(bytes) = incoming.recv() {
                let result = input
                    .write_all(&bytes)
                    .and_then(|_| input.flush())
                    .map_err(|e| format!("TCS host write: {e}"));
                let failed = result.is_err();
                if written.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let (tx, replies) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = String::new();
                let result = reader.by_ref().take(MAX_LINE + 1).read_line(&mut line);
                let result = match result {
                    Ok(0) => Err("TCS host exited / EOF".into()),
                    Ok(n) if n as u64 > MAX_LINE || !line.ends_with('\n') => {
                        Err("TCS reply exceeds 64 KiB or is truncated".into())
                    }
                    Ok(_) => Ok(line),
                    Err(e) => Err(format!("TCS host read: {e}")),
                };
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let mut host = Self {
            child,
            requests,
            writes: Mutex::new(writes),
            replies: Mutex::new(replies),
            timeout: config.timeout,
            seq: 0,
            last: None,
            error: None,
            events: Vec::new(),
        };
        // Compilation/initialization gets a separate bounded deadline.
        host.exchange("initialize", context, Duration::from_secs(10))?;
        Ok(host)
    }

    pub fn push_input(&mut self, input: TcsInput) {
        if self.events.len() < 64 && self.error.is_none() {
            self.events.push(input);
        }
    }

    pub fn tick(&mut self, context: &ScriptContext) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.exchange("tick", context, self.timeout) {
            self.error = Some(error);
            let _ = self.child.kill();
        }
    }

    fn exchange(
        &mut self,
        kind: &str,
        context: &ScriptContext,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        self.seq += 1;
        let request = serde_json::json!({"version":1,"seq":self.seq,"kind":kind,"context":context,"events":std::mem::take(&mut self.events)});
        let mut bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        if bytes.len() >= MAX_LINE as usize {
            return Err("TCS request exceeds 64 KiB".into());
        }
        bytes.push(b'\n');
        self.requests
            .try_send(bytes)
            .map_err(|e| format!("TCS request queue: {e}"))?;
        self.writes
            .lock()
            .map_err(|_| "TCS write lock poisoned")?
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|e| format!("TCS write deadline: {e}"))??;
        let line = self
            .replies
            .lock()
            .map_err(|_| "TCS reply lock poisoned")?
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|e| format!("TCS response deadline: {e}"))??;
        self.last = Some(parse_reply(&line, self.seq)?);
        Ok(())
    }

    pub fn applies_brake(&self) -> bool {
        self.error.is_some()
            || self
                .last
                .as_ref()
                .is_some_and(|s| s.emergency_brake || s.full_brake)
    }

    pub fn status(&self, mut base: EtcsTcsStatus) -> EtcsTcsStatus {
        if let Some(error) = &self.error {
            base.allowed_kmh = 0.0;
            base.target_kmh = Some(0.0);
            base.supervision = EtcsSupervision::Intervention;
            base.messages.push(TextMessage {
                text: format!("TCS host failure: {error}"),
                acknowledgeable: false,
                acknowledged: false,
            });
        } else if let Some(output) = &self.last {
            base.allowed_kmh = output.allowed_mps * 3.6;
            base.target_kmh = output.next_limit_mps.map(|v| v * 3.6);
            base.intervention_kmh = output.intervention_mps * 3.6;
            base.overspeed = base.speed_kmh > base.allowed_kmh;
            base.supervision = if self.applies_brake() {
                EtcsSupervision::Intervention
            } else if base.overspeed {
                EtcsSupervision::Overspeed
            } else {
                EtcsSupervision::Normal
            };
            base.messages = output
                .messages
                .iter()
                .map(|m| TextMessage {
                    text: m.text.clone(),
                    acknowledgeable: m.acknowledgeable,
                    acknowledged: m.acknowledged,
                })
                .collect();
        }
        base.needs_ack = base
            .messages
            .iter()
            .any(|m| m.acknowledgeable && !m.acknowledged);
        base
    }
}

impl Drop for ScriptTcsHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse_reply(line: &str, seq: u64) -> Result<ScriptOutput, String> {
    let reply: Reply = serde_json::from_str(line).map_err(|e| format!("Invalid TCS JSON: {e}"))?;
    if reply.version != 1 || reply.seq != seq {
        return Err("TCS version / sequence mismatch".into());
    }
    if let Some(error) = reply.error {
        return Err(format!("C# TCS: {error}"));
    }
    let status = reply.status.ok_or("TCS status missing")?;
    let valid_speed = |v: f64| v.is_finite() && (0.0..=200.0).contains(&v);
    if !valid_speed(status.allowed_mps)
        || !valid_speed(status.intervention_mps)
        || status.next_limit_mps.is_some_and(|v| !valid_speed(v))
        || status.messages.len() > 32
        || status.messages.iter().any(|m| m.text.len() > 1024)
    {
        return Err("TCS output outside protocol limits".into());
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_rejects_invalid_numbers_versions_and_sequences() {
        let valid = r#"{"version":1,"seq":2,"status":{"allowed_mps":10,"next_limit_mps":null,"intervention_mps":12,"emergency_brake":false,"full_brake":false,"messages":[]},"error":null}"#;
        assert!(parse_reply(valid, 2).is_ok());
        assert!(parse_reply(valid, 3).is_err());
        assert!(parse_reply(&valid.replace("\"version\":1", "\"version\":2"), 2).is_err());
        assert!(
            parse_reply(
                &valid.replace("\"allowed_mps\":10", "\"allowed_mps\":-1"),
                2
            )
            .is_err()
        );
        assert!(
            parse_reply(
                &valid.replace("\"allowed_mps\":10", "\"allowed_mps\":1e999"),
                2
            )
            .is_err()
        );
        assert!(parse_reply("logging mixed into protocol", 2).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn host_death_timeout_and_sequence_failure_apply_brake_without_rust_fallback() {
        for behavior in ["exit", "sleep", "stop_reading", "wrong_seq", "oversize"] {
            let script = tempfile::NamedTempFile::new().unwrap();
            let helper = format!(
                r#"
import sys,json,time
r=json.loads(sys.stdin.readline())
print(json.dumps({{'version':1,'seq':r['seq'],'status':{{'allowed_mps':10,'next_limit_mps':None,'intervention_mps':12,'emergency_brake':False,'full_brake':False,'messages':[]}},'error':None}}),flush=True)
if '{behavior}' == 'stop_reading': time.sleep(3); sys.exit(0)
sys.stdin.readline()
if '{behavior}' == 'sleep': time.sleep(3)
if '{behavior}' == 'wrong_seq': print('{{"version":1,"seq":999,"status":null,"error":null}}',flush=True)
if '{behavior}' == 'oversize': print('x'*70000,flush=True)
"#
            );
            let context = ScriptContext {
                time_s: 0.0,
                dt_s: 0.05,
                speed_mps: 0.0,
                speed_limit_mps: 10.0,
                next_signal_distance_m: None,
                next_signal_stop: false,
                next_stop_distance_m: None,
                train_max_speed_mps: 30.,
                ..Default::default()
            };
            let config = ScriptHostConfig {
                executable: "python3".into(),
                arguments: vec!["-c".into(), helper],
                script: script.path().into(),
                type_name: "Unused".into(),
                timeout: Duration::from_millis(40),
            };
            let mut host = ScriptTcsHost::launch(&config, &context).unwrap();
            assert!(!host.applies_brake());
            if behavior == "stop_reading" {
                // Fill the pipe without exceeding the protocol's 64 KiB bound.
                for _ in 0..48 {
                    host.push_input(TcsInput::Acknowledge {
                        message: "x".repeat(1024),
                    });
                }
            }
            let started = Instant::now();
            host.tick(&context);
            assert!(started.elapsed() < Duration::from_secs(1), "{behavior}");
            assert!(host.applies_brake(), "{behavior}");
            let base = super::super::BasicEtcsTcs::default()
                .compute_from_inputs(0.0, 36.0, false, None, None);
            let status = host.status(base);
            assert_eq!(status.allowed_kmh, 0.0);
            assert_eq!(status.supervision, EtcsSupervision::Intervention);
            assert!(
                status
                    .messages
                    .iter()
                    .any(|m| m.text.contains("TCS host failure"))
            );
        }
    }
}
