// https://www.nesdev.org/wiki/APU_Frame_Counter

// Step timings in CPU cycles. The wiki lists APU cycles (3728.5, 7456.5,
// 11185.5, 14914.5, 18640.5), which are half as many.
const STEP_1: usize = 7457;
const STEP_2: usize = 14913;
const STEP_3: usize = 22371;
const STEP_4: usize = 29829; // 4-step mode only: also raises the frame IRQ
const STEP_5: usize = 37281; // 5-step mode only
const FOUR_STEP_LENGTH: usize = 29830;
const FIVE_STEP_LENGTH: usize = 37282;
const FIVE_STEP_MODE: u8 = 0b1000_0000;
const IRQ_INHIBIT: u8 = 0b0100_0000;

// Which of the two frame clocks fired on a given cycle. Quarter frames clock
// envelopes and the triangle's linear counter; half frames clock length
// counters and sweeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameClock {
    pub quarter: bool,
    pub half: bool,
}

pub struct FrameCounter {
    pub five_step_mode: bool,
    pub irq_inhibit: bool,
    pub irq_flag: bool,
    cycles: usize,
}

impl Default for FrameCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameCounter {
    // Power-up behaves as if $00 were written: 4-step mode with IRQs enabled.
    pub fn new() -> Self {
        FrameCounter {
            five_step_mode: false,
            irq_inhibit: false,
            irq_flag: false,
            cycles: 0,
        }
    }

    // $4017: MI-- ----. Resets the sequence; in 5-step mode the write itself
    // clocks both frames, which the caller must fan out. The hardware's 3-4
    // cycle delay before the reset takes effect is not modelled.
    pub fn write(&mut self, data: u8) -> FrameClock {
        self.five_step_mode = data & FIVE_STEP_MODE != 0;
        self.irq_inhibit = data & IRQ_INHIBIT != 0;
        if self.irq_inhibit {
            self.irq_flag = false;
        }
        self.cycles = 0;
        FrameClock {
            quarter: self.five_step_mode,
            half: self.five_step_mode,
        }
    }

    // Advances one CPU cycle. The flag is level-held: once set it stays up
    // until $4015 is read or inhibit is written, so a handler that does not
    // acknowledge it is interrupted again.
    pub fn tick(&mut self) -> FrameClock {
        self.cycles += 1;
        let mut clock = FrameClock::default();

        match (self.five_step_mode, self.cycles) {
            (_, STEP_1) | (_, STEP_3) => clock.quarter = true,
            (_, STEP_2) => {
                clock.quarter = true;
                clock.half = true;
            }
            (false, STEP_4) => {
                clock.quarter = true;
                clock.half = true;
                if !self.irq_inhibit {
                    self.irq_flag = true;
                }
            }
            (true, STEP_5) => {
                clock.quarter = true;
                clock.half = true;
            }
            _ => {}
        }

        let length = if self.five_step_mode {
            FIVE_STEP_LENGTH
        } else {
            FOUR_STEP_LENGTH
        };
        if self.cycles >= length {
            self.cycles = 0;
        }

        clock
    }

    // Reading $4015 acknowledges the frame interrupt.
    pub fn clear_irq_flag(&mut self) {
        self.irq_flag = false;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Ticks `cycles` times and returns the 1-based cycles on which quarter
    // and half frames fired.
    fn clock_cycles(frame_counter: &mut FrameCounter, cycles: usize) -> (Vec<usize>, Vec<usize>) {
        let mut quarters = Vec::new();
        let mut halves = Vec::new();
        for cycle in 1..=cycles {
            let clock = frame_counter.tick();
            if clock.quarter {
                quarters.push(cycle);
            }
            if clock.half {
                halves.push(cycle);
            }
        }
        (quarters, halves)
    }

    #[test]
    fn four_step_mode_clocks_at_the_documented_cycles_and_repeats() {
        let mut frame_counter = FrameCounter::new();
        let (quarters, halves) = clock_cycles(&mut frame_counter, FOUR_STEP_LENGTH + STEP_1);
        assert_eq!(quarters, [7457, 14913, 22371, 29829, 29830 + 7457]);
        assert_eq!(halves, [14913, 29829]);
    }

    #[test]
    fn four_step_mode_raises_the_frame_irq_at_the_last_step() {
        let mut frame_counter = FrameCounter::new();
        clock_cycles(&mut frame_counter, STEP_4 - 1);
        assert!(!frame_counter.irq_flag);
        frame_counter.tick();
        assert!(frame_counter.irq_flag);
    }

    #[test]
    fn five_step_mode_skips_the_fourth_step_and_never_raises_irq() {
        let mut frame_counter = FrameCounter::new();
        frame_counter.write(FIVE_STEP_MODE);
        let (quarters, halves) = clock_cycles(&mut frame_counter, FIVE_STEP_LENGTH + STEP_1);
        assert_eq!(quarters, [7457, 14913, 22371, 37281, 37282 + 7457]);
        assert_eq!(halves, [14913, 37281]);
        assert!(!frame_counter.irq_flag);
    }

    #[test]
    fn irq_inhibit_blocks_and_clears_the_flag() {
        let mut frame_counter = FrameCounter::new();
        frame_counter.write(IRQ_INHIBIT);
        clock_cycles(&mut frame_counter, FOUR_STEP_LENGTH);
        assert!(!frame_counter.irq_flag);

        frame_counter.irq_flag = true;
        frame_counter.write(IRQ_INHIBIT);
        assert!(!frame_counter.irq_flag);
    }

    #[test]
    fn write_restarts_the_sequence() {
        let mut frame_counter = FrameCounter::new();
        clock_cycles(&mut frame_counter, 5000);
        frame_counter.write(0);
        let (quarters, _) = clock_cycles(&mut frame_counter, STEP_1);
        assert_eq!(quarters, [STEP_1]);
    }

    #[test]
    fn five_step_write_clocks_both_frames_immediately() {
        let mut frame_counter = FrameCounter::new();
        assert_eq!(
            frame_counter.write(FIVE_STEP_MODE),
            FrameClock {
                quarter: true,
                half: true
            }
        );
        assert_eq!(frame_counter.write(0), FrameClock::default());
    }

    #[test]
    fn clear_irq_flag_drops_the_flag() {
        let mut frame_counter = FrameCounter::new();
        frame_counter.irq_flag = true;
        frame_counter.clear_irq_flag();
        assert!(!frame_counter.irq_flag);
    }
}
