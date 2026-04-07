bitflags! {
    // #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ControlRegister: u8 {
        const NAME_TABLE_1        = 0b0000_0001;
        const NAME_TABLE_2        = 0b0000_0010;
        const VRAM_ADD_INC        = 0b0000_0100;
        const SPRITE_PATTERN_ADDR = 0b0000_1000;
        const BG_PATTERN_ADDR     = 0b0001_0000;
        const SPRITE_SIZE         = 0b0010_0000;
        const MASTER_SLAVE_SELECT = 0b0100_0000;
        const GENERATE_NMI        = 0b1000_0000;
    }
}

impl ControlRegister {
    pub fn new() -> Self {
        ControlRegister::from_bits_truncate(0b0000_0000)
    }

    pub fn update(&mut self, value: u8) {
        self.bits = value;
    }

    pub fn vram_add_inc(&self) -> u8 {
        if self.contains(ControlRegister::VRAM_ADD_INC) {
            1
        } else {
            32
        }
    }
}
