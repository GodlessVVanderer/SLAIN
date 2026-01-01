//! Input handling for game controllers

/// Standard button state for NES/SMS style controllers
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
}

impl ButtonState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to NES controller byte format
    pub fn to_nes_byte(&self) -> u8 {
        let mut val = 0u8;
        if self.a { val |= 0x01; }
        if self.b { val |= 0x02; }
        if self.select { val |= 0x04; }
        if self.start { val |= 0x08; }
        if self.up { val |= 0x10; }
        if self.down { val |= 0x20; }
        if self.left { val |= 0x40; }
        if self.right { val |= 0x80; }
        val
    }

    /// Create from NES controller byte format
    pub fn from_nes_byte(val: u8) -> Self {
        Self {
            a: val & 0x01 != 0,
            b: val & 0x02 != 0,
            select: val & 0x04 != 0,
            start: val & 0x08 != 0,
            up: val & 0x10 != 0,
            down: val & 0x20 != 0,
            left: val & 0x40 != 0,
            right: val & 0x80 != 0,
        }
    }
}

/// Extended button state for 6-button controllers (Genesis, Arcade)
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonState6 {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub c: bool,
    pub x: bool,
    pub y: bool,
    pub z: bool,
    pub start: bool,
    pub mode: bool,
}

impl ButtonState6 {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Dreamcast/Atomiswave controller state
#[derive(Debug, Clone, Copy, Default)]
pub struct DreamcastController {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub start: bool,
    /// Left trigger (0-255)
    pub l_trigger: u8,
    /// Right trigger (0-255)
    pub r_trigger: u8,
    /// Analog stick X (-128 to 127)
    pub analog_x: i8,
    /// Analog stick Y (-128 to 127)
    pub analog_y: i8,
}

impl DreamcastController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to Dreamcast controller register format
    pub fn to_register(&self) -> u32 {
        let mut val = 0xFFFFu32;

        // Digital buttons (active low)
        if self.up { val &= !(1 << 4); }
        if self.down { val &= !(1 << 5); }
        if self.left { val &= !(1 << 6); }
        if self.right { val &= !(1 << 7); }
        if self.a { val &= !(1 << 2); }
        if self.b { val &= !(1 << 1); }
        if self.x { val &= !(1 << 10); }
        if self.y { val &= !(1 << 9); }
        if self.start { val &= !(1 << 3); }

        val
    }

    /// Get analog values packed
    pub fn analog_pack(&self) -> u32 {
        let ax = (self.analog_x as u8) as u32;
        let ay = (self.analog_y as u8) as u32;
        let lt = self.l_trigger as u32;
        let rt = self.r_trigger as u32;
        (ax << 24) | (ay << 16) | (lt << 8) | rt
    }
}

/// Arcade stick for fighting games (Fist of the North Star)
#[derive(Debug, Clone, Copy, Default)]
pub struct ArcadeStick {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Light punch
    pub lp: bool,
    /// Medium punch
    pub mp: bool,
    /// Heavy punch
    pub hp: bool,
    /// Light kick
    pub lk: bool,
    /// Medium kick
    pub mk: bool,
    /// Heavy kick
    pub hk: bool,
    pub start: bool,
    pub coin: bool,
    /// Special attack (Fist of the North Star specific)
    pub boost: bool,
}

impl ArcadeStick {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to Atomiswave JVS format
    pub fn to_jvs_p1(&self) -> u16 {
        let mut val = 0u16;
        if self.up { val |= 1 << 0; }
        if self.down { val |= 1 << 1; }
        if self.left { val |= 1 << 2; }
        if self.right { val |= 1 << 3; }
        if self.lp { val |= 1 << 4; }
        if self.mp { val |= 1 << 5; }
        if self.hp { val |= 1 << 6; }
        if self.lk { val |= 1 << 7; }
        if self.mk { val |= 1 << 8; }
        if self.hk { val |= 1 << 9; }
        if self.start { val |= 1 << 10; }
        if self.boost { val |= 1 << 11; }
        val
    }
}

/// Input manager for handling multiple controller types
pub struct InputManager {
    /// NES-style controllers (2 players)
    pub nes: [ButtonState; 2],
    /// SMS controllers (2 players)
    pub sms: [ButtonState; 2],
    /// Dreamcast controllers (4 players)
    pub dc: [DreamcastController; 4],
    /// Arcade sticks (2 players)
    pub arcade: [ArcadeStick; 2],
    /// System buttons
    pub service: bool,
    pub test: bool,
}

impl InputManager {
    pub fn new() -> Self {
        Self {
            nes: [ButtonState::default(); 2],
            sms: [ButtonState::default(); 2],
            dc: [DreamcastController::default(); 4],
            arcade: [ArcadeStick::default(); 2],
            service: false,
            test: false,
        }
    }

    pub fn reset(&mut self) {
        self.nes = [ButtonState::default(); 2];
        self.sms = [ButtonState::default(); 2];
        self.dc = [DreamcastController::default(); 4];
        self.arcade = [ArcadeStick::default(); 2];
        self.service = false;
        self.test = false;
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Key mapping for keyboard to controller translation
#[derive(Debug, Clone)]
pub struct KeyMapping {
    pub up: u32,
    pub down: u32,
    pub left: u32,
    pub right: u32,
    pub a: u32,
    pub b: u32,
    pub x: u32,
    pub y: u32,
    pub start: u32,
    pub select: u32,
    pub l: u32,
    pub r: u32,
}

impl Default for KeyMapping {
    fn default() -> Self {
        Self {
            up: 0x26,    // Arrow Up
            down: 0x28,  // Arrow Down
            left: 0x25,  // Arrow Left
            right: 0x27, // Arrow Right
            a: 0x5A,     // Z
            b: 0x58,     // X
            x: 0x41,     // A
            y: 0x53,     // S
            start: 0x0D, // Enter
            select: 0x20, // Space
            l: 0x51,     // Q
            r: 0x57,     // W
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_state_conversion() {
        let mut state = ButtonState::new();
        state.a = true;
        state.start = true;
        state.up = true;

        let byte = state.to_nes_byte();
        let restored = ButtonState::from_nes_byte(byte);

        assert_eq!(restored.a, true);
        assert_eq!(restored.start, true);
        assert_eq!(restored.up, true);
        assert_eq!(restored.b, false);
    }
}
