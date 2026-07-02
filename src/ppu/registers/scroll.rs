// https://www.nesdev.org/wiki/PPU_registers#PPUSCROLL_-_X_and_Y_scroll_($2005_write)

pub struct ScrollRegister {
    pub scroll_x: u8,
    pub scroll_y: u8,
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
            latch: false,
        }
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
