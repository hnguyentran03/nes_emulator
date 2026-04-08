// https://www.nesdev.org/wiki/PPU_registers#PPUSTATUS_-_Rendering_events_($2002_read)

bitflags! {
    pub struct StatusRegister: u8 {
        const UNUSED           = 0b0000_0001;
        const UNUSED2          = 0b0000_0010;
        const UNUSED3          = 0b0000_0100;
        const UNUSED4          = 0b0000_1000;
        const UNUSED5          = 0b0001_0000;
        const SPRITE_OVERFLOW  = 0b0010_0000;
        const SPRITE_ZERO_HIT  = 0b0100_0000;
        const VBLANK_STARTED   = 0b1000_0000;
    }
}

impl StatusRegister {
    pub fn new() -> Self {
        StatusRegister::from_bits_truncate(0b0000_0000)
    }

    pub fn set_vblank_status(&mut self, status: bool) {
        self.set(StatusRegister::VBLANK_STARTED, status);
    }

    pub fn reset_vblank_status(&mut self) {
        self.remove(StatusRegister::VBLANK_STARTED);
    }

    pub fn is_in_vblank(&self) -> bool {
        self.contains(StatusRegister::VBLANK_STARTED)
    }

    pub fn set_sprite_zero_hit(&mut self, status: bool) {
        self.set(StatusRegister::SPRITE_ZERO_HIT, status);
    }

    pub fn set_sprite_overflow(&mut self, status: bool) {
        self.set(StatusRegister::SPRITE_OVERFLOW, status);
    }
}
