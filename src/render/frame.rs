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

    pub fn set_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        let index = (y * Frame::WIDTH + x) * 3;

        if index + 2 >= self.data.len() {
            return; // Out of bounds, do nothing
        }

        self.data[index] = r;
        self.data[index + 1] = g;
        self.data[index + 2] = b;
    }
}
