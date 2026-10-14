pub struct Frame {
    pub data: Vec<u8>,
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Frame {
    pub const WIDTH: usize = 256;
    pub const HEIGHT: usize = 240;

    pub fn new() -> Self {
        Frame {
            data: vec![0; (Frame::WIDTH) * (Frame::HEIGHT) * 3], // 3 RGB values
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, rgb: (u8, u8, u8)) {
        if x >= Frame::WIDTH || y >= Frame::HEIGHT {
            return; // Off-screen; without the x check a pixel would wrap onto the next row.
        }
        let (r, g, b) = rgb;
        let index = (y * Frame::WIDTH + x) * 3;

        self.data[index] = r;
        self.data[index + 1] = g;
        self.data[index + 2] = b;
    }
}
