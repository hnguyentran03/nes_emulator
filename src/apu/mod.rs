// https://www.nesdev.org/wiki/APU

use crate::apu::channels::{dmc::DMC, noise::Noise, pulse::Pulse, triangle::Triangle};
use crate::apu::frame_counter::{FrameClock, FrameCounter};

pub mod channels;
pub mod frame_counter;
pub mod units;

const CPU_CLOCK_HZ: f64 = 1_789_773.0; // NTSC
pub const SAMPLE_RATE_HZ: f64 = 44_100.0;
const CPU_CYCLES_PER_SAMPLE: f64 = CPU_CLOCK_HZ / SAMPLE_RATE_HZ; // ~40.58

const PULSE_1_REGISTERS: u16 = 0x4000;
const PULSE_2_REGISTERS: u16 = 0x4004;
const TRIANGLE_REGISTERS: u16 = 0x4008;
const NOISE_REGISTERS: u16 = 0x400C;
const DMC_REGISTERS: u16 = 0x4010;
const DMC_REGISTERS_END: u16 = 0x4013;
const REGISTERS_PER_CHANNEL: u16 = 4;

bitflags! {
    // $4015. Written: channel enables. Read: the same bits report whether
    // each length counter (for the DMC, bytes remaining) is non-zero, plus
    // the two interrupt flags.
    // https://www.nesdev.org/wiki/APU#Status_($4015)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StatusRegister: u8 {
        const PULSE_1   = 0b0000_0001;
        const PULSE_2   = 0b0000_0010;
        const TRIANGLE  = 0b0000_0100;
        const NOISE     = 0b0000_1000;
        const DMC       = 0b0001_0000;
        const FRAME_IRQ = 0b0100_0000;
        const DMC_IRQ   = 0b1000_0000;
    }
}

lazy_static! {
    // Non-linear mixer, as lookup tables. pulse_out is indexed by
    // pulse1 + pulse2, tnd_out by 3 * triangle + 2 * noise + dmc; their sum
    // is the output level in 0.0..=1.0.
    // https://www.nesdev.org/wiki/APU_Mixer#Lookup_Table
    static ref PULSE_TABLE: [f32; 31] = {
        let mut table = [0.0; 31];
        for (n, entry) in table.iter_mut().enumerate().skip(1) {
            *entry = 95.52 / (8128.0 / n as f32 + 100.0);
        }
        table
    };
    static ref TND_TABLE: [f32; 203] = {
        let mut table = [0.0; 203];
        for (n, entry) in table.iter_mut().enumerate().skip(1) {
            *entry = 163.67 / (24329.0 / n as f32 + 100.0);
        }
        table
    };
}

pub struct APU {
    pub pulse_1: Pulse,
    pub pulse_2: Pulse,
    pub triangle: Triangle,
    pub noise: Noise,
    pub dmc: DMC,
    pub frame_counter: FrameCounter,

    cycles: usize,     // CPU cycles since power-up; the pulse timers step on even ones
    sample_clock: f64, // CPU cycles until the next output sample
    sample_sum: f32,   // mixer output accumulated since the last sample
    sample_count: u32,
    samples: Vec<f32>, // drained by the bus once per frame
}

impl Default for APU {
    fn default() -> Self {
        Self::new()
    }
}

impl APU {
    pub fn new() -> Self {
        APU {
            pulse_1: Pulse::new(true),
            pulse_2: Pulse::new(false),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: DMC::new(),
            frame_counter: FrameCounter::new(),

            cycles: 0,
            sample_clock: CPU_CYCLES_PER_SAMPLE,
            sample_sum: 0.0,
            sample_count: 0,
            samples: Vec::new(),
        }
    }

    // $4000-$4013: four registers per channel.
    pub fn write_to_register(&mut self, addr: u16, data: u8) {
        let reg = (addr % REGISTERS_PER_CHANNEL) as u8;
        match addr {
            PULSE_1_REGISTERS..PULSE_2_REGISTERS => self.pulse_1.write_to_register(reg, data),
            PULSE_2_REGISTERS..TRIANGLE_REGISTERS => self.pulse_2.write_to_register(reg, data),
            TRIANGLE_REGISTERS..NOISE_REGISTERS => self.triangle.write_to_register(reg, data),
            NOISE_REGISTERS..DMC_REGISTERS => self.noise.write_to_register(reg, data),
            DMC_REGISTERS..=DMC_REGISTERS_END => self.dmc.write_to_register(reg, data),
            _ => panic!("Invalid APU register {:x}", addr),
        }
    }

    // $4015 write: ---D NT21. Any write also acknowledges the DMC interrupt.
    pub fn write_to_status(&mut self, data: u8) {
        let status = StatusRegister::from_bits_truncate(data);
        self.pulse_1
            .set_enabled(status.contains(StatusRegister::PULSE_1));
        self.pulse_2
            .set_enabled(status.contains(StatusRegister::PULSE_2));
        self.triangle
            .set_enabled(status.contains(StatusRegister::TRIANGLE));
        self.noise
            .set_enabled(status.contains(StatusRegister::NOISE));
        self.dmc.set_enabled(status.contains(StatusRegister::DMC));
        self.dmc.irq_flag = false;
    }

    // $4015 read: IF-D NT21. Acknowledges the frame interrupt but not the DMC's.
    pub fn read_status(&mut self) -> u8 {
        let mut status = StatusRegister::empty();
        status.set(StatusRegister::PULSE_1, self.pulse_1.is_active());
        status.set(StatusRegister::PULSE_2, self.pulse_2.is_active());
        status.set(StatusRegister::TRIANGLE, self.triangle.is_active());
        status.set(StatusRegister::NOISE, self.noise.is_active());
        status.set(StatusRegister::DMC, self.dmc.is_active());
        status.set(StatusRegister::FRAME_IRQ, self.frame_counter.irq_flag);
        status.set(StatusRegister::DMC_IRQ, self.dmc.irq_flag);
        self.frame_counter.clear_irq_flag();
        status.bits()
    }

    // $4017 write.
    pub fn write_to_frame_counter(&mut self, data: u8) {
        let clock = self.frame_counter.write(data);
        self.clock_frame(clock);
    }

    // Advances `cycles` CPU cycles, one at a time so register writes between
    // instructions land on the right cycle.
    pub fn tick(&mut self, cycles: usize) {
        for _ in 0..cycles {
            self.tick_cycle();
        }
    }

    fn tick_cycle(&mut self) {
        self.cycles += 1;

        let clock = self.frame_counter.tick();
        self.clock_frame(clock);

        // The triangle, noise and DMC timers run at the CPU clock; the pulse
        // timers at half that (one APU cycle).
        self.triangle.clock_timer();
        self.noise.clock_timer();
        self.dmc.clock_timer();
        if self.cycles.is_multiple_of(2) {
            self.pulse_1.clock_timer();
            self.pulse_2.clock_timer();
        }

        // Every cycle's output is averaged into the next sample, which is
        // also what keeps the high-frequency content from aliasing.
        self.sample_sum += self.mix();
        self.sample_count += 1;
        self.sample_clock -= 1.0;
        if self.sample_clock <= 0.0 {
            self.sample_clock += CPU_CYCLES_PER_SAMPLE;
            self.samples
                .push(self.sample_sum / self.sample_count as f32);
            self.sample_sum = 0.0;
            self.sample_count = 0;
        }
    }

    fn clock_frame(&mut self, clock: FrameClock) {
        if clock.quarter {
            self.pulse_1.clock_quarter_frame();
            self.pulse_2.clock_quarter_frame();
            self.triangle.clock_quarter_frame();
            self.noise.clock_quarter_frame();
        }
        if clock.half {
            self.pulse_1.clock_half_frame();
            self.pulse_2.clock_half_frame();
            self.triangle.clock_half_frame();
            self.noise.clock_half_frame();
        }
    }

    // https://www.nesdev.org/wiki/APU_Mixer
    fn mix(&self) -> f32 {
        let pulse = (self.pulse_1.output() + self.pulse_2.output()) as usize;
        let tnd = 3 * self.triangle.output() as usize
            + 2 * self.noise.output() as usize
            + self.dmc.output() as usize;
        PULSE_TABLE[pulse] + TND_TABLE[tnd]
    }

    pub fn irq_pending(&self) -> bool {
        self.frame_counter.irq_flag || self.dmc.irq_flag
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn clear_samples(&mut self) {
        self.samples.clear();
    }

    pub fn dmc_fetch_request(&self) -> Option<u16> {
        self.dmc.fetch_request()
    }

    pub fn dmc_fetch_complete(&mut self, data: u8) {
        self.dmc.fetch_complete(data);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Pulse 1 enabled at 50% duty, halt clear, constant volume 15, period 8,
    // length 10.
    fn apu_with_sounding_pulse_1() -> APU {
        let mut apu = APU::new();
        apu.write_to_status(StatusRegister::PULSE_1.bits());
        apu.write_to_register(0x4000, 0b1001_1111);
        apu.write_to_register(0x4002, 8);
        apu.write_to_register(0x4003, 0);
        apu
    }

    #[test]
    fn register_writes_route_to_the_right_channel() {
        let mut apu = APU::new();
        apu.write_to_register(0x4002, 0x11);
        apu.write_to_register(0x4006, 0x22);
        apu.write_to_register(0x400A, 0x33);
        apu.write_to_register(0x400E, 0x0F);
        apu.write_to_register(0x4011, 0x44);
        assert_eq!(apu.pulse_1.timer_period, 0x11);
        assert_eq!(apu.pulse_2.timer_period, 0x22);
        assert_eq!(apu.triangle.timer_period, 0x33);
        assert_eq!(apu.noise.timer_period, 4067);
        assert_eq!(apu.dmc.output_level, 0x44);
    }

    #[test]
    #[should_panic(expected = "Invalid APU register 4014")]
    fn register_write_outside_the_channels_panics() {
        let mut apu = APU::new();
        apu.write_to_register(0x4014, 0);
    }

    #[test]
    fn status_write_enables_channels_and_clears_the_dmc_irq() {
        let mut apu = APU::new();
        apu.dmc.irq_flag = true;
        apu.write_to_status(StatusRegister::PULSE_2.bits() | StatusRegister::DMC.bits());
        assert!(!apu.pulse_1.length_counter.enabled);
        assert!(apu.pulse_2.length_counter.enabled);
        assert!(apu.dmc.is_active());
        assert!(!apu.dmc.irq_flag);
    }

    #[test]
    fn status_read_reports_active_channels_and_irqs_and_clears_the_frame_irq() {
        let mut apu = apu_with_sounding_pulse_1();
        apu.frame_counter.irq_flag = true;
        apu.dmc.irq_flag = true;
        let status = StatusRegister::from_bits_truncate(apu.read_status());
        assert_eq!(
            status,
            StatusRegister::PULSE_1 | StatusRegister::FRAME_IRQ | StatusRegister::DMC_IRQ
        );
        assert!(!apu.frame_counter.irq_flag);
        assert!(apu.dmc.irq_flag);
        let status = StatusRegister::from_bits_truncate(apu.read_status());
        assert!(!status.contains(StatusRegister::FRAME_IRQ));
    }

    #[test]
    fn irq_pending_is_either_interrupt_flag() {
        let mut apu = APU::new();
        assert!(!apu.irq_pending());
        apu.frame_counter.irq_flag = true;
        assert!(apu.irq_pending());
        apu.frame_counter.irq_flag = false;
        apu.dmc.irq_flag = true;
        assert!(apu.irq_pending());
    }

    #[test]
    fn five_step_frame_counter_write_clocks_the_channels_at_once() {
        let mut apu = apu_with_sounding_pulse_1();
        assert!(apu.pulse_1.envelope.start);
        apu.write_to_frame_counter(0b1000_0000);
        assert!(!apu.pulse_1.envelope.start);
        assert_eq!(apu.pulse_1.length_counter.counter, 9);
    }

    #[test]
    fn tick_fans_out_quarter_and_half_frames() {
        let mut apu = apu_with_sounding_pulse_1();
        apu.tick(7457);
        assert_eq!(apu.pulse_1.envelope.decay_level, 15);
        assert_eq!(apu.pulse_1.length_counter.counter, 10);
        apu.tick(14913 - 7457);
        assert_eq!(apu.pulse_1.length_counter.counter, 9);
    }

    #[test]
    fn frame_irq_arrives_after_the_fourth_step() {
        let mut apu = APU::new();
        apu.tick(29828);
        assert!(!apu.irq_pending());
        apu.tick(1);
        assert!(apu.irq_pending());
    }

    #[test]
    fn pulse_timers_run_at_half_the_cpu_clock_and_triangle_at_full() {
        let mut apu = apu_with_sounding_pulse_1();
        apu.write_to_status(StatusRegister::PULSE_1.bits() | StatusRegister::TRIANGLE.bits());
        apu.write_to_register(0x400B, 0); // triangle length 10
        apu.triangle.linear_counter = 1;

        apu.tick(1);
        assert_eq!(apu.triangle.sequence_step, 1);
        assert_eq!(apu.pulse_1.sequence_step, 0);
        apu.tick(1);
        assert_eq!(apu.pulse_1.sequence_step, 1);
        apu.tick(16); // period 8: eight more pulse clocks count the timer down
        assert_eq!(apu.pulse_1.sequence_step, 1);
        apu.tick(2);
        assert_eq!(apu.pulse_1.sequence_step, 2);
    }

    #[test]
    fn one_second_of_cycles_yields_one_second_of_samples() {
        let mut apu = APU::new();
        apu.tick(CPU_CLOCK_HZ as usize);
        let count = apu.samples().len();
        assert!((44_099..=44_101).contains(&count), "{count} samples");
        apu.clear_samples();
        assert!(apu.samples().is_empty());
    }

    // A fresh triangle rests at step 0 of its sequence, which outputs 15, so
    // an idle APU carries that constant offset (tnd index 3 * 15 = 45).
    #[test]
    fn idle_apu_outputs_only_the_triangle_rest_level() {
        let mut apu = APU::new();
        apu.tick(1000);
        let rest = TND_TABLE[45];
        assert!(apu.samples().iter().all(|&s| (s - rest).abs() < 1e-6));
    }

    #[test]
    fn mixer_tables_match_the_formula_endpoints() {
        assert_eq!(PULSE_TABLE[0], 0.0);
        assert!((PULSE_TABLE[30] - 0.2575).abs() < 1e-3);
        assert_eq!(TND_TABLE[0], 0.0);
        assert!((TND_TABLE[202] - 0.7425).abs() < 1e-3);
        assert!(PULSE_TABLE[30] + TND_TABLE[202] <= 1.0 + 1e-3);
    }

    #[test]
    fn mix_reads_the_pulse_table() {
        let mut apu = apu_with_sounding_pulse_1();
        apu.pulse_1.sequence_step = 1; // a 1 in the 50% duty sequence
        assert_eq!(apu.mix(), PULSE_TABLE[15] + TND_TABLE[45]);
    }

    #[test]
    fn dmc_fetch_seam_round_trips_through_the_apu() {
        let mut apu = APU::new();
        apu.write_to_register(0x4012, 0x00);
        apu.write_to_register(0x4013, 0x00);
        apu.write_to_status(StatusRegister::DMC.bits());
        assert_eq!(apu.dmc_fetch_request(), Some(0xC000));
        apu.dmc_fetch_complete(0xAA);
        assert_eq!(apu.dmc_fetch_request(), None);
        assert_eq!(apu.dmc.sample_buffer, Some(0xAA));
    }
}
