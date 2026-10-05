use anyhow::{Context, bail};
use openrailsrs_ace::read_ace;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub fn cache_ktx2(input: &Path, root: Option<&Path>, rgba: bool) -> anyhow::Result<()> {
    let default_root = openrailsrs_bevy_scenery::texture_cache::cache_dir();
    let root = root.unwrap_or(&default_root);
    let input = input.canonicalize()?;
    let mut pending = vec![input];
    let mut reports = vec![];
    let formats = if rgba {
        bevy_formats(false)
    } else {
        bevy_formats(true)
    };
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_file() || kind.is_dir() {
                    pending.push(entry.path());
                }
            }
            continue;
        }
        if !path.extension().is_some_and(|e| {
            ["ace", "dds", "ktx2"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        }) {
            continue;
        }
        let now = std::time::Instant::now();
        match openrailsrs_bevy_scenery::texture_cache::load_in(&path,Some(root),formats,None,None) {
            Ok(result)=>reports.push(serde_json::json!({"source":path,"cache_hit":result.hit,"width":result.image.width(),"height":result.image.height(),"mips":result.image.texture_descriptor.mip_level_count,"format":format!("{:?}",result.image.texture_descriptor.format),"upload_bytes":result.image.data.as_ref().map(Vec::len),"elapsed_ms":now.elapsed().as_secs_f64()*1000.})),
            Err(error)=>reports.push(serde_json::json!({"source":path,"error":error})),
        }
    }
    if reports.is_empty() {
        bail!("No ACE, DDS or KTX2 textures found");
    }
    let failed = reports.iter().any(|r| r.get("error").is_some());
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"cache_directory":root,"lossless":true,"textures":reports})
        )?
    );
    if failed {
        bail!("Some textures could not be loaded; see the report");
    }
    Ok(())
}
fn bevy_formats(bc: bool) -> bevy::image::CompressedImageFormats {
    if bc {
        bevy::image::CompressedImageFormats::BC
    } else {
        bevy::image::CompressedImageFormats::NONE
    }
}

#[derive(Serialize)]
struct TextureExport {
    source: PathBuf,
    dds: PathBuf,
    format: &'static str,
    mips: usize,
    rgba_upload_bytes: usize,
    dds_upload_bytes: usize,
}

pub fn export_dds(input: &Path, output: &Path) -> anyhow::Result<()> {
    let input = input
        .canonicalize()
        .context("texture input does not exist")?;
    let root = if input.is_dir() {
        input.as_path()
    } else {
        input.parent().unwrap()
    };
    let mut pending = vec![input.clone()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_dir() || kind.is_file() {
                    pending.push(entry.path());
                }
            }
        } else if path
            .extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("ace"))
        {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        bail!("no ACE textures found in {}", input.display());
    }
    let mut report = Vec::new();
    for source in files {
        let ace = read_ace(&source).with_context(|| format!("decode {}", source.display()))?;
        let bytes = ace
            .to_dds()
            .with_context(|| format!("export {}", source.display()))?;
        let target = output
            .join(source.strip_prefix(root)?)
            .with_extension("dds");
        std::fs::create_dir_all(target.parent().unwrap())?;
        if target.exists() {
            if std::fs::read(&target)? != bytes {
                bail!(
                    "{} already exists with different content; choose another output directory",
                    target.display()
                );
            }
        } else {
            openrailsrs_ace::write_dds(&ace, &target)?;
        }
        let rgba = if ace.mips.is_empty() {
            ace.mip0.len()
        } else {
            ace.mips.iter().map(|m| m.rgba.len()).sum()
        };
        report.push(TextureExport {
            source,
            dds: target,
            format: if ace.compressed_mips.is_empty() {
                "RGBA8"
            } else {
                ace.format.as_str()
            },
            mips: ace.mips.len().max(1),
            rgba_upload_bytes: rgba,
            dds_upload_bytes: bytes.len() - 128,
        });
    }
    let rgba: usize = report.iter().map(|m| m.rgba_upload_bytes).sum();
    let dds: usize = report.iter().map(|m| m.dds_upload_bytes).sum();
    let summary = serde_json::json!({ "textures": report, "lossless": true,
        "rgba_upload_bytes": rgba, "dds_upload_bytes": dds,
        "saving_percent": (1.0 - dds as f64 / rgba as f64) * 100.0 });
    std::fs::write(
        output.join("dds-report.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    println!(
        "Exported {} lossless DDS textures to {}. Texture payload: {} → {} bytes ({:.1}% saved vs RGBA).",
        report.len(),
        output.display(),
        rgba,
        dds,
        summary["saving_percent"].as_f64().unwrap()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dds_export_preserves_source_and_reuses_only_identical_outputs() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-ace/tests/fixtures/dxt1_4x4.ace");
        let source = std::fs::read(&fixture).unwrap();
        let output =
            std::env::temp_dir().join(format!("openrailsrs-dds-export-{}", std::process::id()));
        std::fs::create_dir_all(&output).unwrap();
        export_dds(&fixture, &output).unwrap();
        let dds = output.join("dxt1_4x4.dds");
        let result = std::fs::read(&dds).unwrap();
        assert_eq!(result.len(), 128 + 8);
        assert_eq!(&result[84..88], b"DXT1");
        export_dds(&fixture, &output).unwrap();
        assert_eq!(std::fs::read(&fixture).unwrap(), source);
        std::fs::write(&dds, b"existing unrelated texture").unwrap();
        assert!(export_dds(&fixture, &output).is_err());
        assert_eq!(std::fs::read(&dds).unwrap(), b"existing unrelated texture");
        std::fs::remove_dir_all(output).unwrap();
    }
}
