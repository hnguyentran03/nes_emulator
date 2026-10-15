use crate::render::frame::Frame;
use crate::render::palette;
use crate::rom::Rom;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;

const SCALE: f32 = 3.0;

const BANK_SIZE: usize = 0x1000;
const TILE_SIZE: usize = 16;
const TILES_PER_BANK: usize = BANK_SIZE / TILE_SIZE; // 256

pub fn run(rom_data: &[u8]) {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window(
            "NES Emulator - CHR Tile Viewer",
            (Frame::WIDTH as f32 * SCALE) as u32,
            (Frame::HEIGHT as f32 * SCALE) as u32,
        )
        .position_centered()
        .build()
        .unwrap();
    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    let mut event_pump = sdl_context.event_pump().unwrap();
    canvas.set_scale(SCALE, SCALE).unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_static(
            PixelFormatEnum::RGB24,
            Frame::WIDTH as u32,
            Frame::HEIGHT as u32,
        )
        .unwrap();

    // Decode the ROM's first CHR pattern table into the frame.
    let rom = Rom::new(rom_data).unwrap();

    let mut frame = Frame::new();
    render_chr_bank(&rom.chr_rom, 0, &mut frame);

    texture.update(None, &frame.data, Frame::WIDTH * 3).unwrap();
    canvas.copy(&texture, None, None).unwrap();
    canvas.present();

    // Static view: just hold the window open until quit / Escape.
    loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return,
                _ => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

pub fn show_tile(chr_rom: &[u8], bank: usize, tile_n: usize) -> Frame {
    let mut frame = Frame::new();
    draw_tile(chr_rom, bank, tile_n, 0, 0, &mut frame);
    frame
}

fn render_chr_bank(chr_rom: &[u8], bank: usize, frame: &mut Frame) {
    let tiles_per_row = Frame::WIDTH / 8;
    for tile_n in 0..TILES_PER_BANK {
        let origin_x = (tile_n % tiles_per_row) * 8;
        let origin_y = (tile_n / tiles_per_row) * 8;
        draw_tile(chr_rom, bank, tile_n, origin_x, origin_y, frame);
    }
}

// Draws a single 8x8 tile into `frame` at pixel (origin_x, origin_y)
fn draw_tile(
    chr_rom: &[u8],
    bank: usize,
    tile_n: usize,
    origin_x: usize,
    origin_y: usize,
    frame: &mut Frame,
) {
    let start = bank * BANK_SIZE + tile_n * TILE_SIZE;
    if start + TILE_SIZE > chr_rom.len() {
        return;
    }
    let tile = &chr_rom[start..start + TILE_SIZE];

    for row in 0..8 {
        let mut lower = tile[row];
        let mut upper = tile[row + 8];
        // Bit 7 is the leftmost pixel, so fill columns right-to-left.
        for col in (0..8).rev() {
            let value = ((upper & 1) << 1) | (lower & 1);
            upper >>= 1;
            lower >>= 1;
            let (r, g, b) = match value {
                0 => palette::SYSTEM_PALLETE[0x01],
                1 => palette::SYSTEM_PALLETE[0x23],
                2 => palette::SYSTEM_PALLETE[0x27],
                3 => palette::SYSTEM_PALLETE[0x30],
                _ => unreachable!(),
            };
            frame.set_pixel(origin_x + col, origin_y + row, (r, g, b));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn show_tile_decodes_bitplanes_to_indices() {
        // Row 0: plane0 = 1000_0000, plane1 = 0000_0000 -> leftmost pixel = idx 1,
        // the pixel to its right = idx 0 (background).
        let mut chr = vec![0u8; BANK_SIZE];
        chr[0] = 0b1000_0000;

        let frame = show_tile(&chr, 0, 0);

        assert_eq!(
            (frame.data[0], frame.data[1], frame.data[2]),
            palette::SYSTEM_PALLETE[0x23]
        );
        assert_eq!(
            (frame.data[3], frame.data[4], frame.data[5]),
            palette::SYSTEM_PALLETE[0x01]
        );
    }

    #[test]
    fn show_tile_out_of_range_is_blank() {
        let frame = show_tile(&[], 0, 0); // empty CHR must not panic
        assert!(frame.data.iter().all(|&b| b == 0));
    }
}
