// https://www.nesdev.org/wiki/APU_Envelope

const LOOP_FLAG: u8 = 0b0010_0000;
const CONSTANT_VOLUME: u8 = 0b0001_0000;
const VOLUME_MASK: u8 = 0b0000_1111;
const DECAY_START: u8 = 15;

pub struct Envelope {
    pub start: bool,
    pub divider: u8,
    pub decay_level: u8,
    pub loop_flag: bool,
    pub constant_volume: bool,
    pub volume: u8,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    pub fn new() -> Self {
        Envelope {
            start: false,
            divider: 0,
            decay_level: 0,
            loop_flag: false,
            constant_volume: false,
            volume: 0,
        }
    }

    // Bits 5-0 of $4000/$4004/$400C: --LC VVVV. V is the volume in constant
    // mode and the divider's reload value otherwise.
    pub fn write(&mut self, data: u8) {
        self.loop_flag = data & LOOP_FLAG != 0;
        self.constant_volume = data & CONSTANT_VOLUME != 0;
        self.volume = data & VOLUME_MASK;
    }

    // Writing the channel's length register restarts the envelope on its
    // next quarter-frame clock.
    pub fn restart(&mut self) {
        self.start = true;
    }

    // Quarter-frame clock.
    pub fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay_level = DECAY_START;
            self.divider = self.volume;
            return;
        }

        if self.divider > 0 {
            self.divider -= 1;
            return;
        }

        self.divider = self.volume;
        if self.decay_level > 0 {
            self.decay_level -= 1;
        } else if self.loop_flag {
            self.decay_level = DECAY_START;
        }
    }

    pub fn output(&self) -> u8 {
        if self.constant_volume {
            self.volume
        } else {
            self.decay_level
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn constant_volume_outputs_the_v_bits_directly() {
        let mut envelope = Envelope::new();
        envelope.write(0b0001_1010);
        assert_eq!(envelope.output(), 10);
    }

    #[test]
    fn first_clock_after_restart_loads_decay_15() {
        let mut envelope = Envelope::new();
        envelope.write(0b0000_0000);
        envelope.restart();
        envelope.clock();
        assert_eq!(envelope.output(), 15);
        assert!(!envelope.start);
    }

    #[test]
    fn decay_steps_down_every_v_plus_1_clocks() {
        let mut envelope = Envelope::new();
        envelope.write(0b0000_0010); // V = 2: one step every 3 quarter frames
        envelope.restart();
        envelope.clock(); // start -> 15, divider 2
        envelope.clock(); // divider 1
        envelope.clock(); // divider 0
        assert_eq!(envelope.output(), 15);
        envelope.clock(); // underflow: reload, decay 14
        assert_eq!(envelope.output(), 14);
    }

    #[test]
    fn decay_stops_at_zero_unless_looping() {
        let mut envelope = Envelope::new();
        envelope.write(0b0000_0000);
        envelope.restart();
        for _ in 0..=16 {
            envelope.clock();
        }
        assert_eq!(envelope.output(), 0);
        envelope.clock();
        assert_eq!(envelope.output(), 0);

        envelope.write(LOOP_FLAG);
        envelope.clock();
        assert_eq!(envelope.output(), 15);
    }

    #[test]
    fn switching_to_constant_volume_keeps_the_decay_running() {
        let mut envelope = Envelope::new();
        envelope.write(0b0000_0000);
        envelope.restart();
        envelope.clock();
        envelope.write(CONSTANT_VOLUME | 0b0011);
        envelope.clock();
        assert_eq!(envelope.output(), 3);
        envelope.write(0b0000_0000);
        assert_eq!(envelope.output(), 14);
    }
}
