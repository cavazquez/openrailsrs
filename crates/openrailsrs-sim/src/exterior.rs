//! Exterior rolling-stock presentation state for live drive (#81).
//!
//! Visual-only: doors / pantograph command for shape keys. Not air-brake physics.

/// Door presentation (OR-style coarse states).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DoorState {
    #[default]
    Closed,
    Closing,
    Opening,
    Open,
}

impl DoorState {
    /// OR `MSTSWagonViewer`: animate toward open when `State >= Opening`.
    pub fn anim_open_target(self) -> bool {
        matches!(self, Self::Opening | Self::Open)
    }
}

/// Consist-level exterior anim targets consumed by the viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct RollingStockExteriorState {
    pub door: DoorState,
    /// OR pantograph `CommandUp` (true = raise / up).
    pub pantograph_command_up: bool,
    /// Seconds remaining in Opening/Closing before snapping to Open/Closed.
    door_transition_remaining_s: f64,
}

impl Default for RollingStockExteriorState {
    fn default() -> Self {
        Self::new()
    }
}

impl RollingStockExteriorState {
    pub const DOOR_TRANSITION_S: f64 = 1.0;

    pub fn new() -> Self {
        Self {
            door: DoorState::Closed,
            pantograph_command_up: false,
            door_transition_remaining_s: 0.0,
        }
    }

    pub fn set_door(&mut self, door: DoorState) {
        self.door = door;
        self.door_transition_remaining_s = match door {
            DoorState::Opening | DoorState::Closing => Self::DOOR_TRANSITION_S,
            DoorState::Closed | DoorState::Open => 0.0,
        };
    }

    pub fn toggle_door(&mut self) {
        match self.door {
            DoorState::Closed | DoorState::Closing => self.set_door(DoorState::Opening),
            DoorState::Open | DoorState::Opening => self.set_door(DoorState::Closing),
        }
    }

    pub fn set_pantograph_up(&mut self, up: bool) {
        self.pantograph_command_up = up;
    }

    pub fn toggle_pantograph(&mut self) {
        self.pantograph_command_up = !self.pantograph_command_up;
    }

    /// Advance door Opening/Closing timers toward Open/Closed.
    pub fn tick(&mut self, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        match self.door {
            DoorState::Opening | DoorState::Closing => {
                self.door_transition_remaining_s -= dt;
                if self.door_transition_remaining_s <= 0.0 {
                    self.door_transition_remaining_s = 0.0;
                    self.door = if self.door == DoorState::Opening {
                        DoorState::Open
                    } else {
                        DoorState::Closed
                    };
                }
            }
            DoorState::Closed | DoorState::Open => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_door_opens_then_closes() {
        let mut e = RollingStockExteriorState::new();
        e.toggle_door();
        assert_eq!(e.door, DoorState::Opening);
        assert!(e.door.anim_open_target());
        e.tick(RollingStockExteriorState::DOOR_TRANSITION_S);
        assert_eq!(e.door, DoorState::Open);
        e.toggle_door();
        assert_eq!(e.door, DoorState::Closing);
        assert!(!e.door.anim_open_target());
        e.tick(RollingStockExteriorState::DOOR_TRANSITION_S);
        assert_eq!(e.door, DoorState::Closed);
    }

    #[test]
    fn pantograph_toggle() {
        let mut e = RollingStockExteriorState::new();
        assert!(!e.pantograph_command_up);
        e.toggle_pantograph();
        assert!(e.pantograph_command_up);
        e.set_pantograph_up(false);
        assert!(!e.pantograph_command_up);
    }
}
