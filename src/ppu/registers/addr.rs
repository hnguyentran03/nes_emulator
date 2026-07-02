// https://www.nesdev.org/wiki/PPU_registers#PPUADDR_-_VRAM_address_($2006_write)

use crate::ppu::PPU_REGISTERS_END;

pub struct AddrRegister {
    pub lo_byte: u8,
    pub hi_byte: u8,
    pub hi_ptr: bool,
}

impl Default for AddrRegister {
    fn default() -> Self {
        Self::new()
    }
}

impl AddrRegister {
    pub fn new() -> Self {
        AddrRegister {
            lo_byte: 0,
            hi_byte: 0,
            hi_ptr: true,
        }
    }

    fn set(&mut self, data: u16) {
        self.lo_byte = data as u8;
        self.hi_byte = (data >> 8) as u8;
    }

    // Mirror the address if it exceeds the PPU registers end
    fn mirror_address(&mut self) {
        if self.get() > PPU_REGISTERS_END {
            self.set(self.get() & PPU_REGISTERS_END);
        }
    }

    pub fn get(&self) -> u16 {
        (self.hi_byte as u16) << 8 | (self.lo_byte as u16)
    }

    pub fn update(&mut self, data: u8) {
        if self.hi_ptr {
            self.hi_byte = data;
        } else {
            self.lo_byte = data;
        }

        self.mirror_address();

        self.hi_ptr = !self.hi_ptr;
    }

    pub fn increment(&mut self, inc: u8) {
        let lo = self.lo_byte;
        self.lo_byte = lo.wrapping_add(inc);
        if lo > self.lo_byte {
            self.hi_byte = self.hi_byte.wrapping_add(1);
        }

        self.mirror_address();
    }

    pub fn reset_latch(&mut self) {
        self.hi_ptr = true;
    }
}
