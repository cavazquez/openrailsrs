//! Re-export from `openrailsrs-bevy-scenery` (#109).
//!
//! Canonical texture resolution / ACE·DDS decode lives in
//! `openrailsrs_bevy_scenery::textures`. Call sites may keep using
//! `crate::textures::…`.

pub use openrailsrs_bevy_scenery::textures::*;
