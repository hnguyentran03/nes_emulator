// https://www.nesdev.org/wiki/APU_Triangle

use crate::apu::units::length_counter::LengthCounter;

// 32-step waveform: 15 down to 0, then 0 up to 15.
const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, //
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
const SEQUENCE_LENGTH: u8 = 32;
// $4008 bit 7 is both the linear counter control flag and the length halt.
const CONTROL_FLAG: u8 = 0b1000_0000;
const LINEAR_RELOAD_MASK: u8 = 0b0111_1111;
const TIMER_LOW_MASK: u16 = 0x00FF;
const TIMER_HIGH_MASK: u8 = 0b0000_0111;

pub struct Triangle {
    pub sequence_step: u8,
    pub timer_period: u16,
    pub timer: u16,
    pub linear_counter: u8,
    pub linear_reload_value: u8,
    pub linear_reload: bool,
    pub control: bool,
    pub length_counter: LengthCounter,
}

impl Default for Triangle {
    fn default() -> Self {
        Self::new()
    }
}

impl Triangle {
    pub fn new() -> Self {
        Triangle {
            sequence_step: 0,
            timer_period: 0,
            timer: 0,
            linear_counter: 0,
            linear_reload_value: 0,
            linear_reload: false,
            control: false,
            length_counter: LengthCounter::new(),
        }
    }

    // `reg` is the offset from $4008.
    pub fn write_to_register(&mut self, reg: u8, data: u8) {
        match reg {
            // CRRR RRRR
            0 => {
                self.control = data & CONTROL_FLAG != 0;
                self.length_counter.halt = self.control;
                self.linear_reload_value = data & LINEAR_RELOAD_MASK;
            }

            1 => { /* $4009 is unused */ }

            // TTTT TTTT: timer low
            2 => self.timer_period = (self.timer_period & !TIMER_LOW_MASK) | data as u16,

            // LLLL LTTT: length load and timer high. Also flags the linear
            // counter for reload on the next quarter frame.
            3 => {
                self.timer_period =
                    (self.timer_period & TIMER_LOW_MASK) | ((data & TIMER_HIGH_MASK) as u16) << 8;
                self.length_counter.load(data);
                self.linear_reload = true;
            }

            _ => panic!("Invalid triangle register {}", reg),
        }
    }

    // Clocked every CPU cycle (twice the rate of the other channels). The
    // sequencer only advances while both counters are running, so a silenced
    // triangle holds its last value instead of popping to zero.
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            if self.linear_counter > 0 && self.length_counter.is_active() {
                self.sequence_step = (self.sequence_step + 1) % SEQUENCE_LENGTH;
            }
        } else {
            self.timer -= 1;
        }
    }

    // Quarter-frame clock: reload or count the linear counter. The reload
    // flag only clears when the control flag is clear.
    pub fn clock_quarter_frame(&mut self) {
        if self.linear_reload {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.control {
            self.linear_reload = false;
        }
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.length_counter.set_enabled(enabled);
    }

    pub fn is_active(&self) -> bool {
        self.length_counter.is_active()
    }

    pub fn output(&self) -> u8 {
        TRIANGLE_SEQUENCE[self.sequence_step as usize]
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Linear reload 127 (control clear), period 0 (one clock per step),
    // length 10, and the first quarter frame already loaded the linear counter.
    fn sounding_triangle() -> Triangle {
        let mut triangle = Triangle::new();
        triangle.set_enabled(true);
        triangle.write_to_register(0, 0b0111_1111);
        triangle.write_to_register(2, 0);
        triangle.write_to_register(3, 0);
        triangle.clock_quarter_frame();
        triangle
    }

    #[test]
    fn register_0_sets_control_halt_and_linear_reload_value() {
        let mut triangle = Triangle::new();
        triangle.write_to_register(0, 0b1010_1010);
        assert!(triangle.control);
        assert!(triangle.length_counter.halt);
        assert_eq!(triangle.linear_reload_value, 0b010_1010);
    }

    #[test]
    fn registers_2_and_3_assemble_the_period_and_register_3_sets_reload() {
        let mut triangle = Triangle::new();
        triangle.set_enabled(true);
        triangle.write_to_register(2, 0xCD);
        triangle.write_to_register(3, 0b0000_1010);
        assert_eq!(triangle.timer_period, 0x2CD);
        assert_eq!(triangle.length_counter.counter, 254);
        assert!(triangle.linear_reload);
    }

    #[test]
    fn register_1_is_unused() {
        let mut triangle = Triangle::new();
        triangle.write_to_register(1, 0xFF);
        assert_eq!(triangle.timer_period, 0);
        assert_eq!(triangle.linear_reload_value, 0);
    }

    #[test]
    fn output_walks_down_then_up_through_the_sequence() {
        let mut triangle = sounding_triangle();
        for expected in TRIANGLE_SEQUENCE {
            assert_eq!(triangle.output(), expected);
            triangle.clock_timer();
        }
        assert_eq!(triangle.output(), 15); // wrapped around
    }

    #[test]
    fn timer_period_adds_one_clock_per_step() {
        let mut triangle = sounding_triangle();
        triangle.write_to_register(2, 3);
        triangle.clock_timer(); // the timer was at 0: step 1, reload to 3
        for _ in 0..3 {
            triangle.clock_timer();
        }
        assert_eq!(triangle.sequence_step, 1);
        triangle.clock_timer();
        assert_eq!(triangle.sequence_step, 2);
    }

    #[test]
    fn sequencer_holds_while_the_linear_counter_is_zero() {
        let mut triangle = sounding_triangle();
        triangle.clock_timer();
        triangle.clock_timer();
        assert_eq!(triangle.output(), 13);
        triangle.linear_counter = 0;
        triangle.clock_timer();
        assert_eq!(triangle.output(), 13);
    }

    #[test]
    fn sequencer_holds_while_the_length_counter_is_zero() {
        let mut triangle = sounding_triangle();
        triangle.clock_timer();
        triangle.set_enabled(false);
        triangle.clock_timer();
        assert_eq!(triangle.output(), 14);
        assert!(!triangle.is_active());
    }

    #[test]
    fn quarter_frame_reloads_then_counts_the_linear_counter() {
        let mut triangle = Triangle::new();
        triangle.write_to_register(0, 0b0000_0011); // control clear, reload 3
        triangle.write_to_register(3, 0);
        triangle.clock_quarter_frame();
        assert_eq!(triangle.linear_counter, 3);
        assert!(!triangle.linear_reload); // cleared because control is clear
        triangle.clock_quarter_frame();
        assert_eq!(triangle.linear_counter, 2);
    }

    #[test]
    fn control_flag_keeps_reloading_the_linear_counter() {
        let mut triangle = Triangle::new();
        triangle.write_to_register(0, CONTROL_FLAG | 0b0000_0011);
        triangle.write_to_register(3, 0);
        triangle.clock_quarter_frame();
        triangle.clock_quarter_frame();
        assert_eq!(triangle.linear_counter, 3);
        assert!(triangle.linear_reload);
    }

    #[test]
    fn half_frame_clocks_the_length_counter() {
        let mut triangle = sounding_triangle();
        triangle.clock_half_frame();
        assert_eq!(triangle.length_counter.counter, 9);
    }
}
