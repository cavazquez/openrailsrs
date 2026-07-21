//! MSTS / OpenRails `carspawn.dat` vehicle catalogues for road traffic (#32).

use std::path::Path;

use crate::error::FormatError;
use crate::msts_file_text::read_msts_file_decoded;

/// One car model entry: shape filename + length along the road (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct CarSpawnerItem {
    pub shape: String,
    pub length_m: f32,
}

/// Named list of car models (`Default` or an `ORTSListName`).
#[derive(Clone, Debug, PartialEq)]
pub struct CarSpawnerList {
    pub name: String,
    pub items: Vec<CarSpawnerItem>,
}

/// Combined default + OpenRails multi-list carspawn catalogues.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarSpawnerCatalog {
    pub lists: Vec<CarSpawnerList>,
}

impl CarSpawnerCatalog {
    /// Load `route/carspawn.dat` (Default) and `route/OpenRails/carspawn.dat` (named lists).
    pub fn load_for_route(route_dir: &Path) -> Result<Self, FormatError> {
        let mut catalog = Self::default();
        let default_path = route_dir.join("carspawn.dat");
        if default_path.is_file() {
            let text = read_msts_file_decoded(&default_path)?;
            let items = scan_carspawner_items(&text);
            if !items.is_empty() {
                catalog.lists.push(CarSpawnerList {
                    name: "Default".into(),
                    items,
                });
            }
        }
        for sub in ["OpenRails/carspawn.dat", "openrails/carspawn.dat"] {
            let path = route_dir.join(sub);
            if path.is_file() {
                let text = read_msts_file_decoded(&path)?;
                catalog.lists.extend(scan_orts_lists(&text));
                break;
            }
        }
        Ok(catalog)
    }

    pub fn list_by_name(&self, name: &str) -> Option<&CarSpawnerList> {
        self.lists
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Resolve list for a spawner: named ORTS list, else `"Default"`, else first non-empty.
    pub fn resolve_list(&self, list_name: Option<&str>) -> Option<&CarSpawnerList> {
        if let Some(name) = list_name {
            if let Some(list) = self.list_by_name(name) {
                if !list.items.is_empty() {
                    return Some(list);
                }
            }
        }
        if let Some(list) = self.list_by_name("Default") {
            if !list.items.is_empty() {
                return Some(list);
            }
        }
        self.lists.iter().find(|l| !l.items.is_empty())
    }

    /// Deterministic model pick from list + spawner uid.
    pub fn pick_item(&self, list_name: Option<&str>, uid: u32) -> Option<&CarSpawnerItem> {
        let list = self.resolve_list(list_name)?;
        if list.items.is_empty() {
            return None;
        }
        Some(&list.items[uid as usize % list.items.len()])
    }
}

/// Scan `CarSpawnerItem( "shape.s" length )` occurrences (JINX head-outside-paren form).
fn scan_carspawner_items(text: &str) -> Vec<CarSpawnerItem> {
    let lower = text.to_ascii_lowercase();
    let mut items = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("carspawneritem") {
        let idx = search_from + rel;
        let after = &text[idx + "carspawneritem".len()..];
        if let Some(item) = parse_item_args(after) {
            items.push(item);
        }
        search_from = idx + "carspawneritem".len();
    }
    items
}

fn parse_item_args(after_name: &str) -> Option<CarSpawnerItem> {
    let open = after_name.find('(')?;
    let body = &after_name[open + 1..];
    let q1 = body.find('"')?;
    let rest = &body[q1 + 1..];
    let q2 = rest.find('"')?;
    let shape = rest[..q2].to_string();
    let after_shape = &rest[q2 + 1..];
    let length_m = after_shape
        .split(|c: char| c == ')' || c.is_whitespace())
        .find_map(|tok| {
            let t = tok.trim();
            if t.is_empty() {
                None
            } else {
                t.parse::<f32>().ok()
            }
        })
        .unwrap_or(8.0);
    Some(CarSpawnerItem { shape, length_m })
}

fn scan_orts_lists(text: &str) -> Vec<CarSpawnerList> {
    let lower = text.to_ascii_lowercase();
    let mut lists = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("carspawnerlist") {
        let idx = search_from + rel;
        let after = &text[idx + "carspawnerlist".len()..];
        let Some(open_rel) = after.find('(') else {
            search_from = idx + "carspawnerlist".len();
            continue;
        };
        let block_start = idx + "carspawnerlist".len() + open_rel;
        let Some(end_rel) = matching_paren_end(&text[block_start..]) else {
            search_from = idx + "carspawnerlist".len();
            continue;
        };
        let block = &text[block_start..block_start + end_rel + 1];
        let name = find_list_name(block).unwrap_or_else(|| "Unnamed".into());
        let items = scan_carspawner_items(block);
        lists.push(CarSpawnerList { name, items });
        search_from = block_start + end_rel + 1;
    }
    lists
}

fn matching_paren_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(|b| *b == b'(') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_list_name(block: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let idx = lower.find("listname")?;
    let after = &block[idx + "listname".len()..];
    let q1_rel = after.find('"')?;
    let rest = &after[q1_rel + 1..];
    let q2 = rest.find('"')?;
    Some(rest[..q2].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chiltern_route() -> Option<PathBuf> {
        std::env::var_os("CHILTERN_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home)
                    .join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
                p.join("carspawn.dat").is_file().then_some(p)
            })
    }

    #[test]
    fn chiltern_carspawn_loads_default_and_london_inner() {
        let Some(route) = chiltern_route() else {
            return;
        };
        let catalog = CarSpawnerCatalog::load_for_route(&route).expect("load");
        let default = catalog
            .list_by_name("Default")
            .expect("Default list missing");
        assert!(
            default.items.len() >= 80,
            "Default should have ~85 items, got {}",
            default.items.len()
        );
        let london = catalog
            .list_by_name("London Inner")
            .expect("London Inner list");
        assert!(london.items.len() >= 10);
        let picked = catalog.pick_item(Some("London Inner"), 2253).expect("pick");
        assert!(picked.shape.to_ascii_lowercase().ends_with(".s"));
        assert!(picked.length_m > 0.0);
    }
}
