//! ETCS Driver Machine Interface (OR `DriverMachineInterface` subset, #159).
//!
//! CPU raster into the cab `ScreenDisplay` texture (640×480 Full layout).

mod colors;
mod gauge;
mod paint;
mod planning;
mod status;

pub use paint::paint_dmi_full;
pub use status::{EtcsStatus, etcs_status_from_live};
