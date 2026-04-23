// https://www.nesdev.org/wiki/PPU_registers#PPUCTRL_-_Miscellaneous_settings_($2000_write)

bitflags! {
    // #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ControlRegister: u8 {
        const NAMETABLE_1        = 0b0000_0001;
        const NAMETABLE_2        = 0b0000_0010;
        const VRAM_ADD_INCREMENT        = 0b0000_0100;
        const SPRITE_PATTERN_ADDR = 0b0000_1000;
        const BACKGROUND_PATTERN_ADDR     = 0b0001_0000;
        const SPRITE_SIZE         = 0b0010_0000;
        const MASTER_SLAVE_SELECT = 0b0100_0000;
        const GENERATE_NMI        = 0b1000_0000;
    }
}

impl ControlRegister {
    pub fn new() -> Self {
        ControlRegister::from_bits_truncate(0b0000_0000)
    }

    pub fn get_nametable_addr(&self) -> u16 {
        const NAMETABLE_MASK: u8 = 0b11; // because nametable takes up 2 bits
        match self.bits() & NAMETABLE_MASK {
            0 => 0x2000,
            1 => 0x2400,
            2 => 0x2800,
            3 => 0x2c00,
            _ => panic!("Impossible nametable value."),
        }
    }

    pub fn vram_add_increment(&self) -> u8 {
        if self.contains(ControlRegister::VRAM_ADD_INCREMENT) {
            1
        } else {
            32
        }
    }

    pub fn get_sprite_pattern_addr(&self) -> u16 {
        if !self.contains(ControlRegister::SPRITE_PATTERN_ADDR) {
            0
        } else {
            0x1000
        }
    }

    pub fn get_background_pattern_addr(&self) -> u16 {
        if !self.contains(ControlRegister::BACKGROUND_PATTERN_ADDR) {
            0
        } else {
            0x1000
        }
    }

    pub fn get_sprite_size(&self) -> u8 {
        if !self.contains(ControlRegister::SPRITE_SIZE) {
            8
        } else {
            16
        }
    }

    pub fn get_master_slave_select(&self) -> u8 {
        if !self.contains(ControlRegister::MASTER_SLAVE_SELECT) {
            0
        } else {
            1
        }
    }

    pub fn generate_vblank_nmi(&self) -> bool {
        self.contains(ControlRegister::GENERATE_NMI)
    }

    pub fn update(&mut self, data: u8) {
        *self = ControlRegister::from_bits_truncate(data);
    }
}
