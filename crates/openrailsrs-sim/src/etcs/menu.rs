//! Menu / soft-key definitions emitted by the Rust TCS (OR `DMI*Definition` subset).

/// Soft-key column actions (right MenuBar).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftKeyAction {
    None,
    OpenMainMenu,
    Override,
    OpenDataEntry,
    Special,
    OpenSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoftKeyDef {
    pub label: String,
    pub action: SoftKeyAction,
    pub enabled: bool,
}

/// Button inside a menu subwindow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuButtonDef {
    pub label: String,
    pub action: MenuAction,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Flash(String),
    OpenMainMenu,
    OpenSettings,
    OpenDataEntry,
    Close,
    /// Acknowledge pending supervision message.
    Acknowledge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuWindowDef {
    pub id: String,
    pub title: String,
    pub buttons: Vec<MenuButtonDef>,
}

pub fn default_soft_keys() -> Vec<SoftKeyDef> {
    vec![
        SoftKeyDef {
            label: "Main".into(),
            action: SoftKeyAction::OpenMainMenu,
            enabled: true,
        },
        SoftKeyDef {
            label: "Over.".into(),
            action: SoftKeyAction::Override,
            enabled: true,
        },
        SoftKeyDef {
            label: "Data".into(),
            action: SoftKeyAction::OpenDataEntry,
            enabled: true,
        },
        SoftKeyDef {
            label: "Spec".into(),
            action: SoftKeyAction::Special,
            enabled: true,
        },
        SoftKeyDef {
            label: "Sett.".into(),
            action: SoftKeyAction::OpenSettings,
            enabled: true,
        },
        SoftKeyDef {
            label: String::new(),
            action: SoftKeyAction::None,
            enabled: false,
        },
    ]
}

pub fn main_menu_def() -> MenuWindowDef {
    MenuWindowDef {
        id: "main".into(),
        title: "Main".into(),
        buttons: vec![
            btn("Start", MenuAction::Flash("Start".into())),
            btn("Override", MenuAction::Flash("Override".into())),
            btn("Data", MenuAction::OpenDataEntry),
            btn("Special", MenuAction::Flash("Special".into())),
            btn("Settings", MenuAction::OpenSettings),
            btn("Quit", MenuAction::Close),
        ],
    }
}

pub fn settings_menu_def() -> MenuWindowDef {
    MenuWindowDef {
        id: "settings".into(),
        title: "Settings".into(),
        buttons: vec![
            btn("Brightness", MenuAction::Flash("Brightness".into())),
            btn("Volume", MenuAction::Flash("Volume".into())),
            btn("Language", MenuAction::Flash("Language".into())),
            btn("Units", MenuAction::Flash("Units".into())),
            btn("Back", MenuAction::OpenMainMenu),
            btn("", MenuAction::Close),
        ],
    }
}

fn btn(label: &str, action: MenuAction) -> MenuButtonDef {
    MenuButtonDef {
        label: label.into(),
        action,
        enabled: !label.is_empty(),
    }
}
