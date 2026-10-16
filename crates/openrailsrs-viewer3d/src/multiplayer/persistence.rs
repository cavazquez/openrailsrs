use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
pub struct ServerSave {
    pub version: u32,
    pub tick: u64,
    pub content_id: String,
    pub authority: AuthorityCheckpoint,
    pub game: crate::saved_game::SavedGame,
    pub environment: serde_json::Value,
}
pub fn save_path() -> PathBuf {
    crate::player_settings::player_data_dir().join("multiplayer/server.json")
}
pub fn private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Sesión multijugador demasiado grande".into());
    }
    let parent = path.parent().ok_or("Guardado sin directorio")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".network-{}.tmp", random_secret()?.0));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp).map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
pub fn read_save(path: &Path) -> Result<ServerSave, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err("Sesión demasiado grande".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let saved: ServerSave =
        serde_json::from_slice(&bytes).map_err(|_| "Guardado multijugador inválido")?;
    if saved.version != 1 {
        return Err("Versión de sesión multijugador incompatible".into());
    }
    // Run the ordinary save validator too: network metadata cannot waive
    // content, scripts, camera, finite weather, or rolling stock validation.
    crate::saved_game::SavedGame::from_bytes(
        &serde_json::to_vec(&saved.game).map_err(|e| e.to_string())?,
    )?;
    Ok(saved)
}
pub fn random_secret() -> Result<Secret, String> {
    let mut bytes = [0u8; 32];
    // Linux/Snap and Unix use the OS CSPRNG; no clock/process-ID tokens.
    #[cfg(unix)]
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| "No se pudo obtener una identidad aleatoria del sistema")?;
    #[cfg(not(unix))]
    {
        return Err("La identidad de red necesita un proveedor de aleatoriedad del sistema".into());
    }
    #[allow(unreachable_code)]
    Ok(Secret(bytes.iter().map(|x| format!("{x:02x}")).collect()))
}
pub fn identity_secret(identity: &str) -> Result<Secret, String> {
    identity_secret_at(
        &crate::player_settings::player_data_dir().join("multiplayer/identities"),
        identity,
    )
}
fn identity_secret_at(directory: &Path, identity: &str) -> Result<Secret, String> {
    use sha2::{Digest, Sha256};
    if !valid_id(identity) {
        return Err("Elegí un nombre de jugador válido".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!("{:x}.key", Sha256::digest(identity.as_bytes())));
    let read = || -> Result<Secret, String> {
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 256 {
            return Err("Identidad local inválida".into());
        }
        let secret = Secret(
            std::fs::read_to_string(&path).map_err(|_| "No se pudo leer la identidad local")?,
        );
        if !secret.valid() {
            return Err("Identidad local inválida; no se reemplazó la credencial".into());
        }
        Ok(secret)
    };
    if path.exists() {
        return read();
    }
    let secret = random_secret()?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(secret.0.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| "No se pudo guardar la identidad local")?;
            Ok(secret)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read(),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_independent_and_are_never_replaced_after_a_parse_failure() {
        let directory = tempfile::tempdir().unwrap();
        let first = identity_secret_at(directory.path(), "first").unwrap();
        let second = identity_secret_at(directory.path(), "second").unwrap();
        assert!(!first.matches(&second));
        assert!(first.matches(&identity_secret_at(directory.path(), "first").unwrap()));
        assert!(second.matches(&identity_secret_at(directory.path(), "second").unwrap()));
        let paths = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in &paths {
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        use sha2::{Digest, Sha256};
        let first_path = directory
            .path()
            .join(format!("{:x}.key", Sha256::digest(b"first")));
        std::fs::write(&first_path, []).unwrap();
        assert!(identity_secret_at(directory.path(), "first").is_err());
        assert!(std::fs::read(&first_path).unwrap().is_empty());
        assert!(second.matches(&identity_secret_at(directory.path(), "second").unwrap()));
    }
    #[test]
    fn atomic_private_write_replaces_a_public_slot_with_a_private_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("save.json");
        std::fs::write(&path, b"previous").unwrap();
        private_write(&path, b"new state").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new state");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
