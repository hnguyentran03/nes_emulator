pub mod frame;
pub mod palette;

use crate::ppu::PPU;
use frame::Frame;

const NUM_TILES: usize = 960; // 32 x 30 tiles in one nametable
const TILES_PER_ROW: usize = 32;
const TILE_SIZE: usize = 16;
const ATTRIBUTE_TABLE_START: usize = 0x3C0; // the 64 bytes after the 960 tile indices
const SPRITE_PALETTE_START: usize = 0x11; // sprite palettes occupy $3F11-$3F1F
const OAM_ENTRY_SIZE: usize = 4;
const SPRITE_FLIP_VERTICAL: u8 = 0b1000_0000;
const SPRITE_FLIP_HORIZONTAL: u8 = 0b0100_0000;
const SPRITE_PALETTE_MASK: u8 = 0b11;

// Palette RAM is 6 bits wide; the system palette has 64 entries.
fn system_color(palette_byte: u8) -> (u8, u8, u8) {
    palette::SYSTEM_PALLETE[(palette_byte & 0x3F) as usize]
}

// Returns the 16-byte CHR tile, or None if it falls outside the ROM.
fn tile(chr_rom: &[u8], bank: usize, tile_n: usize) -> Option<&[u8]> {
    let start = bank + tile_n * TILE_SIZE;
    chr_rom.get(start..start + TILE_SIZE)
}

// Colour index (0..=3) of pixel (x, y) within a tile. Plane 0 (`tile[y]`) is
// the low bit, plane 1 (`tile[y + 8]`) the high bit; bit 7 is the leftmost pixel.
fn tile_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let shift = 7 - x;
    let lower = (tile[y] >> shift) & 1;
    let upper = (tile[y + 8] >> shift) & 1;
    (upper << 1) | lower
}

// One attribute byte covers a 4x4-tile block, split into four 2x2-tile
// quadrants; each 2-bit field selects one of the four background palettes.
// https://www.nesdev.org/wiki/PPU_attribute_tables
fn bg_palette(ppu: &PPU, tile_column: usize, tile_row: usize) -> [u8; 4] {
    let attr_table_idx = tile_row / 4 * 8 + tile_column / 4;
    let attr_byte = ppu.vram[ATTRIBUTE_TABLE_START + attr_table_idx];

    let palette_idx = match (tile_column % 4 / 2, tile_row % 4 / 2) {
        (0, 0) => attr_byte & 0b11,
        (1, 0) => (attr_byte >> 2) & 0b11,
        (0, 1) => (attr_byte >> 4) & 0b11,
        (1, 1) => (attr_byte >> 6) & 0b11,
        _ => unreachable!(),
    };

    let start = 1 + (palette_idx as usize) * 4;
    [
        ppu.palette_table[0],
        ppu.palette_table[start],
        ppu.palette_table[start + 1],
        ppu.palette_table[start + 2],
    ]
}

// Sprite palettes live at $3F11..$3F1F. Slot 0 is transparent and never read.
fn sprite_palette(ppu: &PPU, palette_idx: u8) -> [u8; 4] {
    let start = SPRITE_PALETTE_START + (palette_idx as usize) * 4;
    [
        0,
        ppu.palette_table[start],
        ppu.palette_table[start + 1],
        ppu.palette_table[start + 2],
    ]
}

// Draws nametable 0 into `frame` using the background pattern table and the
// attribute table to pick each tile's palette. Pixel value 0 always shows
// the universal background colour at $3F00.
fn render_background(ppu: &PPU, frame: &mut Frame) {
    let bank = ppu.control.get_background_pattern_addr() as usize;
    for i in 0..NUM_TILES {
        let tile_column = i % TILES_PER_ROW;
        let tile_row = i / TILES_PER_ROW;
        let Some(tile) = tile(&ppu.chr_rom, bank, ppu.vram[i] as usize) else {
            continue;
        };
        let palette = bg_palette(ppu, tile_column, tile_row);

        for y in 0..8 {
            for x in 0..8 {
                let value = tile_pixel(tile, x, y);
                let color = system_color(palette[value as usize]);
                frame.set_pixel(tile_column * 8 + x, tile_row * 8 + y, color);
            }
        }
    }
}

// Draws every OAM entry (8x8 sprites only). Iterating in reverse makes
// lower-numbered sprites land on top, matching hardware priority.
// https://www.nesdev.org/wiki/PPU_OAM
fn render_sprites(ppu: &PPU, frame: &mut Frame) {
    let bank = ppu.control.get_sprite_pattern_addr() as usize;
    for i in (0..ppu.oam_data.len()).step_by(OAM_ENTRY_SIZE).rev() {
        let sprite_y = ppu.oam_data[i] as usize;
        let tile_idx = ppu.oam_data[i + 1] as usize;
        let attributes = ppu.oam_data[i + 2];
        let sprite_x = ppu.oam_data[i + 3] as usize;

        let flip_vertical = attributes & SPRITE_FLIP_VERTICAL != 0;
        let flip_horizontal = attributes & SPRITE_FLIP_HORIZONTAL != 0;
        let palette = sprite_palette(ppu, attributes & SPRITE_PALETTE_MASK);

        let Some(tile) = tile(&ppu.chr_rom, bank, tile_idx) else {
            continue;
        };

        for y in 0..8 {
            for x in 0..8 {
                let value = tile_pixel(tile, x, y);
                if value == 0 {
                    continue; // transparent
                }
                let color = system_color(palette[value as usize]);
                let px = if flip_horizontal { 7 - x } else { x };
                let py = if flip_vertical { 7 - y } else { y };
                frame.set_pixel(sprite_x + px, sprite_y + py, color);
            }
        }
    }
}

pub fn render(ppu: &PPU, frame: &mut Frame) {
    render_background(ppu, frame);
    render_sprites(ppu, frame);
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rom::Mirror;

    #[test]
    fn render_draws_nametable_tile_at_its_grid_position() {
        // Tile 1, row 0: plane0 = 1000_0000, plane1 = 1000_0000 -> leftmost pixel = idx 3.
        let mut chr = vec![0u8; 0x2000];
        chr[TILE_SIZE] = 0b1000_0000;
        chr[TILE_SIZE + 8] = 0b1000_0000;
        let mut ppu = PPU::new(chr, Mirror::Horizontal);
        ppu.vram[TILES_PER_ROW + 1] = 1; // tile row 1, column 1
        ppu.palette_table[0] = 0x01; // universal background colour
        ppu.palette_table[3] = 0x30; // background palette 0, colour 3

        let mut frame = Frame::new();
        render(&ppu, &mut frame);

        let idx = (8 * Frame::WIDTH + 8) * 3;
        assert_eq!(
            (frame.data[idx], frame.data[idx + 1], frame.data[idx + 2]),
            palette::SYSTEM_PALLETE[0x30]
        );
        assert_eq!(
            (frame.data[0], frame.data[1], frame.data[2]),
            palette::SYSTEM_PALLETE[0x01]
        );
    }

    #[test]
    fn render_with_empty_chr_does_not_panic() {
        let mut ppu = PPU::new(vec![], Mirror::Vertical);
        ppu.oam_data[1] = 5; // a sprite referencing a tile that doesn't exist
        let mut frame = Frame::new();
        render(&ppu, &mut frame);
    }

    fn pixel(frame: &Frame, x: usize, y: usize) -> (u8, u8, u8) {
        let idx = (y * Frame::WIDTH + x) * 3;
        (frame.data[idx], frame.data[idx + 1], frame.data[idx + 2])
    }

    // Tile 1: every pixel has colour index 1 (plane 0 set, plane 1 clear).
    fn chr_with_solid_tile_1() -> Vec<u8> {
        let mut chr = vec![0u8; 0x2000];
        for row in 0..8 {
            chr[TILE_SIZE + row] = 0xFF;
        }
        chr
    }

    #[test]
    fn background_pixel_zero_uses_universal_background_colour() {
        let mut ppu = PPU::new(vec![0u8; 0x2000], Mirror::Horizontal);
        ppu.palette_table[0] = 0x0F;
        ppu.palette_table[4] = 0x16; // palette 1 slot 0 must not be used
        ppu.vram[0x3C0] = 0b01; // top-left quadrant -> palette 1

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0x0F]);
    }

    #[test]
    fn attribute_table_selects_background_palette_per_quadrant() {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), Mirror::Horizontal);
        // Tile at column 2, row 0 is the top-right quadrant (bits 3..2) of attribute byte 0.
        ppu.vram[2] = 1;
        ppu.vram[0x3C0] = 0b0000_1000; // top-right quadrant -> palette 2
        ppu.palette_table[9] = 0x21; // palette 2, colour 1
        ppu.palette_table[5] = 0x11; // palette 1, colour 1 (wrong answer)

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 16, 0), palette::SYSTEM_PALLETE[0x21]);
    }

    #[test]
    fn palette_bytes_above_63_do_not_panic() {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), Mirror::Horizontal);
        ppu.vram[0] = 1;
        ppu.palette_table[0] = 0xFF;
        ppu.palette_table[1] = 0x7F; // 6-bit palette RAM: should act like 0x3F
        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0x3F]);
    }

    #[test]
    fn sprite_is_drawn_at_oam_position_with_sprite_palette() {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), Mirror::Horizontal);
        ppu.palette_table[0x11 + 4] = 0x2A; // sprite palette 1, colour 1
        ppu.oam_data[0] = 20; // y
        ppu.oam_data[1] = 1; // tile
        ppu.oam_data[2] = 0b01; // palette 1
        ppu.oam_data[3] = 40; // x

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 40, 20), palette::SYSTEM_PALLETE[0x2A]);
        assert_eq!(pixel(&frame, 47, 27), palette::SYSTEM_PALLETE[0x2A]);
        assert_eq!(pixel(&frame, 39, 20), palette::SYSTEM_PALLETE[0]);
    }

    #[test]
    fn sprite_transparent_pixels_leave_background_intact() {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), Mirror::Horizontal);
        ppu.palette_table[0] = 0x0F;
        // Tile 2: only the leftmost column is colour 1.
        for row in 0..8 {
            ppu.chr_rom[2 * TILE_SIZE + row] = 0b1000_0000;
        }
        ppu.palette_table[0x11] = 0x2A;
        ppu.oam_data[0] = 0;
        ppu.oam_data[1] = 2;
        ppu.oam_data[2] = 0;
        ppu.oam_data[3] = 0;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0x2A]);
        assert_eq!(pixel(&frame, 1, 0), palette::SYSTEM_PALLETE[0x0F]);
    }

    #[test]
    fn sprite_horizontal_flip_mirrors_columns() {
        let mut ppu = PPU::new(vec![0u8; 0x2000], Mirror::Horizontal);
        for row in 0..8 {
            ppu.chr_rom[2 * TILE_SIZE + row] = 0b1000_0000;
        }
        ppu.palette_table[0x11] = 0x2A;
        ppu.oam_data[1] = 2;
        ppu.oam_data[2] = 0b0100_0000; // flip horizontally

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 7, 0), palette::SYSTEM_PALLETE[0x2A]);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0]);
    }

    #[test]
    fn sprite_vertical_flip_mirrors_rows() {
        let mut ppu = PPU::new(vec![0u8; 0x2000], Mirror::Horizontal);
        ppu.chr_rom[2 * TILE_SIZE] = 0xFF; // only row 0 is colour 1
        ppu.palette_table[0x11] = 0x2A;
        ppu.oam_data[1] = 2;
        ppu.oam_data[2] = 0b1000_0000; // flip vertically

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 7), palette::SYSTEM_PALLETE[0x2A]);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0]);
    }

    #[test]
    fn lower_numbered_sprite_is_drawn_on_top() {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), Mirror::Horizontal);
        ppu.palette_table[0x11] = 0x2A; // sprite palette 0
        ppu.palette_table[0x15] = 0x16; // sprite palette 1
        // Sprite 0 and sprite 1 both at (0, 0), different palettes.
        ppu.oam_data[1] = 1;
        ppu.oam_data[2] = 0;
        ppu.oam_data[5] = 1;
        ppu.oam_data[6] = 1;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0x2A]);
    }

    // Drives the real ROM for a few frames so OAM DMA and sprite rendering see
    // genuine game traffic. Skipped if the ROM isn't checked out.
    #[test]
    fn alter_ego_renders_frames_without_panicking() {
        use crate::bus::Bus;
        use crate::cpu::CPU;
        use crate::joypad::Joypad;
        use crate::rom::Rom;
        use std::cell::Cell;

        let Ok(bytes) = std::fs::read("roms/Alter_Ego.nes") else {
            return;
        };
        let rom = Rom::new(&bytes).unwrap();
        let frames = Cell::new(0u32);
        let mut frame = Frame::new();
        let non_background_pixels = Cell::new(0usize);
        let bus = Bus::new(rom, |ppu: &PPU, _joypad: &mut Joypad| {
            render(ppu, &mut frame);
            let bg = system_color(ppu.palette_table[0]);
            let visible = frame
                .data
                .chunks(3)
                .filter(|p| (p[0], p[1], p[2]) != bg)
                .count();
            non_background_pixels.set(non_background_pixels.get() + visible);
            frames.set(frames.get() + 1);
            if frames.get() >= 120 {
                // cpu.run() only returns on BRK, so unwind out with a marker.
                std::panic::panic_any(StopEmulation);
            }
        });
        struct StopEmulation;
        let mut cpu = CPU::new(bus);
        cpu.reset();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cpu.run()));
        match result {
            Err(payload) if payload.is::<StopEmulation>() => {}
            Err(payload) => std::panic::resume_unwind(payload),
            Ok(()) => panic!("ROM halted before 120 frames"),
        }
        assert!(
            non_background_pixels.get() > 0,
            "rendered frames were blank"
        );
    }

    #[test]
    fn sprite_at_right_edge_does_not_wrap_onto_next_row() {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), Mirror::Horizontal);
        ppu.palette_table[0x11] = 0x2A;
        ppu.oam_data[0] = 10;
        ppu.oam_data[1] = 1;
        ppu.oam_data[3] = 252; // columns 252..259, the last four fall off-screen

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 255, 10), palette::SYSTEM_PALLETE[0x2A]);
        for x in 0..4 {
            assert_eq!(pixel(&frame, x, 11), palette::SYSTEM_PALLETE[0]);
        }
    }
}
