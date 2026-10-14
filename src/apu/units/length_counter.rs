// https://www.nesdev.org/wiki/APU_Length_Counter

// Indexed by bits 7-3 of the channel's fourth register.
pub const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, //
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

// Bit 5 of $4000/$4004/$400C halts the length counter (and loops the
// envelope). The triangle uses bit 7 of $4008 instead.
pub const HALT_FLAG: u8 = 0b0010_0000;

pub struct LengthCounter {
    pub counter: u8,
    pub halt: bool,
    pub enabled: bool,
}

impl Default for LengthCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl LengthCounter {
    pub fn new() -> Self {
        LengthCounter {
            counter: 0,
            halt: false,
            enabled: false,
        }
    }

    // Bits 7-3 of the channel's length register. Ignored while the channel is
    // disabled through $4015.
    pub fn load(&mut self, data: u8) {
        if self.enabled {
            self.counter = LENGTH_TABLE[(data >> 3) as usize];
        }
    }

    // $4015: clearing a channel's bit silences it by zeroing the counter and
    // the old value is lost; setting the bit has no immediate effect.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.counter = 0;
        }
    }

    // Half-frame clock.
    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    pub fn is_active(&self) -> bool {
        self.counter > 0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn load_looks_up_bits_7_to_3_in_the_length_table() {
        let mut length = LengthCounter::new();
        length.set_enabled(true);
        length.load(0b0000_1000); // index 1
        assert_eq!(length.counter, 254);
        length.load(0b1111_1000); // index 31
        assert_eq!(length.counter, 30);
    }

    #[test]
    fn load_is_ignored_while_disabled() {
        let mut length = LengthCounter::new();
        length.load(0b0000_1000);
        assert_eq!(length.counter, 0);
        assert!(!length.is_active());
    }

    #[test]
    fn disabling_zeroes_the_counter_and_enabling_does_not_restore_it() {
        let mut length = LengthCounter::new();
        length.set_enabled(true);
        length.load(0);
        assert_eq!(length.counter, 10);
        length.set_enabled(false);
        assert_eq!(length.counter, 0);
        length.set_enabled(true);
        assert_eq!(length.counter, 0);
    }

    #[test]
    fn clock_counts_down_to_zero_and_stops() {
        let mut length = LengthCounter::new();
        length.set_enabled(true);
        length.load(0b0001_1000); // index 3 -> 2
        length.clock();
        assert!(length.is_active());
        length.clock();
        assert!(!length.is_active());
        length.clock();
        assert_eq!(length.counter, 0);
    }

    #[test]
    fn halt_flag_freezes_the_counter() {
        let mut length = LengthCounter::new();
        length.set_enabled(true);
        length.load(0);
        length.halt = true;
        length.clock();
        assert_eq!(length.counter, 10);
    }
}
