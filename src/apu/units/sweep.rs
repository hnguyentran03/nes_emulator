// https://www.nesdev.org/wiki/APU_Sweep

const ENABLED: u8 = 0b1000_0000;
const PERIOD_MASK: u8 = 0b0111_0000;
const PERIOD_SHIFT: u8 = 4;
const NEGATE: u8 = 0b0000_1000;
const SHIFT_MASK: u8 = 0b0000_0111;
const MIN_PERIOD: u16 = 8;
const MAX_PERIOD: u16 = 0x7FF;

pub struct Sweep {
    pub enabled: bool,
    pub period: u8,
    pub negate: bool,
    pub shift: u8,
    pub divider: u8,
    pub reload: bool,
    // Pulse 1 negates with the ones' complement (-change - 1), pulse 2 with
    // the two's complement (-change).
    pub ones_complement: bool,
}

impl Sweep {
    pub fn new(ones_complement: bool) -> Self {
        Sweep {
            enabled: false,
            period: 0,
            negate: false,
            shift: 0,
            divider: 0,
            reload: false,
            ones_complement,
        }
    }

    // $4001/$4005: EPPP NSSS. The write only takes effect on the next
    // half-frame clock.
    pub fn write(&mut self, data: u8) {
        self.enabled = data & ENABLED != 0;
        self.period = (data & PERIOD_MASK) >> PERIOD_SHIFT;
        self.negate = data & NEGATE != 0;
        self.shift = data & SHIFT_MASK;
        self.reload = true;
    }

    pub fn target_period(&self, current: u16) -> u16 {
        let change = current >> self.shift;
        if self.negate {
            let change = if self.ones_complement {
                change + 1
            } else {
                change
            };
            current.saturating_sub(change)
        } else {
            current + change
        }
    }

    // The channel is silenced whenever its period is below 8 or the sweep's
    // target would overflow, whether or not the sweep is enabled.
    pub fn mutes(&self, current: u16) -> bool {
        current < MIN_PERIOD || self.target_period(current) > MAX_PERIOD
    }

    // Half-frame clock. Writes the target into the channel's timer period
    // when the divider expires, then reloads or counts the divider.
    pub fn clock(&mut self, timer_period: &mut u16) {
        if self.divider == 0 && self.enabled && self.shift != 0 && !self.mutes(*timer_period) {
            *timer_period = self.target_period(*timer_period);
        }

        if self.divider == 0 || self.reload {
            self.divider = self.period;
            self.reload = false;
        } else {
            self.divider -= 1;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn write_sets_fields_and_reload_flag() {
        let mut sweep = Sweep::new(true);
        sweep.write(0b1011_1010);
        assert!(sweep.enabled);
        assert_eq!(sweep.period, 3);
        assert!(sweep.negate);
        assert_eq!(sweep.shift, 2);
        assert!(sweep.reload);
    }

    #[test]
    fn target_adds_the_shifted_period() {
        let mut sweep = Sweep::new(true);
        sweep.write(0b0000_0010); // shift 2
        assert_eq!(sweep.target_period(100), 125);
    }

    #[test]
    fn pulse_1_negates_with_ones_complement_and_pulse_2_with_twos() {
        let mut pulse_1 = Sweep::new(true);
        pulse_1.write(NEGATE | 0b010);
        assert_eq!(pulse_1.target_period(100), 74);

        let mut pulse_2 = Sweep::new(false);
        pulse_2.write(NEGATE | 0b010);
        assert_eq!(pulse_2.target_period(100), 75);
    }

    #[test]
    fn negative_target_clamps_to_zero() {
        let mut sweep = Sweep::new(true);
        sweep.write(NEGATE);
        assert_eq!(sweep.target_period(20), 0);
    }

    #[test]
    fn mutes_below_8_and_when_the_target_overflows_even_if_disabled() {
        let mut sweep = Sweep::new(false);
        sweep.write(0b0000_0001); // disabled, shift 1
        assert!(sweep.mutes(7));
        assert!(!sweep.mutes(8));
        assert!(sweep.mutes(0x600)); // 0x600 + 0x300 > 0x7FF
        assert!(!sweep.mutes(0x500));
    }

    #[test]
    fn clock_updates_the_period_when_the_divider_hits_zero() {
        let mut sweep = Sweep::new(false);
        sweep.write(ENABLED | (1 << PERIOD_SHIFT) | 0b001); // period 1, shift 1
        let mut timer_period = 100u16;
        sweep.clock(&mut timer_period); // divider 0 and reload: update, then reload to 1
        assert_eq!(timer_period, 150);
        sweep.clock(&mut timer_period); // divider 1 -> 0, no update
        assert_eq!(timer_period, 150);
        sweep.clock(&mut timer_period); // divider 0: update
        assert_eq!(timer_period, 225);
    }

    #[test]
    fn clock_does_not_update_when_disabled_shift_zero_or_muting() {
        let mut timer_period = 100u16;

        let mut disabled = Sweep::new(false);
        disabled.write(0b0000_0001);
        disabled.clock(&mut timer_period);
        assert_eq!(timer_period, 100);

        let mut shift_zero = Sweep::new(false);
        shift_zero.write(ENABLED);
        shift_zero.clock(&mut timer_period);
        assert_eq!(timer_period, 100);

        let mut muting = Sweep::new(false);
        muting.write(ENABLED | 0b001);
        let mut big = 0x700u16;
        muting.clock(&mut big);
        assert_eq!(big, 0x700);
    }
}
