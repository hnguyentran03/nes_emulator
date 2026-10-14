// https://www.nesdev.org/wiki/PPU_registers#PPUSCROLL_-_X_and_Y_scroll_($2005_write)

// Together with `nametable_addr` this plays the role of the PPU's internal
// "t" register: the scroll position the next frame (or, mid-frame, the rest of
// this one) is drawn from. $2000 sets the nametable, $2005 the X/Y offsets and
// $2006 overwrites coarse X/Y, fine Y and the nametable.
// https://www.nesdev.org/wiki/PPU_scrolling#PPU_internal_registers
pub struct ScrollRegister {
    pub scroll_x: u8,
    pub scroll_y: u8,
    pub nametable_addr: u16,
    pub latch: bool,
}

impl Default for ScrollRegister {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollRegister {
    pub fn new() -> Self {
        ScrollRegister {
            scroll_x: 0,
            scroll_y: 0,
            nametable_addr: 0x2000,
            latch: false,
        }
    }

    // PPUCTRL bits 0-1.
    pub fn set_nametable(&mut self, index: u8) {
        self.nametable_addr = 0x2000 + (index & 0b11) as u16 * 0x400;
    }

    // A completed $2006 write loads the VRAM address into the scroll position:
    // bits 0-4 coarse X, 5-9 coarse Y, 10-11 nametable, 12-14 fine Y. Fine X
    // is not part of the address and is kept.
    pub fn set_from_vram_addr(&mut self, addr: u16) {
        let coarse_x = (addr & 0b1_1111) as u8;
        let coarse_y = ((addr >> 5) & 0b1_1111) as u8;
        let fine_y = ((addr >> 12) & 0b111) as u8;
        self.scroll_x = (coarse_x << 3) | (self.scroll_x & 0b111);
        self.scroll_y = (coarse_y << 3) | fine_y;
        self.set_nametable(((addr >> 10) & 0b11) as u8);
    }

    pub fn write(&mut self, data: u8) {
        if !self.latch {
            self.scroll_x = data;
        } else {
            self.scroll_y = data;
        }

        self.latch = !self.latch;
    }

    pub fn reset_latch(&mut self) {
        self.latch = false;
    }
}
