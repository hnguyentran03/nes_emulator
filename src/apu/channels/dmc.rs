// https://www.nesdev.org/wiki/APU_DMC

// NTSC CPU cycles between output-level changes, indexed by $4010 bits 3-0.
// Clocked every CPU cycle, so the timer reloads one short.
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];
const IRQ_ENABLED: u8 = 0b1000_0000;
const LOOP_FLAG: u8 = 0b0100_0000;
const RATE_MASK: u8 = 0b0000_1111;
const OUTPUT_LEVEL_MASK: u8 = 0b0111_1111;
const SAMPLE_ADDRESS_START: u16 = 0xC000; // $4012: address = $C000 + A * 64
const SAMPLE_ADDRESS_UNIT: u16 = 64;
const SAMPLE_LENGTH_UNIT: u16 = 16; // $4013: length = L * 16 + 1 bytes
const ADDRESS_WRAP: u16 = 0x8000; // the reader wraps from $FFFF back to $8000
const OUTPUT_STEP: u8 = 2;
const INCREMENT_LIMIT: u8 = 126; // a 1 bit adds 2 only below this
const DECREMENT_LIMIT: u8 = 1; // a 0 bit subtracts 2 only above this
const BITS_PER_SAMPLE_BYTE: u8 = 8;

pub struct DMC {
    pub irq_enabled: bool,
    pub loop_flag: bool,
    pub timer_period: u16,
    pub timer: u16,
    pub output_level: u8,
    pub sample_address: u16,
    pub sample_length: u16,
    pub current_address: u16,
    pub bytes_remaining: u16,
    pub sample_buffer: Option<u8>,
    pub shift_register: u8,
    pub bits_remaining: u8,
    pub silence: bool,
    pub irq_flag: bool,
    // Address the memory reader is waiting on. The APU lives inside the bus
    // and cannot read CPU memory itself, so the bus services this after each
    // tick, the way it copies a page for OAM DMA.
    // TODO: the fetch stalls the CPU for 1-4 cycles; not modelled yet.
    pub fetch_pending: Option<u16>,
}

impl Default for DMC {
    fn default() -> Self {
        Self::new()
    }
}

impl DMC {
    pub fn new() -> Self {
        DMC {
            irq_enabled: false,
            loop_flag: false,
            timer_period: DMC_RATE_TABLE[0] - 1,
            timer: 0,
            output_level: 0,
            sample_address: SAMPLE_ADDRESS_START,
            sample_length: 1,
            current_address: SAMPLE_ADDRESS_START,
            bytes_remaining: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: BITS_PER_SAMPLE_BYTE,
            silence: true,
            irq_flag: false,
            fetch_pending: None,
        }
    }

    // `reg` is the offset from $4010.
    pub fn write_to_register(&mut self, reg: u8, data: u8) {
        match reg {
            // IL-- RRRR
            0 => {
                self.irq_enabled = data & IRQ_ENABLED != 0;
                if !self.irq_enabled {
                    self.irq_flag = false;
                }
                self.loop_flag = data & LOOP_FLAG != 0;
                self.timer_period = DMC_RATE_TABLE[(data & RATE_MASK) as usize] - 1;
            }

            // -DDD DDDD: direct load of the output level
            1 => self.output_level = data & OUTPUT_LEVEL_MASK,

            // AAAA AAAA
            2 => self.sample_address = SAMPLE_ADDRESS_START + data as u16 * SAMPLE_ADDRESS_UNIT,

            // LLLL LLLL
            3 => self.sample_length = data as u16 * SAMPLE_LENGTH_UNIT + 1,

            _ => panic!("Invalid DMC register {}", reg),
        }
    }

    // $4015 bit 4. Clearing it ends playback before the next byte is read;
    // setting it restarts the sample only if nothing is left of the last one.
    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.bytes_remaining = 0;
            self.fetch_pending = None;
        } else if self.bytes_remaining == 0 {
            self.restart_sample();
        }
        self.request_fetch_if_needed();
    }

    fn restart_sample(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    // The memory reader: with the sample buffer empty and bytes left, ask the
    // bus for the next one.
    fn request_fetch_if_needed(&mut self) {
        if self.sample_buffer.is_none() && self.bytes_remaining > 0 && self.fetch_pending.is_none()
        {
            self.fetch_pending = Some(self.current_address);
        }
    }

    pub fn fetch_request(&self) -> Option<u16> {
        self.fetch_pending
    }

    // The bus answers a fetch request with the byte it read.
    pub fn fetch_complete(&mut self, data: u8) {
        self.fetch_pending = None;
        self.sample_buffer = Some(data);
        self.current_address = if self.current_address == u16::MAX {
            ADDRESS_WRAP
        } else {
            self.current_address + 1
        };
        self.bytes_remaining = self.bytes_remaining.saturating_sub(1);
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart_sample();
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
        }
    }

    // Clocked every CPU cycle. Each timer wrap plays one bit of the shift
    // register; after eight the next byte is pulled from the sample buffer,
    // or the output goes quiet until one arrives.
    pub fn clock_timer(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
            return;
        }
        self.timer = self.timer_period;

        if !self.silence {
            if self.shift_register & 1 == 1 {
                if self.output_level < INCREMENT_LIMIT {
                    self.output_level += OUTPUT_STEP;
                }
            } else if self.output_level > DECREMENT_LIMIT {
                self.output_level -= OUTPUT_STEP;
            }
        }
        self.shift_register >>= 1;
        self.bits_remaining -= 1;

        if self.bits_remaining == 0 {
            self.bits_remaining = BITS_PER_SAMPLE_BYTE;
            match self.sample_buffer.take() {
                Some(byte) => {
                    self.shift_register = byte;
                    self.silence = false;
                }
                None => self.silence = true,
            }
            self.request_fetch_if_needed();
        }
    }

    pub fn is_active(&self) -> bool {
        self.bytes_remaining > 0
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Enabled with a 17-byte sample at $C080 and a timer that wraps every clock.
    fn playing_dmc() -> DMC {
        let mut dmc = DMC::new();
        dmc.write_to_register(2, 0x02);
        dmc.write_to_register(3, 0x01);
        dmc.set_enabled(true);
        dmc.timer_period = 0;
        dmc
    }

    #[test]
    fn register_0_sets_irq_loop_and_rate() {
        let mut dmc = DMC::new();
        dmc.write_to_register(0, IRQ_ENABLED | LOOP_FLAG | 0x01);
        assert!(dmc.irq_enabled);
        assert!(dmc.loop_flag);
        assert_eq!(dmc.timer_period, 379);
    }

    #[test]
    fn clearing_irq_enable_clears_a_pending_irq() {
        let mut dmc = DMC::new();
        dmc.irq_flag = true;
        dmc.write_to_register(0, 0x00);
        assert!(!dmc.irq_flag);
    }

    #[test]
    fn register_1_loads_the_output_level_directly() {
        let mut dmc = DMC::new();
        dmc.write_to_register(1, 0xFF);
        assert_eq!(dmc.output(), 127);
    }

    #[test]
    fn registers_2_and_3_scale_the_address_and_length() {
        let mut dmc = DMC::new();
        dmc.write_to_register(2, 0x02);
        dmc.write_to_register(3, 0x01);
        assert_eq!(dmc.sample_address, 0xC080);
        assert_eq!(dmc.sample_length, 17);
    }

    #[test]
    fn enabling_with_nothing_left_restarts_the_sample_and_requests_a_byte() {
        let dmc = playing_dmc();
        assert!(dmc.is_active());
        assert_eq!(dmc.bytes_remaining, 17);
        assert_eq!(dmc.fetch_request(), Some(0xC080));
    }

    #[test]
    fn enabling_mid_sample_does_not_restart_it() {
        let mut dmc = playing_dmc();
        dmc.bytes_remaining = 5;
        dmc.set_enabled(true);
        assert_eq!(dmc.bytes_remaining, 5);
    }

    #[test]
    fn disabling_ends_playback_and_drops_the_pending_fetch() {
        let mut dmc = playing_dmc();
        dmc.set_enabled(false);
        assert!(!dmc.is_active());
        assert_eq!(dmc.fetch_request(), None);
    }

    #[test]
    fn fetch_complete_fills_the_buffer_and_advances_the_reader() {
        let mut dmc = playing_dmc();
        dmc.fetch_complete(0xAB);
        assert_eq!(dmc.sample_buffer, Some(0xAB));
        assert_eq!(dmc.current_address, 0xC081);
        assert_eq!(dmc.bytes_remaining, 16);
        assert_eq!(dmc.fetch_request(), None); // buffer is full
    }

    #[test]
    fn reader_wraps_from_ffff_to_8000() {
        let mut dmc = playing_dmc();
        dmc.current_address = 0xFFFF;
        dmc.fetch_complete(0x00);
        assert_eq!(dmc.current_address, ADDRESS_WRAP);
    }

    #[test]
    fn end_of_sample_loops_when_the_loop_flag_is_set() {
        let mut dmc = playing_dmc();
        dmc.write_to_register(0, LOOP_FLAG | IRQ_ENABLED);
        dmc.bytes_remaining = 1;
        dmc.fetch_complete(0x00);
        assert_eq!(dmc.bytes_remaining, 17);
        assert_eq!(dmc.current_address, 0xC080);
        assert!(!dmc.irq_flag);
    }

    #[test]
    fn end_of_sample_raises_irq_only_when_enabled() {
        let mut dmc = playing_dmc();
        dmc.bytes_remaining = 1;
        dmc.fetch_complete(0x00);
        assert!(!dmc.is_active());
        assert!(!dmc.irq_flag);

        let mut dmc = playing_dmc();
        dmc.write_to_register(0, IRQ_ENABLED);
        dmc.bytes_remaining = 1;
        dmc.fetch_complete(0x00);
        assert!(dmc.irq_flag);
    }

    #[test]
    fn emptying_the_buffer_loads_the_shift_register_and_requests_the_next_byte() {
        let mut dmc = playing_dmc();
        dmc.fetch_complete(0xFF);
        for _ in 0..BITS_PER_SAMPLE_BYTE {
            dmc.clock_timer(); // drains the silent start-up byte
        }
        assert!(!dmc.silence);
        assert_eq!(dmc.shift_register, 0xFF);
        assert_eq!(dmc.sample_buffer, None);
        assert_eq!(dmc.fetch_request(), Some(0xC081));
    }

    #[test]
    fn output_rises_by_two_per_one_bit_and_falls_by_two_per_zero_bit() {
        let mut dmc = DMC::new();
        dmc.timer_period = 0;
        dmc.silence = false;
        dmc.shift_register = 0b0000_1111;
        dmc.output_level = 10;
        for _ in 0..4 {
            dmc.clock_timer();
        }
        assert_eq!(dmc.output(), 18);
        for _ in 0..4 {
            dmc.clock_timer();
        }
        assert_eq!(dmc.output(), 10);
    }

    #[test]
    fn output_clamps_at_the_top_and_bottom() {
        let mut dmc = DMC::new();
        dmc.timer_period = 0;
        dmc.silence = false;
        dmc.shift_register = 0xFF;
        dmc.output_level = 126;
        dmc.clock_timer();
        assert_eq!(dmc.output(), 126);

        dmc.shift_register = 0x00;
        dmc.output_level = 1;
        dmc.clock_timer();
        assert_eq!(dmc.output(), 1);
    }

    #[test]
    fn timer_period_sets_clocks_per_bit() {
        let mut dmc = DMC::new();
        dmc.timer_period = 3;
        dmc.silence = false;
        dmc.shift_register = 0xFF;
        dmc.clock_timer(); // the timer was at 0: one bit, reload to 3
        assert_eq!(dmc.output(), 2);
        for _ in 0..3 {
            dmc.clock_timer();
        }
        assert_eq!(dmc.output(), 2);
        dmc.clock_timer();
        assert_eq!(dmc.output(), 4);
    }

    #[test]
    fn fresh_dmc_clocked_with_empty_buffer_stays_silent() {
        let mut dmc = DMC::new();
        dmc.timer_period = 0;
        for _ in 0..20 {
            dmc.clock_timer();
        }
        assert_eq!(dmc.output(), 0);
        assert!(dmc.silence);
        assert_eq!(dmc.fetch_request(), None);
    }
}
