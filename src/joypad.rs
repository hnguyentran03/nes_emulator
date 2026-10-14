// https://www.nesdev.org/wiki/Standard_controller

bitflags! {
    // Bit order matches the serial report order: A is shifted out first.
    // https://www.nesdev.org/wiki/Controller_reading_code
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct JoypadButton: u8 {
        const BUTTON_A = 0b0000_0001;
        const BUTTON_B = 0b0000_0010;
        const SELECT   = 0b0000_0100;
        const START    = 0b0000_1000;
        const UP       = 0b0001_0000;
        const DOWN     = 0b0010_0000;
        const LEFT     = 0b0100_0000;
        const RIGHT    = 0b1000_0000;
    }
}

pub struct Joypad {
    strobe: bool,
    button_index: u8,
    button_status: JoypadButton,
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new()
    }
}

impl Joypad {
    pub fn new() -> Self {
        Joypad {
            strobe: false,
            button_index: 0,
            button_status: JoypadButton::empty(),
        }
    }

    // Only bit 0 matters. While strobe is high the shift register is held
    // at button A; the falling edge latches the current state for reading.
    pub fn write(&mut self, data: u8) {
        self.strobe = data & 1 == 1;
        if self.strobe {
            self.button_index = 0;
        }
    }

    // Shifts out one button per read (A, B, Select, Start, Up, Down, Left,
    // Right). After all eight the controller returns 1 until the next strobe.
    pub fn read(&mut self) -> u8 {
        if self.button_index > 7 {
            return 1;
        }
        let response = (self.button_status.bits() >> self.button_index) & 1;
        if !self.strobe {
            self.button_index += 1;
        }
        response
    }

    pub fn set_button_pressed_status(&mut self, button: JoypadButton, pressed: bool) {
        self.button_status.set(button, pressed);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn strobe_on_reports_button_a_on_every_read() {
        let mut joypad = Joypad::new();
        joypad.write(1);
        joypad.set_button_pressed_status(JoypadButton::BUTTON_A, true);
        for _ in 0..10 {
            assert_eq!(joypad.read(), 1);
        }
    }

    #[test]
    fn strobe_off_cycles_through_buttons_then_returns_ones() {
        let mut joypad = Joypad::new();
        joypad.write(0);
        joypad.set_button_pressed_status(JoypadButton::RIGHT, true);
        joypad.set_button_pressed_status(JoypadButton::LEFT, true);
        joypad.set_button_pressed_status(JoypadButton::SELECT, true);
        joypad.set_button_pressed_status(JoypadButton::BUTTON_B, true);

        for _ in 0..=1 {
            // A, B, Select, Start, Up, Down, Left, Right
            let expected = [0, 1, 1, 0, 0, 0, 1, 1];
            for want in expected {
                assert_eq!(joypad.read(), want);
            }
            for _ in 0..10 {
                assert_eq!(joypad.read(), 1);
            }
            joypad.write(1);
            joypad.write(0);
        }
    }

    #[test]
    fn strobe_on_resets_index_mid_sequence() {
        let mut joypad = Joypad::new();
        joypad.set_button_pressed_status(JoypadButton::BUTTON_A, true);
        joypad.write(1);
        joypad.write(0);
        assert_eq!(joypad.read(), 1); // A
        assert_eq!(joypad.read(), 0); // B
        joypad.write(1);
        joypad.write(0);
        assert_eq!(joypad.read(), 1); // back to A
    }

    #[test]
    fn releasing_a_button_clears_its_bit() {
        let mut joypad = Joypad::new();
        joypad.set_button_pressed_status(JoypadButton::START, true);
        joypad.set_button_pressed_status(JoypadButton::START, false);
        joypad.write(1);
        joypad.write(0);
        for _ in 0..8 {
            assert_eq!(joypad.read(), 0);
        }
    }
}
