//! Deterministic, replayable smoke campaigns; libFuzzer shares the same exercises.
use openrailsrs_fuzz::{MAX_INPUT, TARGETS, exercise, seeds};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn argument(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|p| p[0] == name).map(|p| p[1].clone())
}
fn random(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}
fn mutate(seed: &[u8], state: &mut u64, run: usize) -> Vec<u8> {
    if run == 0 {
        return seed.to_vec();
    }
    let mut data = seed.to_vec();
    for _ in 0..1 + random(state) % 4 {
        let n = random(state) as usize;
        match n % 5 {
            0 if !data.is_empty() => {
                let i = n % data.len();
                data[i] ^= (random(state) as u8) | 1;
            }
            1 if !data.is_empty() => data.truncate(n % (data.len() + 1)),
            2 if data.len() < MAX_INPUT => {
                let at = n % (data.len() + 1);
                data.insert(at, random(state) as u8);
            }
            3 if !data.is_empty() && data.len() < MAX_INPUT / 2 => {
                let block = data.clone();
                data.extend(block);
            }
            _ if !data.is_empty() => {
                let at = n % data.len();
                data[at] = [0, 0xff, b'(', b')', b'"', b'{', b'}'][n % 7];
            }
            _ => (),
        }
    }
    data.truncate(MAX_INPUT);
    data
}
fn record(
    out: &Path,
    target: &str,
    bytes: &[u8],
    run: usize,
    seed: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(out.join("last-input.bin"), bytes)?;
    std::fs::write(
        out.join("last-case.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"target":target,"run":run,"seed":seed,"input_sha256":format!("{:x}",Sha256::digest(bytes)),"max_input":MAX_INPUT,"package_version":env!("CARGO_PKG_VERSION")}),
        )?,
    )?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let target = argument(&args, "--target").unwrap_or("all".into());
    if target != "all" && !TARGETS.contains(&target.as_str()) {
        return Err("Unknown --target".into());
    }
    let runs = argument(&args, "--runs")
        .unwrap_or("256".into())
        .parse::<usize>()?;
    if runs == 0 || runs > 1_000_000 {
        return Err("--runs must be 1..1000000".into());
    }
    let seed = argument(&args, "--seed")
        .unwrap_or("1612026".into())
        .parse::<u64>()?;
    let out = PathBuf::from(argument(&args, "--out").unwrap_or("tmp/fuzz-regression".into()));
    std::fs::create_dir_all(&out)?;
    if out.join("last-input.bin").exists() || out.join("result.json").exists() {
        return Err(
            "Choose a fresh --out directory; previous reproduction inputs are preserved".into(),
        );
    }
    if let Some(input) = argument(&args, "--replay") {
        if target == "all" {
            return Err("Replay requires --target".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(input)?
            .take(MAX_INPUT as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_INPUT {
            return Err("Replay exceeds input budget".into());
        }
        record(&out, &target, &bytes, 0, seed)?;
        exercise(&target, &bytes);
        println!("PASS replay {target}");
        return Ok(());
    }
    let mut results = vec![];
    for name in TARGETS
        .into_iter()
        .filter(|n| target == "all" || *n == target)
    {
        let corpus = seeds(name);
        if let Some(directory) = argument(&args, "--export-corpus") {
            let path = PathBuf::from(directory).join(name);
            std::fs::create_dir_all(&path)?;
            for (i, bytes) in corpus.iter().enumerate() {
                std::fs::write(path.join(format!("seed-{i:03}")), bytes)?;
            }
        }
        let mut state = seed;
        let mut count = 0;
        for (i, original) in corpus.iter().enumerate() {
            for run in 0..runs {
                let bytes = mutate(original, &mut state, run);
                record(&out, name, &bytes, i * runs + run, seed)?;
                exercise(name, &bytes);
                count += 1;
            }
        }
        println!("PASS {name}: {count} own-seed mutations");
        results.push(serde_json::json!({"target":name,"cases":count}));
    }
    std::fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"status":"pass","seed":seed,"runs_per_seed":runs,"max_input":MAX_INPUT,"targets":results,"scope":"deterministic mutations, not a coverage-guided fuzz campaign"}),
        )?,
    )?;
    Ok(())
}
