// https://www.nesdev.org/wiki/APU_Noise

use crate::apu::units::{
    envelope::Envelope,
    length_counter::{HALT_FLAG, LengthCounter},
};

// NTSC timer periods in CPU cycles, indexed by $400E bits 3-0. Because they
// are CPU cycles the timer is clocked every CPU cycle and reloads one short.
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
const MODE_FLAG: u8 = 0b1000_0000;
const PERIOD_MASK: u8 = 0b0000_1111;
// Feedback is bit 0 XOR this bit: 1 normally, 6 in mode 1 (short, tonal noise).
const FEEDBACK_TAP: u16 = 1;
const MODE_FEEDBACK_TAP: u16 = 6;
const FEEDBACK_BIT: u16 = 14;
const SHIFT_REGISTER_START: u16 = 1;

pub struct Noise {
    pub mode: bool,
    pub timer_period: u16,
    pub timer: u16,
    pub shift_register: u16,
    pub envelope: Envelope,
    pub length_counter: LengthCounter,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

impl Noise {
    pub fn new() -> Self {
        Noise {
            mode: false,
            timer_period: NOISE_PERIOD_TABLE[0] - 1,
            timer: 0,
            shift_register: SHIFT_REGISTER_START,
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
        }
    }

    // `reg` is the offset from $400C.
    pub fn write_to_register(&mut self, reg: u8, data: u8) {
        match reg {
            // --LC VVVV
            0 => {
                self.length_counter.halt = data & HALT_FLAG != 0;
                self.envelope.write(data);
            }

            1 => { /* $400D is unused */ }

            // M--- PPPP
            2 => {
                self.mode = data & MODE_FLAG != 0;
                self.timer_period = NOISE_PERIOD_TABLE[(data & PERIOD_MASK) as usize] - 1;
            }

            // LLLL L---
            3 => {
                self.length_counter.load(data);
                self.envelope.restart();
            }

            _ => panic!("Invalid noise register {}", reg),
        }
    }

    // Clocked every CPU cycle. Each timer wrap shifts the 15-bit LFSR right,
    // feeding bit 0 XOR the tap bit back in at the top.
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            let tap = if self.mode {
                MODE_FEEDBACK_TAP
            } else {
                FEEDBACK_TAP
            };
            let feedback = (self.shift_register & 1) ^ ((self.shift_register >> tap) & 1);
            self.shift_register = (self.shift_register >> 1) | (feedback << FEEDBACK_BIT);
        } else {
            self.timer -= 1;
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
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
        if self.shift_register & 1 == 1 || !self.length_counter.is_active() {
            return 0;
        }
        self.envelope.output()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Counts timer underflows until the shift register returns to its start.
    fn lfsr_cycle_length(noise: &mut Noise) -> usize {
        noise.timer_period = 0;
        noise.timer = 0;
        let mut shifts = 0;
        loop {
            noise.clock_timer();
            shifts += 1;
            if noise.shift_register == SHIFT_REGISTER_START {
                return shifts;
            }
        }
    }

    #[test]
    fn shift_register_starts_at_one() {
        assert_eq!(Noise::new().shift_register, SHIFT_REGISTER_START);
    }

    #[test]
    fn register_0_sets_halt_and_envelope() {
        let mut noise = Noise::new();
        noise.write_to_register(0, 0b0011_0111);
        assert!(noise.length_counter.halt);
        assert!(noise.envelope.constant_volume);
        assert_eq!(noise.envelope.volume, 7);
    }

    #[test]
    fn register_2_sets_mode_and_looks_up_the_period() {
        let mut noise = Noise::new();
        noise.write_to_register(2, MODE_FLAG | 0x0F);
        assert!(noise.mode);
        assert_eq!(noise.timer_period, 4067);
        noise.write_to_register(2, 0x00);
        assert!(!noise.mode);
        assert_eq!(noise.timer_period, 3);
    }

    #[test]
    fn register_3_loads_length_and_restarts_the_envelope() {
        let mut noise = Noise::new();
        noise.set_enabled(true);
        noise.write_to_register(3, 0b0000_1000);
        assert_eq!(noise.length_counter.counter, 254);
        assert!(noise.envelope.start);
    }

    #[test]
    fn timer_shifts_once_per_period_plus_one_clocks() {
        let mut noise = Noise::new();
        noise.write_to_register(2, 0x00); // period 3 -> 4 clocks per shift
        noise.clock_timer(); // the timer was at 0: shifts and reloads to 3
        let after_first = noise.shift_register;
        for _ in 0..3 {
            noise.clock_timer();
        }
        assert_eq!(noise.shift_register, after_first);
        noise.clock_timer();
        assert_ne!(noise.shift_register, after_first);
    }

    #[test]
    fn mode_0_lfsr_repeats_every_32767_shifts() {
        let mut noise = Noise::new();
        assert_eq!(lfsr_cycle_length(&mut noise), 32767);
    }

    #[test]
    fn mode_1_lfsr_repeats_every_93_shifts() {
        let mut noise = Noise::new();
        noise.write_to_register(2, MODE_FLAG);
        assert_eq!(lfsr_cycle_length(&mut noise), 93);
    }

    #[test]
    fn output_is_volume_only_while_bit_0_is_clear_and_length_runs() {
        let mut noise = Noise::new();
        noise.set_enabled(true);
        noise.write_to_register(0, 0b0001_1111); // constant volume 15
        noise.write_to_register(3, 0);
        noise.shift_register = 0b10;
        assert_eq!(noise.output(), 15);
        noise.shift_register = 0b11;
        assert_eq!(noise.output(), 0);
        noise.shift_register = 0b10;
        noise.set_enabled(false);
        assert_eq!(noise.output(), 0);
    }

    #[test]
    fn frame_clocks_reach_the_envelope_and_length_counter() {
        let mut noise = Noise::new();
        noise.set_enabled(true);
        noise.write_to_register(0, 0);
        noise.write_to_register(3, 0);
        noise.clock_quarter_frame();
        assert_eq!(noise.envelope.decay_level, 15);
        noise.clock_half_frame();
        assert_eq!(noise.length_counter.counter, 9);
    }
}
