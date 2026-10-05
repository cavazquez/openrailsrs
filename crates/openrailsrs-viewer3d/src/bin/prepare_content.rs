//! Audit and prepare an installed official package without opening a window.
use openrailsrs_viewer3d::official_content;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let directory = args
        .next()
        .map(PathBuf::from)
        .ok_or("Uso: openrailsrs-prepare-content /ruta/al/paquete-instalado")?;
    if args.next().is_some() {
        return Err("Se espera únicamente la carpeta del paquete instalado".into());
    }
    println!("{}", official_content::prepare_installed(&directory, None)?);
    for route in official_content::prepared_routes() {
        if route
            .native
            .starts_with(directory.canonicalize().map_err(|e| e.to_string())?)
        {
            println!(
                "Ruta: {} · {} actividades",
                route.native.display(),
                route.activities.len()
            );
        }
    }
    Ok(())
}
