//! Player-owned formations preserve the author's ENG/WAG files and source order.
use bevy::prelude::*;
use std::path::{Path, PathBuf};
#[derive(Clone, Debug)]
pub struct Slot {
    pub file: PathBuf,
    pub engine: bool,
    pub flipped: bool,
}
#[derive(Resource, Default)]
pub struct FormationBuilder {
    pub slots: Vec<Slot>,
    pub catalogue: Vec<PathBuf>,
    pub selected: usize,
    pub status: String,
    undo: Vec<Vec<Slot>>,
    pub draft: Option<PathBuf>,
}
impl FormationBuilder {
    pub fn load(&mut self, menu: &crate::player_launch::PlayerLaunchMenu) -> Result<(), String> {
        let con = menu
            .consists
            .get(menu.consist)
            .ok_or("Elegí una formación primero")?;
        let text = openrailsrs_formats::read_msts_file_to_string(con).map_err(|e| e.to_string())?;
        let ast = openrailsrs_formats::parse_vehicle_text(&text).map_err(|e| e.to_string())?;
        let entries =
            openrailsrs_formats::ConsistFile::from_ast(&ast).map_err(|e| e.to_string())?;
        if entries.entries.len() > 64 {
            return Err("El editor admite hasta 64 vehículos por formación".into());
        }
        self.slots = entries
            .entries
            .iter()
            .map(|e| Slot {
                file: openrailsrs_train::resolve_consist_entry_path(
                    openrailsrs_train::consist_asset_root(con),
                    e.path(),
                ),
                engine: matches!(e, openrailsrs_formats::ConsistEntry::Engine { .. }),
                flipped: e.flipped(),
            })
            .collect();
        self.catalogue = menu
            .consist_audits
            .values()
            .flat_map(|a| a.compatibility.iter().map(|v| v.stock_path.clone()))
            .collect();
        self.catalogue.sort();
        self.catalogue.dedup();
        self.undo.clear();
        self.selected = 0;
        self.status = "Editando una copia propia. Los recursos incompletos siguen visibles.".into();
        Ok(())
    }
    pub fn edit(&mut self, operation: u8, index: usize) -> Result<(), String> {
        let mut next = self.slots.clone();
        match operation {
            0 => {
                if next.len() >= 64 {
                    return Err("Límite de 64 vehículos".into());
                }
                let file = self
                    .catalogue
                    .get(index)
                    .ok_or("Vehículo no disponible")?
                    .clone();
                let engine = file
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("eng"));
                next.push(Slot {
                    file,
                    engine,
                    flipped: false,
                });
            }
            1 => {
                if index >= next.len() || next.len() == 1 {
                    return Err("Conservá al menos un vehículo".into());
                }
                next.remove(index);
            }
            2 => {
                let slot = next.get_mut(index).ok_or("Vehículo inexistente")?;
                slot.flipped = !slot.flipped;
            }
            3 if index > 0 && index < next.len() => next.swap(index, index - 1),
            4 if index + 1 < next.len() => next.swap(index, index + 1),
            _ => return Err("No se puede mover más allá del extremo de la formación".into()),
        }
        self.undo.push(std::mem::replace(&mut self.slots, next));
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), String> {
        self.slots = self.undo.pop().ok_or("No hay cambios para deshacer")?;
        Ok(())
    }
    pub fn save(&mut self, data: &Path) -> Result<PathBuf, String> {
        if self.slots.is_empty() || self.slots.len() > 64 {
            return Err("Formación vacía o demasiado larga".into());
        }
        let mut text = "SIMISA@@@@@@@@@@JINX0D0t______\n(Train\n".to_string();
        for (i, slot) in self.slots.iter().enumerate() {
            let path = slot.file.to_string_lossy();
            if !slot.file.is_absolute()
                || path.len() > 4096
                || path.contains(['"', '\n', '\r', '\0'])
            {
                return Err("Referencia ENG/WAG inválida".into());
            }
            text += &format!(
                "({} \"{}\" (UiD {}){})\n",
                if slot.engine { "Engine" } else { "Wagon" },
                path.replace('\\', "/"),
                i + 1,
                if slot.flipped { " (Flip (1))" } else { "" }
            );
        }
        text += ")\n";
        // Immutable draft names make cancellation and preview caches unambiguous.
        let mut hash = 0xcbf29ce484222325u64;
        for b in text.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let path = data
            .join("custom-consists")
            .join(format!("formation-{hash:016x}.con"));
        let mut bytes = vec![0xff, 0xfe];
        for word in text.encode_utf16() {
            bytes.extend(word.to_le_bytes());
        }
        crate::player_settings::atomic_write(&path, &bytes)?;
        self.status = format!(
            "Formación propia guardada en {}. La auditoría continúa en segundo plano.",
            path.display()
        );
        self.draft = Some(path.clone());
        Ok(path)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flip_and_order_round_trip_without_modifying_original_stock() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/player_lab/vehicles")
            .canonicalize()
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let mut editor = FormationBuilder {
            slots: vec![
                Slot {
                    file: root.join("lab.eng"),
                    engine: true,
                    flipped: false,
                },
                Slot {
                    file: root.join("lab.wag"),
                    engine: false,
                    flipped: false,
                },
            ],
            ..default()
        };
        let original = std::fs::read(root.join("lab.eng")).unwrap();
        editor.edit(2, 1).unwrap();
        editor.edit(3, 1).unwrap();
        let file = editor.save(temp.path()).unwrap();
        let parsed = openrailsrs_formats::ConsistFile::from_ast(
            &openrailsrs_formats::parse_vehicle_text(
                &openrailsrs_formats::read_msts_file_to_string(&file).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(parsed.entries[0].flipped());
        assert!(matches!(
            parsed.entries[0],
            openrailsrs_formats::ConsistEntry::Wagon { .. }
        ));
        assert_eq!(std::fs::read(root.join("lab.eng")).unwrap(), original);
        let consist = openrailsrs_train::load_consist_from_path(&file).unwrap();
        assert_eq!(consist.vehicles.len(), 2);
        editor.undo().unwrap();
        assert!(editor.slots[0].engine);
    }
}
