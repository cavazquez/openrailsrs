//! Bounded interpreter for the original MSTS SIGSCR language used by Chiltern.
//! Unknown instructions/functions are errors; a caller must keep a stop aspect.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeSignalDef {
    pub name: String,
    pub function: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct SignalProgram(Vec<Statement>);
#[derive(Clone, Debug)]
enum Statement {
    Assign(String, Expr),
    If(Expr, Vec<Statement>, Vec<Statement>),
}
#[derive(Clone, Debug)]
enum Expr {
    Value(f64),
    Var(String),
    Call(String, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug)]
pub struct SignalContext {
    pub enabled: bool,
    pub route_set: bool,
    pub block_clear: bool,
    pub next_normal: u8,
    pub distant_normal: u8,
    pub this_normal: u8,
    pub draw_states: [i32; 8],
}
impl Default for SignalContext {
    fn default() -> Self {
        Self {
            enabled: true,
            route_set: true,
            block_clear: true,
            next_normal: 7,
            distant_normal: 7,
            this_normal: 7,
            draw_states: [0, 0, 0, 1, 1, 1, 2, 2],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignalResult {
    pub aspect: u8,
    pub draw_state: i32,
}

fn tokens(source: &str) -> Result<Vec<String>, String> {
    if source.len() > 256 * 1024 {
        return Err("SIGSCR too large".into());
    }
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut out = vec![];
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() || c == b'#' {
            i += 1;
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            i += 2;
            while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                i += 1;
            }
            if i + 1 == bytes.len() {
                return Err("unterminated comment".into());
            }
            i += 2;
            continue;
        }
        let start = i;
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
        } else if [b"==", b"!=", b"<=", b">=", b"&&", b"||"]
            .iter()
            .any(|op| bytes.get(i..i + 2) == Some(*op))
        {
            i += 2;
        } else if b"(){};,=<>!+-*/".contains(&c) {
            i += 1;
        } else {
            return Err(format!("unsupported SIGSCR byte {c}"));
        }
        out.push(source[start..i].to_ascii_uppercase());
        if out.len() > 20000 {
            return Err("too many SIGSCR tokens".into());
        }
    }
    Ok(out)
}
struct Parser {
    tokens: Vec<String>,
    i: usize,
    depth: usize,
}
impl Parser {
    fn peek(&self) -> &str {
        self.tokens.get(self.i).map_or("", String::as_str)
    }
    fn take(&mut self) -> String {
        let s = self.peek().to_owned();
        self.i += 1;
        s
    }
    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.peek() != s {
            return Err(format!("expected {s}, got {}", self.peek()));
        }
        self.i += 1;
        Ok(())
    }
    fn statements(&mut self, block: bool) -> Result<Vec<Statement>, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("SIGSCR nesting limit".into());
        }
        let mut result = vec![];
        while !self.peek().is_empty() && self.peek() != "}" {
            if matches!(self.peek(), "EXTERN" | "FLOAT") {
                while self.peek() != ";" {
                    if self.peek().is_empty() {
                        return Err("unterminated declaration".into());
                    }
                    self.i += 1;
                }
                self.i += 1;
                continue;
            }
            result.push(self.statement()?);
        }
        if block {
            self.expect("}")?;
        }
        self.depth -= 1;
        Ok(result)
    }
    fn branch(&mut self) -> Result<Vec<Statement>, String> {
        if self.peek() == "{" {
            self.i += 1;
            self.statements(true)
        } else {
            self.depth += 1;
            if self.depth > 64 {
                return Err("SIGSCR branch nesting limit".into());
            }
            let statement = self.statement()?;
            self.depth -= 1;
            Ok(vec![statement])
        }
    }
    fn statement(&mut self) -> Result<Statement, String> {
        if self.peek() == "IF" {
            self.i += 1;
            self.expect("(")?;
            let e = self.expr(0)?;
            self.expect(")")?;
            let yes = self.branch()?;
            let no = if self.peek() == "ELSE" {
                self.i += 1;
                self.branch()?
            } else {
                vec![]
            };
            Ok(Statement::If(e, yes, no))
        } else {
            let name = self.take();
            if !name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
                return Err("expected variable".into());
            }
            self.expect("=")?;
            let e = self.expr(0)?;
            self.expect(";")?;
            Ok(Statement::Assign(name, e))
        }
    }
    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let start = self.i;
        self.depth += 1;
        if self.depth > 64 {
            return Err("SIGSCR expression nesting limit".into());
        }
        let token = self.take();
        let mut lhs = match token.as_str() {
            "!" | "-" | "+" => Expr::Unary(token, Box::new(self.expr(7)?)),
            "(" => {
                let e = self.expr(0)?;
                self.expect(")")?;
                e
            }
            "" => return Err("missing expression".into()),
            _ => {
                if let Ok(v) = token.parse::<f64>() {
                    Expr::Value(v)
                } else if self.peek() == "(" {
                    self.i += 1;
                    let mut args = vec![];
                    while self.peek() != ")" {
                        args.push(self.expr(0)?);
                        if self.peek() != "," {
                            break;
                        }
                        self.i += 1;
                    }
                    self.expect(")")?;
                    Expr::Call(token, args)
                } else {
                    Expr::Var(token)
                }
            }
        };
        loop {
            // Flat operators also build a nested AST. Bound its size before
            // evaluation/drop, independently of syntactic parentheses.
            if self.i - start > 256 {
                return Err("SIGSCR expression size limit".into());
            }
            let precedence = match self.peek() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" => 3,
                "<" | ">" | "<=" | ">=" => 4,
                "+" | "-" => 5,
                "*" | "/" => 6,
                _ => 0,
            };
            if precedence == 0 || precedence < min {
                break;
            }
            let op = self.take();
            let rhs = self.expr(precedence + 1)?;
            if self.i - start > 256 {
                return Err("SIGSCR expression size limit".into());
            }
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        self.depth -= 1;
        Ok(lhs)
    }
}
fn constant(name: &str) -> Option<f64> {
    Some(match name {
        "SIGASP_STOP" => 0.,
        "SIGASP_STOP_AND_PROCEED" => 1.,
        "SIGASP_RESTRICTING" => 2.,
        "SIGASP_APPROACH_1" => 3.,
        "SIGASP_APPROACH_2" => 4.,
        "SIGASP_APPROACH_3" => 5.,
        "SIGASP_CLEAR_1" => 6.,
        "SIGASP_CLEAR_2" => 7.,
        "BLOCK_CLEAR" | "SIGFN_NORMAL" => 0.,
        "SIGFN_DISTANCE" => 1.,
        _ => return None,
    })
}
fn evaluate(e: &Expr, vars: &HashMap<String, f64>, c: SignalContext) -> Result<f64, String> {
    Ok(match e {
        Expr::Value(v) => *v,
        Expr::Var(n) => constant(n)
            .or_else(|| vars.get(n).copied())
            .ok_or_else(|| format!("unknown variable {n}"))?,
        Expr::Unary(op, e) => {
            let v = evaluate(e, vars, c)?;
            match op.as_str() {
                "!" => f64::from(v == 0.),
                "-" => -v,
                _ => v,
            }
        }
        Expr::Binary(op, a, b) => {
            let a = evaluate(a, vars, c)?;
            if op == "&&" && a == 0. {
                return Ok(0.);
            }
            if op == "||" && a != 0. {
                return Ok(1.);
            }
            let b = evaluate(b, vars, c)?;
            match op.as_str() {
                "==" => f64::from(a == b),
                "!=" => f64::from(a != b),
                "<" => f64::from(a < b),
                ">" => f64::from(a > b),
                "<=" => f64::from(a <= b),
                ">=" => f64::from(a >= b),
                "&&" => f64::from(b != 0.),
                "||" => f64::from(b != 0.),
                "+" => a + b,
                "-" => a - b,
                "*" => a * b,
                "/" if b != 0. => a / b,
                _ => return Err("invalid arithmetic".into()),
            }
        }
        Expr::Call(name, args) => {
            let args = args
                .iter()
                .map(|a| evaluate(a, vars, c))
                .collect::<Result<Vec<_>, _>>()?;
            match (name.as_str(), args.as_slice()) {
                ("BLOCK_STATE", []) => f64::from(!c.block_clear),
                ("ROUTE_SET", []) => f64::from(c.route_set),
                ("NEXT_SIG_LR", [0.]) => f64::from(c.next_normal),
                ("DIST_MULTI_SIG_MR", [0., 1.]) => f64::from(c.distant_normal),
                ("THIS_SIG_LR", [0.]) => f64::from(c.this_normal),
                ("DEF_DRAW_STATE", [v]) if v.fract() == 0. && (0.0..8.0).contains(v) => {
                    f64::from(c.draw_states[*v as usize])
                }
                _ => return Err(format!("unsupported function {name}{args:?}")),
            }
        }
    })
}
fn run(
    statements: &[Statement],
    vars: &mut HashMap<String, f64>,
    c: SignalContext,
    budget: &mut usize,
) -> Result<(), String> {
    for s in statements {
        *budget = budget.checked_sub(1).ok_or("SIGSCR execution limit")?;
        match s {
            Statement::Assign(n, e) => {
                let v = evaluate(e, vars, c)?;
                if !v.is_finite() {
                    return Err("nonfinite result".into());
                }
                vars.insert(n.clone(), v);
            }
            Statement::If(e, yes, no) => run(
                if evaluate(e, vars, c)? != 0. { yes } else { no },
                vars,
                c,
                budget,
            )?,
        }
    }
    Ok(())
}
impl SignalProgram {
    pub fn compile(source: &str) -> Result<Self, String> {
        let mut parser = Parser {
            tokens: tokens(source)?,
            i: 0,
            depth: 0,
        };
        let statements = parser.statements(false)?;
        if !parser.peek().is_empty() {
            return Err("unmatched closing brace".into());
        }
        Ok(Self(statements))
    }
    pub fn evaluate(&self, context: SignalContext) -> Result<SignalResult, String> {
        let mut vars = HashMap::from([
            ("ENABLED".into(), f64::from(context.enabled)),
            ("STATE".into(), 0.),
            ("DRAW_STATE".into(), 0.),
        ]);
        run(&self.0, &mut vars, context, &mut 4096)?;
        let aspect = vars["STATE"];
        let draw = vars["DRAW_STATE"];
        if aspect.fract() != 0.
            || !(0.0..8.0).contains(&aspect)
            || draw.fract() != 0.
            || !(-1.0..=255.0).contains(&draw)
        {
            return Err("invalid signal result".into());
        }
        Ok(SignalResult {
            aspect: aspect as u8,
            draw_state: draw as i32,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const HOME: &str = "extern float block_state(); extern float enabled; if (!enabled || block_state() !=# BLOCK_CLEAR || !route_set()) {state=SIGASP_STOP;} else {state=SIGASP_CLEAR_2;} draw_state=def_draw_state(state);";
    #[test]
    fn native_home_blocks_on_occupancy_disabled_or_wrong_route() {
        let p = SignalProgram::compile(HOME).unwrap();
        assert_eq!(
            p.evaluate(SignalContext::default()).unwrap(),
            SignalResult {
                aspect: 7,
                draw_state: 2
            }
        );
        for c in [
            SignalContext {
                block_clear: false,
                ..Default::default()
            },
            SignalContext {
                enabled: false,
                ..Default::default()
            },
            SignalContext {
                route_set: false,
                ..Default::default()
            },
        ] {
            assert_eq!(p.evaluate(c).unwrap().aspect, 0);
        }
    }
    #[test]
    fn distant_and_four_aspect_scripts_preserve_native_states() {
        let p=SignalProgram::compile("// comment\nif(next_sig_lr(SIGFN_NORMAL)==#SIGASP_STOP){state=SIGASP_APPROACH_1;}else if(next_sig_lr(SIGFN_NORMAL)==#SIGASP_APPROACH_1){state=SIGASP_APPROACH_2;}else{state=SIGASP_CLEAR_2;} draw_state=def_draw_state(state);").unwrap();
        for (next, want) in [(0, 3), (3, 4), (4, 7), (7, 7)] {
            assert_eq!(
                p.evaluate(SignalContext {
                    next_normal: next,
                    ..Default::default()
                })
                .unwrap()
                .aspect,
                want
            );
        }
    }
    #[test]
    fn unsupported_calls_and_malformed_inputs_never_silently_clear() {
        assert!(SignalProgram::compile("while(1){state=7;}").is_err());
        assert!(SignalProgram::compile("if (((").is_err());
        let p = SignalProgram::compile("state=unsupported();").unwrap();
        assert!(p.evaluate(SignalContext::default()).is_err());
        assert!(SignalProgram::compile(&"(".repeat(300)).is_err());
    }
    #[test]
    fn unbraced_branches_and_flat_expressions_have_stack_safe_limits() {
        assert!(SignalProgram::compile(&format!("{}state=7;", "if(1)".repeat(500))).is_err());
        assert!(SignalProgram::compile(&format!("state={}0;", "1+".repeat(2000))).is_err());
    }
}
