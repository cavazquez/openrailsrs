//! Runtime downloads live outside the application and its Git checkout.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};
pub const CATALOG: &str = include_str!("../../../docs/fixtures/content/official-catalog.json");
const INSTALLER: &[u8] = include_bytes!("../../../scripts/download_official_content.py");

/// Resources of a moved binary/Snap, with source-checkout fallback for developers.
pub fn resource_dir(fallback: &Path) -> PathBuf {
    if let Some(root) = std::env::var_os("OPENRAILSRS_RESOURCES")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
    {
        return root;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
    {
        for root in [bin.join("../share/openrailsrs"), bin.join("resources")] {
            if root.join("assets/shaders").is_dir() {
                return root;
            }
        }
    }
    fallback.to_path_buf()
}

pub fn data_dir() -> PathBuf {
    data_dir_with(
        std::env::consts::OS,
        |key| std::env::var_os(key),
        &std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()),
    )
}
/// Read-only discovery of storage used by older source/portable launches.
pub fn legacy_data_dirs() -> Vec<PathBuf> {
    if std::env::var_os("OPENRAILSRS_PLAYER_DIR").is_some() {
        return vec![];
    }
    let mut roots = vec![];
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("player-data"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        roots.push(parent.join("player-data"));
    }
    let mut roots = roots
        .into_iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    roots
}

/// Preserve old preferences/saves without moving bulk content or replacing data.
pub fn migrate_player_files(destination: &Path, sources: &[PathBuf]) {
    for source in sources {
        for name in ["settings.json", "save-1.json", "save-2.json", "save-3.json"] {
            let path = source.join(name);
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            if !path.starts_with(source) || !metadata.is_file() || metadata.len() > 8 * 1024 * 1024
            {
                continue;
            }
            let target = destination.join(name);
            if target.exists() || std::fs::create_dir_all(destination).is_err() {
                continue;
            }
            if let Ok(mut output) = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                let copied = std::fs::File::open(&path)
                    .and_then(|mut input| std::io::copy(&mut input, &mut output));
                if copied.is_err() {
                    let _ = std::fs::remove_file(&target);
                }
            }
        }
    }
}
fn data_dir_with(os: &str, env: impl Fn(&str) -> Option<OsString>, cwd: &Path) -> PathBuf {
    let absolute = |path: OsString| {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let value = |key| env(key).filter(|v| !v.is_empty());
    if let Some(path) = value("OPENRAILSRS_PLAYER_DIR") {
        return absolute(path);
    }
    if let Some(path) = value("SNAP_USER_COMMON")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return path.join("openrailsrs");
    }
    if os == "windows"
        && let Some(path) = value("LOCALAPPDATA")
    {
        return absolute(path).join("openrailsrs");
    }
    if os != "macos"
        && let Some(path) = value("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    {
        return path.join("openrailsrs");
    }
    if let Some(home) = value("HOME").or_else(|| value("USERPROFILE")) {
        let home = absolute(home);
        return home.join(if os == "macos" {
            "Library/Application Support/openrailsrs"
        } else {
            ".local/share/openrailsrs"
        });
    }
    std::env::temp_dir().join(format!("openrailsrs-{}", std::process::id()))
}

/// The installer/catalogue are embedded so a moved binary needs no checkout.
/// Python 3 is a runtime dependency and is bundled when packaging the Snap.
pub fn installer_command() -> Result<Command, String> {
    let directory = data_dir().join("runtime/content-installer");
    installer_command_in(&directory)
}
fn installer_command_in(directory: &Path) -> Result<Command, String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let script = directory.join("download_official_content.py");
    write_resource(&script, INSTALLER)?;
    write_resource(&directory.join("official-catalog.json"), CATALOG.as_bytes())?;
    let mut command =
        Command::new(std::env::var_os("OPENRAILSRS_PYTHON").unwrap_or("python3".into()));
    command.arg(script);
    command.env("OPENRAILSRS_PLAYER_DIR", data_dir());
    Ok(command)
}
fn write_resource(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolve(os: &str, values: &[(&str, &str)]) -> PathBuf {
        data_dir_with(
            os,
            |key| {
                values
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).into())
            },
            Path::new("/app/read-only"),
        )
    }
    #[test]
    fn snap_data_survives_revision_changes_and_overrides_xdg() {
        let values = [
            ("SNAP_USER_COMMON", "/home/u/snap/openrailsrs/common"),
            ("SNAP_USER_DATA", "/home/u/snap/openrailsrs/32"),
            ("XDG_DATA_HOME", "/home/u/snap/openrailsrs/32/.local/share"),
        ];
        assert_eq!(
            resolve("linux", &values),
            Path::new("/home/u/snap/openrailsrs/common/openrailsrs")
        );
    }
    #[test]
    fn compiled_binary_uses_user_storage_and_explicit_portable_override() {
        assert_eq!(
            resolve("linux", &[("HOME", "/home/u")]),
            Path::new("/home/u/.local/share/openrailsrs")
        );
        assert_eq!(
            resolve("linux", &[("XDG_DATA_HOME", "/data")]),
            Path::new("/data/openrailsrs")
        );
        assert_eq!(
            resolve(
                "linux",
                &[("HOME", "/home/u"), ("XDG_DATA_HOME", "relative")]
            ),
            Path::new("/home/u/.local/share/openrailsrs")
        );
        assert_eq!(
            resolve("linux", &[("OPENRAILSRS_PLAYER_DIR", "portable")]),
            Path::new("/app/read-only/portable")
        );
    }
    #[test]
    fn other_platforms_use_their_user_data_directories() {
        assert_eq!(
            resolve("macos", &[("HOME", "/Users/u")]),
            Path::new("/Users/u/Library/Application Support/openrailsrs")
        );
        assert_eq!(
            resolve("windows", &[("LOCALAPPDATA", "/Users/u/AppData/Local")]),
            Path::new("/Users/u/AppData/Local/openrailsrs")
        );
    }
    #[test]
    fn installer_bundle_runs_outside_the_repository() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("standalone");
        let mut command = installer_command_in(&directory).unwrap();
        let output = command
            .arg("--list")
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Demo Model 1"));
    }
    #[test]
    fn legacy_migration_preserves_existing_data_and_leaves_bulk_content() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("player-data");
        let new = temp.path().join("user-data");
        std::fs::create_dir_all(old.join("official-content")).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("settings.json"), "old preferences").unwrap();
        std::fs::write(old.join("save-1.json"), "saved game").unwrap();
        std::fs::write(new.join("settings.json"), "new preferences").unwrap();
        migrate_player_files(&new, std::slice::from_ref(&old));
        assert_eq!(
            std::fs::read_to_string(new.join("settings.json")).unwrap(),
            "new preferences"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("save-1.json")).unwrap(),
            "saved game"
        );
        assert!(old.join("save-1.json").exists());
        assert!(!new.join("official-content").exists());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(new.join("settings.json"), old.join("save-2.json")).unwrap();
            migrate_player_files(&new, &[old]);
            assert!(!new.join("save-2.json").exists());
        }
    }
}
