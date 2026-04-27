use std::io::Read;
use std::path::Path;

use csv::ReaderBuilder;

use crate::ExportError;

/// Human-readable textual replay from a `run.csv` (first N rows).
pub fn textual_replay_from_csv(path: &Path, max_lines: usize) -> Result<String, ExportError> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(buf.as_bytes());
    let mut out = String::from("textual replay\n");
    for (i, rec) in rdr.records().enumerate() {
        if i >= max_lines {
            break;
        }
        let rec = rec?;
        out.push_str(&format!("{i}: {rec:?}\n"));
    }
    Ok(out)
}
