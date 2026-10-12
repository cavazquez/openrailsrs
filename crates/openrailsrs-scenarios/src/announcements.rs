//! An explicitly licensed local package. No automatic download or synthesized fallback.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AnnouncementPackage {
    pub root: String,
    pub author: String,
    pub license: String,
    pub source: String,
    pub events: Vec<AnnouncementDef>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnnouncementEvent {
    Approach,
    Arrival,
    Departure,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnnouncementDef {
    pub id: String,
    pub stop: usize,
    pub event: AnnouncementEvent,
    pub text: String,
    pub file: String,
    #[serde(default)]
    pub service: Option<String>,
}
impl AnnouncementPackage {
    pub fn valid(&self) -> bool {
        if self.events.is_empty() {
            return true;
        }
        self.root.len() <= 4096
            && !self.root.chars().any(char::is_control)
            && !self.author.trim().is_empty()
            && self.author.len() <= 256
            && !self.license.trim().is_empty()
            && self.license.len() <= 256
            && self.source.starts_with("https://")
            && self.source.len() <= 512
            && self.events.len() <= 256
            && self.events.iter().enumerate().all(|(i, e)| {
                !e.id.is_empty()
                    && e.id.len() <= 128
                    && self.events[..i].iter().all(|p| p.id != e.id)
                    && e.stop < 4096
                    && !e.text.is_empty()
                    && e.text.len() <= 512
                    && !e.text.chars().any(char::is_control)
                    && e.service
                        .as_ref()
                        .is_none_or(|s| !s.is_empty() && s.len() <= 256)
                    && std::path::Path::new(&e.file)
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                    && e.file.len() <= 256
                    && std::path::Path::new(&e.file)
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("wav"))
            })
    }
}
