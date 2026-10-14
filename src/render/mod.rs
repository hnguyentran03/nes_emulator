pub mod frame;
pub mod palette;

use crate::ppu::{PPU, ScrollState, tile_pixel};
use frame::Frame;
use std::ops::Range;

const TILES_PER_ROW: usize = 32;
const TILES_PER_COLUMN: usize = 30;
const NAMETABLE_SIZE: usize = 0x400; // 960 tile indices + 64 attribute bytes
const ATTRIBUTE_TABLE_START: usize = 0x3C0; // the 64 bytes after the 960 tile indices
const NAMETABLE_WIDTH: usize = TILES_PER_ROW * 8; // 256 px
const NAMETABLE_HEIGHT: usize = TILES_PER_COLUMN * 8; // 240 px
// XOR-ing these into a nametable address selects the horizontal / vertical neighbour.
const NAMETABLE_RIGHT: u16 = 0x400;
const NAMETABLE_BELOW: u16 = 0x800;
const SPRITE_PALETTE_START: usize = 0x11; // sprite palettes occupy $3F11-$3F1F
const OAM_ENTRY_SIZE: usize = 4;
const SPRITE_FLIP_VERTICAL: u8 = 0b1000_0000;
const SPRITE_FLIP_HORIZONTAL: u8 = 0b0100_0000;
const SPRITE_PALETTE_MASK: u8 = 0b11;

// Palette RAM is 6 bits wide; the system palette has 64 entries.
fn system_color(palette_byte: u8) -> (u8, u8, u8) {
    palette::SYSTEM_PALLETE[(palette_byte & 0x3F) as usize]
}

// One attribute byte covers a 4x4-tile block, split into four 2x2-tile
// quadrants; each 2-bit field selects one of the four background palettes.
// https://www.nesdev.org/wiki/PPU_attribute_tables
fn bg_palette(ppu: &PPU, attribute_table: &[u8], tile_column: usize, tile_row: usize) -> [u8; 4] {
    let attr_table_idx = tile_row / 4 * 8 + tile_column / 4;
    let attr_byte = attribute_table[attr_table_idx];

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

// A half-open rectangle in nametable pixel space (0..256 x 0..240).
#[derive(Clone, Copy)]
struct Viewport {
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
}

impl Viewport {
    fn is_empty(&self) -> bool {
        self.x1 >= self.x2 || self.y1 >= self.y2
    }

    // Keeps only the part that lands on screen rows `rows` after shifting by `shift_y`.
    fn clip_to_screen_rows(self, rows: &Range<usize>, shift_y: isize) -> Viewport {
        let to_nametable = |screen_y: usize| (screen_y as isize - shift_y).max(0) as usize;
        Viewport {
            y1: self.y1.max(to_nametable(rows.start)),
            y2: self.y2.min(to_nametable(rows.end)),
            ..self
        }
    }
}

// The 1 KiB nametable that the PPU address `addr` resolves to after mirroring.
fn nametable_at(ppu: &PPU, addr: u16) -> &[u8] {
    let start = ppu.mirror_vram_addr(addr) as usize;
    &ppu.vram[start..start + NAMETABLE_SIZE]
}

// Draws the part of `nametable` inside `view`, translated by (shift_x, shift_y)
// onto the frame. Each tile's palette comes from this nametable's own attribute
// table. Pixel value 0 always shows the universal background colour at $3F00.
fn render_nametable(
    ppu: &PPU,
    frame: &mut Frame,
    nametable: &[u8],
    view: Viewport,
    shift_x: isize,
    shift_y: isize,
) {
    let view = Viewport {
        x2: view.x2.min(NAMETABLE_WIDTH),
        y2: view.y2.min(NAMETABLE_HEIGHT),
        ..view
    };
    if view.is_empty() {
        return;
    }
    let bank = ppu.control.get_background_pattern_addr() as usize;
    let attribute_table = &nametable[ATTRIBUTE_TABLE_START..NAMETABLE_SIZE];

    for tile_row in view.y1 / 8..view.y2.div_ceil(8) {
        for tile_column in view.x1 / 8..view.x2.div_ceil(8) {
            let tile_n = nametable[tile_row * TILES_PER_ROW + tile_column] as usize;
            let Some(tile) = ppu.chr_tile(bank, tile_n) else {
                continue;
            };
            let palette = bg_palette(ppu, attribute_table, tile_column, tile_row);

            for y in 0..8 {
                let pixel_y = tile_row * 8 + y;
                if pixel_y < view.y1 || pixel_y >= view.y2 {
                    continue;
                }
                for x in 0..8 {
                    let pixel_x = tile_column * 8 + x;
                    if pixel_x < view.x1 || pixel_x >= view.x2 {
                        continue;
                    }
                    let value = tile_pixel(tile, x, y);
                    let color = system_color(palette[value as usize]);
                    let screen_x = pixel_x as isize + shift_x;
                    let screen_y = pixel_y as isize + shift_y;
                    if screen_x >= 0 && screen_y >= 0 {
                        frame.set_pixel(screen_x as usize, screen_y as usize, color);
                    }
                }
            }
        }
    }
}

// Draws screen rows `rows` of the background as seen through the scroll
// window `scroll`. The window starts (x, y) pixels into the base nametable and
// spills over into the nametables to the right, below and diagonally; which
// physical table each of those is depends on the cartridge's mirroring.
// https://www.nesdev.org/wiki/PPU_scrolling
fn render_scroll_window(ppu: &PPU, frame: &mut Frame, scroll: ScrollState, rows: Range<usize>) {
    let scroll_x = scroll.x as usize;
    let scroll_y = scroll.y as usize;
    let base = scroll.nametable_addr;
    let (width, height) = (NAMETABLE_WIDTH, NAMETABLE_HEIGHT);
    let (shift_left, shift_up) = (-(scroll_x as isize), -(scroll_y as isize));
    let (shift_right, shift_down) = (width as isize + shift_left, height as isize + shift_up);

    let regions = [
        // (nametable, the part of it that is visible, where its origin lands on screen)
        (
            base,
            Viewport {
                x1: scroll_x,
                y1: scroll_y,
                x2: width,
                y2: height,
            },
            shift_left,
            shift_up,
        ),
        (
            base ^ NAMETABLE_RIGHT,
            Viewport {
                x1: 0,
                y1: scroll_y,
                x2: scroll_x,
                y2: height,
            },
            shift_right,
            shift_up,
        ),
        (
            base ^ NAMETABLE_BELOW,
            Viewport {
                x1: scroll_x,
                y1: 0,
                x2: width,
                y2: scroll_y,
            },
            shift_left,
            shift_down,
        ),
        (
            base ^ NAMETABLE_RIGHT ^ NAMETABLE_BELOW,
            Viewport {
                x1: 0,
                y1: 0,
                x2: scroll_x,
                y2: scroll_y,
            },
            shift_right,
            shift_down,
        ),
    ];

    for (addr, view, shift_x, shift_y) in regions {
        let view = view.clip_to_screen_rows(&rows, shift_y);
        render_nametable(ppu, frame, nametable_at(ppu, addr), view, shift_x, shift_y);
    }
}

// Draws the background in horizontal bands: the frame-start scroll from row 0,
// then each mid-frame scroll split from its scanline down. That is how games
// keep a status bar still while the playfield underneath scrolls.
fn render_background(ppu: &PPU, frame: &mut Frame) {
    let mut scroll = ppu.frame_scroll;
    let mut row = 0;
    for split in &ppu.scroll_splits {
        let split_row = (split.scanline as usize).min(NAMETABLE_HEIGHT);
        render_scroll_window(ppu, frame, scroll, row..split_row);
        scroll = split.state;
        row = split_row;
    }
    render_scroll_window(ppu, frame, scroll, row..NAMETABLE_HEIGHT);
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

        let Some(tile) = ppu.chr_tile(bank, tile_idx) else {
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
    use crate::ppu::{ScrollSplit, ScrollState, TILE_SIZE};
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

    // Scrolling. Physical VRAM holds two nametables: A at 0..0x400, B at
    // 0x400..0x800; `Mirror` decides which logical $2000/$2400/$2800/$2C00
    // table maps where.
    fn scroll_ppu(mirror: Mirror) -> PPU {
        let mut ppu = PPU::new(chr_with_solid_tile_1(), mirror);
        ppu.palette_table[0] = 0x0F; // universal background
        ppu.palette_table[1] = 0x21; // palette 0, colour 1
        ppu
    }

    #[test]
    fn horizontal_scroll_shifts_the_base_nametable_left() {
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.vram[1] = 1; // nametable A, column 1
        ppu.frame_scroll.x = 8;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 8, 0), palette::SYSTEM_PALLETE[0x0F]);
    }

    #[test]
    fn horizontal_scroll_fills_the_right_edge_from_the_next_nametable() {
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.vram[0x400] = 1; // nametable B ($2400 under vertical mirroring), column 0
        ppu.frame_scroll.x = 8;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 247, 0), palette::SYSTEM_PALLETE[0x0F]);
        assert_eq!(pixel(&frame, 248, 0), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 255, 0), palette::SYSTEM_PALLETE[0x21]);
    }

    #[test]
    fn vertical_scroll_fills_the_bottom_edge_from_the_nametable_below() {
        let mut ppu = scroll_ppu(Mirror::Horizontal);
        ppu.vram[0x400] = 1; // nametable B ($2800 under horizontal mirroring), row 0
        ppu.frame_scroll.y = 8;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 231), palette::SYSTEM_PALLETE[0x0F]);
        assert_eq!(pixel(&frame, 0, 232), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 0, 239), palette::SYSTEM_PALLETE[0x21]);
    }

    #[test]
    fn scrolled_in_tiles_use_their_own_nametables_attribute_table() {
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.vram[0x400] = 1;
        ppu.vram[0x400 + 0x3C0] = 0b10; // nametable B: top-left quadrant -> palette 2
        ppu.vram[0x3C0] = 0b00; // nametable A says palette 0 (the wrong answer)
        ppu.palette_table[9] = 0x2A; // palette 2, colour 1
        ppu.frame_scroll.x = 8;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 248, 0), palette::SYSTEM_PALLETE[0x2A]);
    }

    #[test]
    fn control_register_selects_the_base_nametable() {
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.vram[0x400] = 1; // nametable B, column 0
        ppu.frame_scroll.nametable_addr = 0x2400;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 0), palette::SYSTEM_PALLETE[0x21]);
    }

    #[test]
    fn horizontal_scroll_under_horizontal_mirroring_wraps_onto_the_same_nametable() {
        // $2400 mirrors $2000, so the right edge must come from nametable A,
        // not from nametable B (which holds $2800).
        let mut ppu = scroll_ppu(Mirror::Horizontal);
        ppu.vram[0] = 1; // nametable A, column 0
        for row in 0..8 {
            ppu.chr_rom[2 * TILE_SIZE + 8 + row] = 0xFF; // tile 2: solid colour 2
        }
        ppu.palette_table[2] = 0x16;
        ppu.vram[0x400] = 2; // nametable B, column 0: the wrong answer
        ppu.frame_scroll.x = 8;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 248, 0), palette::SYSTEM_PALLETE[0x21]);
    }

    #[test]
    fn scrolling_on_both_axes_fills_the_corner_from_the_diagonal_nametable() {
        // Under four-screen mirroring all four tables are distinct; $2C00 is
        // physical 0xC00. Only its top-left tile is set, so pixel (248, 232)
        // can only come from that table.
        let mut ppu = scroll_ppu(Mirror::FourScreen);
        ppu.vram[0xC00] = 1;
        ppu.frame_scroll.x = 8;
        ppu.frame_scroll.y = 8;

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 248, 232), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 247, 232), palette::SYSTEM_PALLETE[0x0F]);
        assert_eq!(pixel(&frame, 248, 231), palette::SYSTEM_PALLETE[0x0F]);
    }

    #[test]
    fn scroll_y_beyond_the_nametable_height_does_not_panic() {
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.frame_scroll.x = 255;
        ppu.frame_scroll.y = 250; // hardware treats 240..=255 as "negative" scroll
        let mut frame = Frame::new();
        render(&ppu, &mut frame);
    }

    #[test]
    fn rows_above_a_scroll_split_keep_the_frame_start_scroll() {
        // Status-bar style split: rows 0..26 unscrolled, rows 26.. scrolled by 8.
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.vram[3 * TILES_PER_ROW + 1] = 1; // nametable A, tile row 3 (y 24..32), column 1
        ppu.scroll_splits.push(ScrollSplit {
            scanline: 26,
            state: ScrollState {
                x: 8,
                y: 0,
                nametable_addr: 0x2000,
            },
        });

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 25), palette::SYSTEM_PALLETE[0x0F]);
        assert_eq!(pixel(&frame, 8, 25), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 0, 26), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 8, 26), palette::SYSTEM_PALLETE[0x0F]);
    }

    #[test]
    fn a_scroll_split_can_switch_the_base_nametable() {
        let mut ppu = scroll_ppu(Mirror::Vertical);
        ppu.vram[0x400 + 4 * TILES_PER_ROW] = 1; // nametable B ($2400), tile row 4
        ppu.vram[4 * TILES_PER_ROW] = 0; // nametable A, same spot: background
        ppu.scroll_splits.push(ScrollSplit {
            scanline: 32,
            state: ScrollState {
                x: 0,
                y: 0,
                nametable_addr: 0x2400,
            },
        });

        let mut frame = Frame::new();
        render(&ppu, &mut frame);
        assert_eq!(pixel(&frame, 0, 32), palette::SYSTEM_PALLETE[0x21]);
        assert_eq!(pixel(&frame, 0, 31), palette::SYSTEM_PALLETE[0x0F]);
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
