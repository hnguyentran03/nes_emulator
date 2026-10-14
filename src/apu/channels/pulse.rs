// https://www.nesdev.org/wiki/APU_Pulse

use crate::apu::units::{
    envelope::Envelope,
    length_counter::{HALT_FLAG, LengthCounter},
    sweep::Sweep,
};

// The 8-step waveform for each duty setting ($4000 bits 7-6): 12.5%, 25%,
// 50% and 25% negated.
const DUTY_SEQUENCES: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];
const SEQUENCE_LENGTH: u8 = 8;
const DUTY_SHIFT: u8 = 6;
const TIMER_LOW_MASK: u16 = 0x00FF;
const TIMER_HIGH_MASK: u8 = 0b0000_0111;

pub struct Pulse {
    pub duty: u8,
    pub sequence_step: u8,
    pub timer_period: u16,
    pub timer: u16,
    pub envelope: Envelope,
    pub sweep: Sweep,
    pub length_counter: LengthCounter,
}

impl Pulse {
    // Pulse 1 passes `true`: its sweep negates with the ones' complement.
    pub fn new(ones_complement: bool) -> Self {
        Pulse {
            duty: 0,
            sequence_step: 0,
            timer_period: 0,
            timer: 0,
            envelope: Envelope::new(),
            sweep: Sweep::new(ones_complement),
            length_counter: LengthCounter::new(),
        }
    }

    // `reg` is the offset from the channel's base address ($4000 or $4004).
    pub fn write_to_register(&mut self, reg: u8, data: u8) {
        match reg {
            // DDLC VVVV
            0 => {
                self.duty = data >> DUTY_SHIFT;
                self.length_counter.halt = data & HALT_FLAG != 0;
                self.envelope.write(data);
            }

            // EPPP NSSS
            1 => self.sweep.write(data),

            // TTTT TTTT: timer low
            2 => self.timer_period = (self.timer_period & !TIMER_LOW_MASK) | data as u16,

            // LLLL LTTT: length load and timer high. Also restarts the
            // envelope and the waveform.
            3 => {
                self.timer_period =
                    (self.timer_period & TIMER_LOW_MASK) | ((data & TIMER_HIGH_MASK) as u16) << 8;
                self.length_counter.load(data);
                self.sequence_step = 0;
                self.envelope.restart();
            }

            _ => panic!("Invalid pulse register {}", reg),
        }
    }

    // Clocked every other CPU cycle. The timer counts down from the period
    // and advances the waveform one step when it wraps, so each step lasts
    // period + 1 clocks.
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.sequence_step = (self.sequence_step + 1) % SEQUENCE_LENGTH;
        } else {
            self.timer -= 1;
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
        self.sweep.clock(&mut self.timer_period);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.length_counter.set_enabled(enabled);
    }

    pub fn is_active(&self) -> bool {
        self.length_counter.is_active()
    }

    pub fn output(&self) -> u8 {
        let duty_bit = DUTY_SEQUENCES[self.duty as usize][self.sequence_step as usize];
        if duty_bit == 0 || self.sweep.mutes(self.timer_period) || !self.length_counter.is_active()
        {
            return 0;
        }
        self.envelope.output()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Duty 1 (25%), constant volume 15, period 8 (9 timer clocks per step),
    // length 10. The sweep is left disabled.
    fn sounding_pulse() -> Pulse {
        let mut pulse = Pulse::new(true);
        pulse.set_enabled(true);
        pulse.write_to_register(0, 0b0101_1111);
        pulse.write_to_register(2, 8);
        pulse.write_to_register(3, 0);
        pulse
    }

    #[test]
    fn register_0_sets_duty_halt_and_envelope() {
        let mut pulse = Pulse::new(true);
        pulse.write_to_register(0, 0b1011_0101);
        assert_eq!(pulse.duty, 2);
        assert!(pulse.length_counter.halt);
        assert!(pulse.envelope.loop_flag);
        assert!(pulse.envelope.constant_volume);
        assert_eq!(pulse.envelope.volume, 5);
    }

    #[test]
    fn registers_2_and_3_assemble_the_11_bit_period() {
        let mut pulse = Pulse::new(true);
        pulse.write_to_register(2, 0xAB);
        pulse.write_to_register(3, 0b0000_0101);
        assert_eq!(pulse.timer_period, 0x5AB);
        pulse.write_to_register(2, 0x01);
        assert_eq!(pulse.timer_period, 0x501);
    }

    #[test]
    fn register_3_loads_length_restarts_envelope_and_resets_the_sequence() {
        let mut pulse = Pulse::new(true);
        pulse.set_enabled(true);
        pulse.sequence_step = 5;
        pulse.write_to_register(3, 0b0000_1000); // length index 1
        assert_eq!(pulse.length_counter.counter, 254);
        assert!(pulse.envelope.start);
        assert_eq!(pulse.sequence_step, 0);
    }

    #[test]
    fn output_follows_the_duty_sequence_one_step_per_period() {
        let mut pulse = sounding_pulse();
        let outputs: Vec<u8> = (0..8)
            .map(|_| {
                let out = pulse.output();
                for _ in 0..9 {
                    pulse.clock_timer();
                }
                out
            })
            .collect();
        assert_eq!(outputs, [0, 15, 15, 0, 0, 0, 0, 0]);
        assert_eq!(pulse.sequence_step, 0); // wrapped around
    }

    #[test]
    fn period_below_8_is_silent() {
        let mut pulse = sounding_pulse();
        pulse.write_to_register(2, 7);
        pulse.sequence_step = 1;
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn expired_length_counter_is_silent() {
        let mut pulse = sounding_pulse();
        pulse.sequence_step = 1;
        assert_eq!(pulse.output(), 15);
        for _ in 0..10 {
            pulse.clock_half_frame();
        }
        assert!(!pulse.is_active());
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn disabling_silences_the_channel() {
        let mut pulse = sounding_pulse();
        pulse.sequence_step = 1;
        pulse.set_enabled(false);
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn half_frame_clock_runs_the_sweep() {
        let mut pulse = sounding_pulse();
        pulse.write_to_register(2, 100);
        pulse.write_to_register(1, 0b1000_0001); // enabled, period 0, shift 1
        pulse.clock_half_frame();
        assert_eq!(pulse.timer_period, 150);
    }

    #[test]
    fn quarter_frame_clock_runs_the_envelope() {
        let mut pulse = Pulse::new(true);
        pulse.set_enabled(true);
        pulse.write_to_register(0, 0b0000_0000);
        pulse.write_to_register(3, 0);
        pulse.clock_quarter_frame();
        assert_eq!(pulse.envelope.decay_level, 15);
    }
}
