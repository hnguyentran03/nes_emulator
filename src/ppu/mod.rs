use crate::ppu::registers::{
    addr::AddrRegister, control::ControlRegister, mask::MaskRegister, scroll::ScrollRegister,
    status::StatusRegister,
};
use crate::rom::Mirror;

pub mod registers;

pub const PPU_REGISTERS: u16 = 0x2000;
pub const PPU_REGISTERS_END: u16 = 0x3FFF;

const CHR_ROM_START: u16 = 0x0000;
const CHR_ROM_END: u16 = 0x1FFF;
const VRAM_START: u16 = 0x2000;
const VRAM_END: u16 = 0x2FFF;

const UNUSED_START: u16 = 0x3000;
const UNUSED_END: u16 = 0x3EFF;

const PALETTE_TABLE_START: u16 = 0x3F00;
const PALETTE_TABLE_END: u16 = 0x3FFF;
const PALETTE_TABLE_SIZE: u16 = 0x20;

const MIRROR_MASK: u16 = 0b1011_1111_1111_1111; // Mirrors from 0x3000-0x3EFF to 0x2000-0x2EFF
const SCREEN_STATE_SIZE: u16 = 0x400; // 1 KiB

pub const TILE_SIZE: usize = 16; // bytes per 8x8 CHR tile: two 8-byte bit planes
const TILES_PER_ROW: usize = 32;
const NAMETABLE_WIDTH: usize = 256;
const NAMETABLE_HEIGHT: usize = 240;
const SPRITE_FLIP_VERTICAL: u8 = 0b1000_0000;
const SPRITE_FLIP_HORIZONTAL: u8 = 0b0100_0000;

const SCANLINES_PER_FRAME: u16 = 262;
const VISIBLE_SCANLINES: u16 = 240;
const CYCLES_PER_SCANLINE: usize = 341;
const VISIBLE_DOTS: usize = 256; // the rest of the scanline is horizontal blank
const CYCLE_AT_NMI_INTERUPT: u16 = 241;

// The scroll position plus base nametable that the background is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    pub x: u8,
    pub y: u8,
    pub nametable_addr: u16,
}

impl Default for ScrollState {
    fn default() -> Self {
        ScrollState {
            x: 0,
            y: 0,
            nametable_addr: 0x2000,
        }
    }
}

// A scroll change made while the frame was being drawn; it applies from
// `scanline` downwards. Games use this (after a sprite-zero hit) to keep a
// status bar fixed while the playfield scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollSplit {
    pub scanline: u16,
    pub state: ScrollState,
}

pub struct PPU {
    pub chr_rom: Vec<u8>,
    pub palette_table: [u8; 0x20],
    pub vram: [u8; 0x4000],
    pub oam_data: [u8; 0x100],
    pub oam_addr: u8,
    pub mirror: Mirror,
    internal_data_buf: u8,

    pub addr: AddrRegister,
    pub control: ControlRegister,
    pub mask: MaskRegister,
    pub scroll: ScrollRegister,
    pub status: StatusRegister,
    // Scroll in effect at scanline 0, and the mid-frame changes since then
    // (ascending scanline). Writes during vblank only land in `frame_scroll`
    // once the next frame starts.
    pub frame_scroll: ScrollState,
    pub scroll_splits: Vec<ScrollSplit>,

    cycles: usize,
    scanlines: u16,
    pub nmi_interrupted: Option<u8>,
}

// Colour index (0..=3) of pixel (x, y) within a 16-byte CHR tile. Plane 0
// (`tile[y]`) is the low bit, plane 1 (`tile[y + 8]`) the high bit; bit 7 is
// the leftmost pixel.
pub fn tile_pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    let shift = 7 - x;
    let lower = (tile[y] >> shift) & 1;
    let upper = (tile[y + 8] >> shift) & 1;
    (upper << 1) | lower
}

impl PPU {
    pub fn new(chr_rom: Vec<u8>, mirror: Mirror) -> Self {
        PPU {
            chr_rom,
            palette_table: [0; 0x20],
            vram: [0; 0x4000],
            oam_data: [0; 0x100],
            oam_addr: 0,
            mirror,
            internal_data_buf: 0,

            addr: AddrRegister::new(),
            control: ControlRegister::new(),
            mask: MaskRegister::new(),
            scroll: ScrollRegister::new(),
            status: StatusRegister::new(),
            frame_scroll: ScrollState::default(),
            scroll_splits: Vec::new(),

            cycles: 0,
            scanlines: 0,
            nmi_interrupted: None,
        }
    }

    pub fn tick(&mut self, cycles: usize) -> bool {
        self.cycles += cycles;
        if self.cycles >= CYCLES_PER_SCANLINE {
            if self.is_sprite_zero_hit() {
                self.status.set_sprite_zero_hit(true);
            }

            self.cycles -= CYCLES_PER_SCANLINE;
            self.scanlines += 1;

            if self.scanlines == CYCLE_AT_NMI_INTERUPT {
                self.status.set_vblank_status(true);
                self.status.set_sprite_zero_hit(false);
                if self.control.generate_vblank_nmi() {
                    self.status.set_vblank_status(true);
                    self.nmi_interrupted = Some(1);
                }
            }

            if self.scanlines >= SCANLINES_PER_FRAME {
                self.scanlines = 0;
                self.frame_scroll = self.scroll_state();
                self.scroll_splits.clear();
                self.nmi_interrupted = None;
                self.status.set_sprite_zero_hit(false);
                self.status.reset_vblank_status();
                return true;
            }
        }

        false
    }

    // https://www.nesdev.org/wiki/PPU_OAM#Sprite_zero_hits: true if, on the
    // scanline just rendered, an opaque pixel of sprite 0 overlaps an opaque
    // background pixel. Sprites are drawn one scanline below their OAM Y and
    // only 8x8 sprites are considered. Checked once per scanline, so the hit
    // lands at the end of the scanline rather than at the exact dot.
    fn is_sprite_zero_hit(&self) -> bool {
        if !(self.mask.show_sprites() && self.mask.show_background()) {
            return false;
        }
        if self.scanlines >= VISIBLE_SCANLINES {
            return false;
        }
        let sprite_y = self.oam_data[0] as u16;
        let tile_n = self.oam_data[1] as usize;
        let attributes = self.oam_data[2];
        let sprite_x = self.oam_data[3] as usize;
        if sprite_x == 255 {
            return false; // hardware never reports a hit at x = 255
        }
        let Some(row) = self.scanlines.checked_sub(sprite_y + 1) else {
            return false;
        };
        let row = row as usize;
        if row >= 8 {
            return false;
        }
        let bank = self.control.get_sprite_pattern_addr() as usize;
        let Some(tile) = self.chr_tile(bank, tile_n) else {
            return false;
        };
        let tile_row = if attributes & SPRITE_FLIP_VERTICAL != 0 {
            7 - row
        } else {
            row
        };
        let scroll = self.scroll_state();
        let screen_y = self.scanlines as usize;
        (0..8)
            .filter(|&col| sprite_x + col < NAMETABLE_WIDTH)
            .any(|col| {
                let tile_col = if attributes & SPRITE_FLIP_HORIZONTAL != 0 {
                    7 - col
                } else {
                    col
                };
                tile_pixel(tile, tile_col, tile_row) != 0
                    && self.background_pixel(scroll, sprite_x + col, screen_y) != 0
            })
    }

    // The 16-byte CHR tile `tile_n` of pattern table `bank`, or None if it
    // falls outside the ROM.
    pub fn chr_tile(&self, bank: usize, tile_n: usize) -> Option<&[u8]> {
        let start = bank + tile_n * TILE_SIZE;
        self.chr_rom.get(start..start + TILE_SIZE)
    }

    pub fn scroll_state(&self) -> ScrollState {
        ScrollState {
            x: self.scroll.scroll_x,
            y: self.scroll.scroll_y,
            nametable_addr: self.scroll.nametable_addr,
        }
    }

    // Colour index of the background pixel at screen (x, y) under `scroll`,
    // following the window into the neighbouring nametables.
    fn background_pixel(&self, scroll: ScrollState, screen_x: usize, screen_y: usize) -> u8 {
        let mut nametable = scroll.nametable_addr;
        let mut x = screen_x + scroll.x as usize;
        if x >= NAMETABLE_WIDTH {
            x -= NAMETABLE_WIDTH;
            nametable ^= 0x400;
        }
        let mut y = screen_y + scroll.y as usize;
        if y >= NAMETABLE_HEIGHT {
            y -= NAMETABLE_HEIGHT;
            nametable ^= 0x800;
        }
        let base = self.mirror_vram_addr(nametable) as usize;
        let tile_n = self.vram[base + (y / 8) * TILES_PER_ROW + x / 8] as usize;
        let bank = self.control.get_background_pattern_addr() as usize;
        self.chr_tile(bank, tile_n)
            .map_or(0, |tile| tile_pixel(tile, x % 8, y % 8))
    }

    // Called after any register write that moves the scroll window. During
    // visible scanlines it records a split for the renderer; several writes on
    // the same scanline (e.g. an X/Y pair) collapse into one. A write during
    // horizontal blank cannot change the row already drawn, so it counts from
    // the next scanline.
    fn record_scroll_change(&mut self) {
        let mut scanline = self.scanlines;
        if self.cycles >= VISIBLE_DOTS {
            scanline += 1;
        }
        if scanline >= VISIBLE_SCANLINES {
            return;
        }
        let split = ScrollSplit {
            scanline,
            state: self.scroll_state(),
        };
        match self.scroll_splits.last_mut() {
            Some(last) if last.scanline == split.scanline => *last = split,
            _ => self.scroll_splits.push(split),
        }
    }

    pub fn poll_nmi_interrupted(&mut self) -> Option<u8> {
        self.nmi_interrupted.take()
    }

    pub fn read_status(&mut self) -> u8 {
        let data = self.status.bits();
        self.status.reset_vblank_status();
        self.addr.reset_latch();
        self.scroll.reset_latch();
        data
    }

    pub fn read_oam_data(&self) -> u8 {
        self.oam_data[self.oam_addr as usize]
    }

    fn increment_vram_addr(&mut self) {
        self.addr.increment(self.control.vram_add_increment());
    }

    pub fn read_data(&mut self) -> u8 {
        let addr = self.addr.get();
        self.increment_vram_addr();

        match addr {
            CHR_ROM_START..=CHR_ROM_END => {
                let result = self.internal_data_buf;
                self.internal_data_buf = self.chr_rom[addr as usize];
                result
            }

            VRAM_START..=VRAM_END => {
                let result = self.internal_data_buf;
                self.internal_data_buf = self.vram[self.mirror_vram_addr(addr) as usize];
                result
            }

            0x3000..=0x3EFF => {
                panic!("Unexpected address: {}", addr);
            }

            PALETTE_TABLE_START..=PALETTE_TABLE_END => {
                self.palette_table[Self::palette_index(addr)]
            }

            _ => {
                panic!("Unexpected address to mirrored space: {}", addr);
            }
        }
    }

    // $3F00-$3F1F is mirrored through $3FFF, and $3F10/$3F14/$3F18/$3F1C
    // are mirrors of $3F00/$3F04/$3F08/$3F0C.
    // https://www.nesdev.org/wiki/PPU_palettes#Memory_Map
    fn palette_index(addr: u16) -> usize {
        let idx = (addr - PALETTE_TABLE_START) % PALETTE_TABLE_SIZE;
        match idx {
            0x10 | 0x14 | 0x18 | 0x1C => (idx - 0x10) as usize,
            _ => idx as usize,
        }
    }

    pub fn write_to_addr(&mut self, data: u8) {
        let completes_address = !self.addr.hi_ptr;
        self.addr.update(data);
        if completes_address {
            self.scroll.set_from_vram_addr(self.addr.get());
            self.record_scroll_change();
        }
    }

    pub fn write_to_control(&mut self, data: u8) {
        let before_nmi_status = self.control.generate_vblank_nmi();
        self.control.update(data);
        self.scroll.set_nametable(data);
        self.record_scroll_change();
        if !before_nmi_status && self.control.generate_vblank_nmi() && self.status.is_in_vblank() {
            self.nmi_interrupted = Some(1)
        }
    }

    pub fn write_to_mask(&mut self, data: u8) {
        self.mask.update(data);
    }

    // https://www.nesdev.org/wiki/PPU_registers#OAMADDR_-_Sprite_RAM_address_($2003_write)
    pub fn write_to_oam_addr(&mut self, data: u8) {
        self.oam_addr = data;
    }

    // https://www.nesdev.org/wiki/PPU_registers#OAMDATA_-_Sprite_RAM_data_($2004_read/write)
    pub fn write_to_oam_data(&mut self, data: u8) {
        self.oam_data[self.oam_addr as usize] = data;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    pub fn write_to_scroll(&mut self, data: u8) {
        self.scroll.write(data);
        self.record_scroll_change();
    }

    pub fn write_to_data(&mut self, data: u8) {
        let addr = self.addr.get();
        match addr {
            CHR_ROM_START..=CHR_ROM_END => { /* Ignore attempts to write to CHR ROM */ } // TODO double check

            VRAM_START..=VRAM_END => {
                self.vram[self.mirror_vram_addr(addr) as usize] = data;
            }

            UNUSED_START..=UNUSED_END => { /* Ignore attempts to write to unused addrs */ }

            PALETTE_TABLE_START..=PALETTE_TABLE_END => {
                self.palette_table[Self::palette_index(addr)] = data;
            }

            _ => panic!("Unexpected acces to mirrored space {}", addr),
        }

        self.increment_vram_addr();
    }

    // https://www.nesdev.org/wiki/PPU_registers#OAMDMA_-_Sprite_DMA_($4014_write)
    pub fn write_oam_dma(&mut self, data: &[u8; 0x100]) {
        for d in data.iter() {
            self.oam_data[self.oam_addr as usize] = *d;
            self.oam_addr = self.oam_addr.wrapping_add(1);
        }
    }

    pub fn mirror_vram_addr(&self, addr: u16) -> u16 {
        let mirror_vram = addr & MIRROR_MASK;
        let vram_index = mirror_vram - VRAM_START;
        let name_table = vram_index / SCREEN_STATE_SIZE;
        match (&self.mirror, name_table) {
            (Mirror::Vertical, 2) | (Mirror::Vertical, 3) => vram_index - 2 * SCREEN_STATE_SIZE,
            (Mirror::Horizontal, 1) | (Mirror::Horizontal, 2) => vram_index - SCREEN_STATE_SIZE,
            (Mirror::Horizontal, 3) => vram_index - 2 * SCREEN_STATE_SIZE,
            _ => vram_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ppu::registers::control::ControlRegister;
    use crate::ppu::registers::mask::{Color, MaskRegister};
    use crate::ppu::registers::status::StatusRegister;
    use crate::rom::CHR_ROM_PAGE_SIZE;

    fn set_vram_addr(ppu: &mut PPU, addr: u16) {
        ppu.addr.reset_latch();
        ppu.write_to_addr((addr >> 8) as u8);
        ppu.write_to_addr(addr as u8);
    }

    // Addr Register
    #[test]
    fn addr_register_sets_address_from_two_writes() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_addr(0x20);
        ppu.write_to_addr(0x05);
        assert_eq!(ppu.addr.get(), 0x2005);
    }

    #[test]
    fn addr_register_wraps_above_ppu_space() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_addr(0x40);
        ppu.write_to_addr(0x00);
        assert_eq!(ppu.addr.get(), 0x0000);
    }

    #[test]
    fn addr_register_increment_respects_control_vram_step() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        set_vram_addr(&mut ppu, 0x2000);
        ppu.write_to_control(0);
        ppu.write_to_data(0x01);
        assert_eq!(ppu.addr.get(), 0x2001);

        set_vram_addr(&mut ppu, 0x2000);
        ppu.write_to_control(ControlRegister::VRAM_ADD_INCREMENT.bits());
        ppu.write_to_data(0x02);
        assert_eq!(ppu.addr.get(), 0x2020);
    }

    #[test]
    fn read_status_resets_addr_latch() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_addr(0x20);
        assert!(!ppu.addr.hi_ptr);
        let _ = ppu.read_status();
        assert!(ppu.addr.hi_ptr);
    }

    // Control Register
    #[test]
    fn control_nametable_base_addresses() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(0b00);
        assert_eq!(ppu.control.get_nametable_addr(), 0x2000);
        ppu.write_to_control(0b01);
        assert_eq!(ppu.control.get_nametable_addr(), 0x2400);
        ppu.write_to_control(0b10);
        assert_eq!(ppu.control.get_nametable_addr(), 0x2800);
        ppu.write_to_control(0b11);
        assert_eq!(ppu.control.get_nametable_addr(), 0x2C00);
    }

    #[test]
    fn control_pattern_and_sprite_settings() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(0);
        assert_eq!(ppu.control.get_sprite_pattern_addr(), 0);
        assert_eq!(ppu.control.get_background_pattern_addr(), 0);
        assert_eq!(ppu.control.get_sprite_size(), 8);
        assert_eq!(ppu.control.get_master_slave_select(), 0);

        ppu.write_to_control(
            ControlRegister::SPRITE_PATTERN_ADDR.bits()
                | ControlRegister::BACKGROUND_PATTERN_ADDR.bits()
                | ControlRegister::SPRITE_SIZE.bits()
                | ControlRegister::MASTER_SLAVE_SELECT.bits(),
        );
        assert_eq!(ppu.control.get_sprite_pattern_addr(), 0x1000);
        assert_eq!(ppu.control.get_background_pattern_addr(), 0x1000);
        assert_eq!(ppu.control.get_sprite_size(), 16);
        assert_eq!(ppu.control.get_master_slave_select(), 1);
    }

    // Mask Register
    #[test]
    fn mask_rendering_and_emphasis_flags() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_mask(0);
        assert!(!ppu.mask.is_grayscale());
        assert!(!ppu.mask.show_background());
        assert!(ppu.mask.emphasise().is_empty());

        ppu.write_to_mask(
            MaskRegister::GREYSCALE.bits()
                | MaskRegister::SHOW_BACKGROUND.bits()
                | MaskRegister::SHOW_SPRITES.bits()
                | MaskRegister::LEFTMOST_8PXL_BACKGROUND.bits()
                | MaskRegister::LEFTMOST_8PXL_SPRITE.bits()
                | MaskRegister::EMPHASISE_RED.bits()
                | MaskRegister::EMPHASISE_GREEN.bits()
                | MaskRegister::EMPHASISE_BLUE.bits(),
        );
        assert!(ppu.mask.is_grayscale());
        assert!(ppu.mask.show_background());
        assert!(ppu.mask.show_sprites());
        assert!(ppu.mask.show_leftmost_8pxl_background());
        assert!(ppu.mask.show_leftmost_8pxl_sprite());
        let colors = ppu.mask.emphasise();
        assert!(colors.iter().any(|c| matches!(c, Color::Red)));
        assert!(colors.iter().any(|c| matches!(c, Color::Green)));
        assert!(colors.iter().any(|c| matches!(c, Color::Blue)));
    }

    // Scroll Register
    #[test]
    fn scroll_alternates_x_and_y() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_scroll(0x11);
        ppu.write_to_scroll(0x22);
        assert_eq!(ppu.scroll.scroll_x, 0x11);
        assert_eq!(ppu.scroll.scroll_y, 0x22);
    }

    #[test]
    fn read_status_resets_scroll_latch() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_scroll(0x11);
        assert!(ppu.scroll.latch);
        let _ = ppu.read_status();
        assert!(!ppu.scroll.latch);
    }

    // Status Register
    #[test]
    fn read_status_returns_flags_and_clears_vblank() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.status.set_vblank_status(true);
        ppu.status.set_sprite_zero_hit(true);
        let data = ppu.read_status();
        assert!(data & StatusRegister::VBLANK_STARTED.bits() != 0);
        assert!(data & StatusRegister::SPRITE_ZERO_HIT.bits() != 0);
        assert!(!ppu.status.is_in_vblank());
    }

    // Sprite zero hit (https://www.nesdev.org/wiki/PPU_OAM#Sprite_zero_hits)
    // CHR tile 1 is solid colour 1; tile 2 has only its row 6 opaque.
    fn hit_test_chr() -> Vec<u8> {
        let mut chr = vec![0u8; CHR_ROM_PAGE_SIZE];
        for row in 0..8 {
            chr[TILE_SIZE + row] = 0xFF;
        }
        chr[2 * TILE_SIZE + 6] = 0xFF;
        chr
    }

    // Sprite 0 uses `tile` at (x, y) over a background made entirely of solid tile 1.
    fn ppu_with_sprite_zero(x: u8, y: u8, tile: u8, mask: u8) -> PPU {
        let mut ppu = PPU::new(hit_test_chr(), Mirror::Horizontal);
        ppu.vram[..0x3C0].fill(1);
        ppu.oam_data[0] = y;
        ppu.oam_data[1] = tile;
        ppu.oam_data[3] = x;
        ppu.write_to_mask(mask);
        ppu
    }

    const SHOW_BG_AND_SPRITES: u8 = 0b0001_1000;

    fn sprite_zero_hit(ppu: &PPU) -> bool {
        ppu.status.contains(StatusRegister::SPRITE_ZERO_HIT)
    }

    fn tick_scanlines(ppu: &mut PPU, n: usize) {
        for _ in 0..n {
            ppu.tick(CYCLES_PER_SCANLINE);
        }
    }

    #[test]
    fn sprite_zero_hit_is_set_once_the_sprites_first_scanline_has_rendered() {
        // Sprites appear one scanline below their OAM Y, so Y=30 covers 31..=38.
        let mut ppu = ppu_with_sprite_zero(100, 30, 1, SHOW_BG_AND_SPRITES);
        tick_scanlines(&mut ppu, 31);
        assert!(!sprite_zero_hit(&ppu), "hit before scanline 31 finished");
        ppu.tick(CYCLES_PER_SCANLINE);
        assert!(sprite_zero_hit(&ppu), "no hit after scanline 31 finished");
    }

    #[test]
    fn sprite_zero_hit_waits_for_the_sprites_first_opaque_row() {
        // Tile 2 is opaque only on row 6: Y=24 -> scanline 24 + 1 + 6 = 31.
        let mut ppu = ppu_with_sprite_zero(88, 24, 2, SHOW_BG_AND_SPRITES);
        tick_scanlines(&mut ppu, 31);
        assert!(!sprite_zero_hit(&ppu), "hit before the opaque row");
        ppu.tick(CYCLES_PER_SCANLINE);
        assert!(sprite_zero_hit(&ppu), "no hit on the opaque row");
    }

    #[test]
    fn sprite_zero_vertical_flip_moves_the_opaque_row() {
        let mut ppu = ppu_with_sprite_zero(88, 24, 2, SHOW_BG_AND_SPRITES);
        ppu.oam_data[2] = 0b1000_0000; // flipped: row 6 becomes row 1 -> scanline 26
        tick_scanlines(&mut ppu, 26);
        assert!(!sprite_zero_hit(&ppu));
        ppu.tick(CYCLES_PER_SCANLINE);
        assert!(sprite_zero_hit(&ppu));
    }

    #[test]
    fn sprite_zero_hit_requires_an_opaque_background_pixel() {
        let mut ppu = ppu_with_sprite_zero(100, 30, 1, SHOW_BG_AND_SPRITES);
        ppu.vram[..0x3C0].fill(0); // transparent background everywhere
        tick_scanlines(&mut ppu, 240);
        assert!(!sprite_zero_hit(&ppu));
    }

    #[test]
    fn sprite_zero_hit_requires_sprites_and_background_to_be_shown() {
        let mut only_sprites = ppu_with_sprite_zero(100, 30, 1, 0b0001_0000);
        tick_scanlines(&mut only_sprites, 240);
        assert!(!sprite_zero_hit(&only_sprites));

        let mut only_background = ppu_with_sprite_zero(100, 30, 1, 0b0000_1000);
        tick_scanlines(&mut only_background, 240);
        assert!(!sprite_zero_hit(&only_background));
    }

    #[test]
    fn sprite_zero_hit_uses_the_scrolled_background() {
        // Background is solid only in the right half of nametable A; with
        // scroll_x = 128 the sprite at screen x=0 sits over column 128.
        let mut ppu = ppu_with_sprite_zero(0, 30, 1, SHOW_BG_AND_SPRITES);
        for row in 0..30 {
            for col in 0..16 {
                ppu.vram[row * 32 + col] = 0;
            }
        }
        tick_scanlines(&mut ppu, 40);
        assert!(!sprite_zero_hit(&ppu), "hit over a transparent tile");

        let mut ppu = ppu_with_sprite_zero(0, 30, 1, SHOW_BG_AND_SPRITES);
        for row in 0..30 {
            for col in 0..16 {
                ppu.vram[row * 32 + col] = 0;
            }
        }
        ppu.write_to_scroll(128);
        ppu.write_to_scroll(0);
        tick_scanlines(&mut ppu, 40);
        assert!(
            sprite_zero_hit(&ppu),
            "no hit over the scrolled-in solid tile"
        );
    }

    #[test]
    fn sprite_zero_hit_is_cleared_when_vblank_starts() {
        let mut ppu = ppu_with_sprite_zero(100, 30, 1, SHOW_BG_AND_SPRITES);
        tick_scanlines(&mut ppu, CYCLE_AT_NMI_INTERUPT as usize - 1);
        assert!(sprite_zero_hit(&ppu));
        ppu.tick(CYCLES_PER_SCANLINE); // enters scanline 241
        assert!(!sprite_zero_hit(&ppu));
        assert!(ppu.status.is_in_vblank());
    }

    // Scroll splits
    #[test]
    fn scroll_written_during_vblank_becomes_the_next_frames_scroll() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        tick_scanlines(&mut ppu, 245);
        ppu.write_to_scroll(5);
        ppu.write_to_scroll(7);
        ppu.write_to_control(0b01);
        tick_scanlines(&mut ppu, 262 - 245); // wraps into the next frame
        assert_eq!(
            ppu.frame_scroll,
            ScrollState {
                x: 5,
                y: 7,
                nametable_addr: 0x2400
            }
        );
        assert!(ppu.scroll_splits.is_empty());
    }

    #[test]
    fn scroll_written_mid_frame_is_recorded_as_a_split_at_that_scanline() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        tick_scanlines(&mut ppu, 26);
        ppu.write_to_scroll(147);
        ppu.write_to_scroll(0);
        ppu.write_to_control(0b01);
        assert_eq!(ppu.frame_scroll, ScrollState::default());
        assert_eq!(
            ppu.scroll_splits,
            vec![ScrollSplit {
                scanline: 26,
                state: ScrollState {
                    x: 147,
                    y: 0,
                    nametable_addr: 0x2400
                }
            }]
        );
    }

    #[test]
    fn scroll_written_during_hblank_applies_from_the_next_scanline() {
        // The visible dots of a scanline are 0..256; a write after that cannot
        // change the row already drawn, so the split belongs to the next row.
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        tick_scanlines(&mut ppu, 31);
        ppu.tick(300); // scanline 31, dot 300: horizontal blank
        ppu.write_to_scroll(147);
        ppu.tick(60); // crosses into scanline 32
        ppu.write_to_scroll(0);
        ppu.write_to_control(0b01);
        assert_eq!(
            ppu.scroll_splits,
            vec![ScrollSplit {
                scanline: 32,
                state: ScrollState {
                    x: 147,
                    y: 0,
                    nametable_addr: 0x2400
                }
            }]
        );
    }

    #[test]
    fn scroll_splits_are_cleared_when_the_next_frame_starts() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        tick_scanlines(&mut ppu, 26);
        ppu.write_to_scroll(147);
        ppu.write_to_scroll(0);
        tick_scanlines(&mut ppu, 262 - 26);
        assert!(ppu.scroll_splits.is_empty());
        assert_eq!(ppu.frame_scroll.x, 147);
    }

    #[test]
    fn ppu_addr_writes_set_the_scroll_window() {
        // $2443: nametable 1, coarse x 3, coarse y 2, fine y 2 (bit 13).
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_scroll(5); // fine x = 5 survives a $2006 write
        ppu.write_to_scroll(0);
        ppu.write_to_addr(0x24);
        ppu.write_to_addr(0x43);
        assert_eq!(
            ppu.scroll_state(),
            ScrollState {
                x: 3 * 8 + 5,
                y: 2 * 8 + 2,
                nametable_addr: 0x2400
            }
        );
    }

    #[test]
    fn smb_style_vblank_sequence_starts_the_frame_on_nametable_0() {
        // PPUCTRL selects nametable 1, then the game resets the address to
        // $0000 and writes scroll (0, 0): the $2006 write clears the nametable.
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        tick_scanlines(&mut ppu, 242);
        ppu.write_to_control(0b01);
        ppu.write_to_addr(0x00);
        ppu.write_to_addr(0x00);
        ppu.write_to_scroll(0);
        ppu.write_to_scroll(0);
        tick_scanlines(&mut ppu, 262 - 242);
        assert_eq!(ppu.frame_scroll, ScrollState::default());
    }

    #[test]
    fn ppu_addr_write_mid_frame_records_a_split() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        tick_scanlines(&mut ppu, 100);
        ppu.write_to_addr(0x28);
        ppu.write_to_addr(0x00);
        assert_eq!(
            ppu.scroll_splits,
            vec![ScrollSplit {
                scanline: 100,
                state: ScrollState {
                    x: 0,
                    y: 2, // bit 13 of $2800 is fine Y bit 1
                    nametable_addr: 0x2800
                }
            }]
        );
    }

    // OAM Register
    #[test]
    fn oam_addr_and_auto_increment_on_data_write() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_oam_addr(0x02);
        ppu.write_to_oam_data(0xAA);
        ppu.write_to_oam_data(0xBB);
        assert_eq!(ppu.oam_addr, 0x04);
        ppu.write_to_oam_addr(0x02);
        assert_eq!(ppu.read_oam_data(), 0xAA);
        ppu.write_to_oam_addr(0x03);
        assert_eq!(ppu.read_oam_data(), 0xBB);
    }

    #[test]
    fn oam_dma_writes_page() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        let mut page = [0u8; 0x100];
        page[0] = 0x10;
        page[0xFF] = 0xEF;
        ppu.write_to_oam_addr(0x00);
        ppu.write_oam_dma(&page);
        assert_eq!(ppu.oam_addr, 0x00);
        assert_eq!(ppu.oam_data[0], 0x10);
        assert_eq!(ppu.oam_data[0xFF], 0xEF);
    }

    // VRAM read/write
    #[test]
    fn vram_write_and_buffered_read() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x2000);
        ppu.write_to_data(0x77);
        set_vram_addr(&mut ppu, 0x2000);
        let first = ppu.read_data();
        let second = ppu.read_data();
        assert_eq!(first, 0);
        assert_eq!(second, 0x77);
        assert_eq!(ppu.vram[0], 0x77);
    }

    #[test]
    fn palette_write_and_read() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(ControlRegister::VRAM_ADD_INCREMENT.bits());
        set_vram_addr(&mut ppu, 0x3F01);
        ppu.write_to_data(0x0D);
        set_vram_addr(&mut ppu, 0x3F01);
        assert_eq!(ppu.read_data(), 0x0D);
        assert_eq!(ppu.palette_table[1], 0x0D);
    }

    #[test]
    fn palette_mirrors_3f10_to_3f00() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x3F10);
        ppu.write_to_data(0x2C);
        assert_eq!(ppu.palette_table[0], 0x2C);
    }

    #[test]
    fn palette_mirrors_3f20_through_3fff_onto_3f00_3f1f() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x3F21); // mirror of 0x3F01
        ppu.write_to_data(0x1A);
        assert_eq!(ppu.palette_table[1], 0x1A);
        set_vram_addr(&mut ppu, 0x3FFF); // mirror of 0x3F1F
        ppu.write_to_data(0x2B);
        assert_eq!(ppu.palette_table[0x1F], 0x2B);
        set_vram_addr(&mut ppu, 0x3F30); // mirror of 0x3F10 -> 0x3F00
        ppu.write_to_data(0x3C);
        assert_eq!(ppu.palette_table[0], 0x3C);
        set_vram_addr(&mut ppu, 0x3FE1);
        assert_eq!(ppu.read_data(), 0x1A);
    }

    #[test]
    fn chr_rom_write_is_ignored() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.chr_rom[0] = 0x55;
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x0000);
        ppu.write_to_data(0x99);
        assert_eq!(ppu.chr_rom[0], 0x55);
    }

    #[test]
    fn chr_rom_read_is_buffered_like_vram() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.chr_rom[0x10] = 0xCC;
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x0010);
        assert_eq!(ppu.read_data(), 0);
        assert_eq!(ppu.read_data(), 0xCC);
    }

    // Mirroring
    #[test]
    fn mirror_vertical_maps_nt2_to_nt0_and_nt3_to_nt1() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Vertical);
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x2000);
        ppu.write_to_data(0xA1);
        set_vram_addr(&mut ppu, 0x2800);
        ppu.write_to_data(0xA2);
        set_vram_addr(&mut ppu, 0x2400);
        ppu.write_to_data(0xB1);
        set_vram_addr(&mut ppu, 0x2C00);
        ppu.write_to_data(0xB2);

        assert_eq!(ppu.vram[0], 0xA2);
        assert_eq!(ppu.vram[0x400], 0xB2);
    }

    #[test]
    fn mirror_horizontal_maps_nt1_to_nt0_and_bottom_row_to_top() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Horizontal);
        ppu.write_to_control(0);
        set_vram_addr(&mut ppu, 0x2000);
        ppu.write_to_data(0xC0);
        set_vram_addr(&mut ppu, 0x2400);
        ppu.write_to_data(0xC1);
        set_vram_addr(&mut ppu, 0x2800);
        ppu.write_to_data(0xD0);
        set_vram_addr(&mut ppu, 0x2C00);
        ppu.write_to_data(0xD1);

        assert_eq!(ppu.vram[0], 0xC1);
        assert_eq!(ppu.vram[0x400], 0xD1);
    }

    #[test]
    fn mirror_four_screen_keeps_four_distinct_nametables() {
        let mut ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::FourScreen);
        ppu.write_to_control(0);
        for (addr, val) in [
            (0x2000u16, 1u8),
            (0x2400u16, 2u8),
            (0x2800u16, 3u8),
            (0x2C00u16, 4u8),
        ] {
            set_vram_addr(&mut ppu, addr);
            ppu.write_to_data(val);
        }
        assert_eq!(ppu.vram[0], 1);
        assert_eq!(ppu.vram[0x400], 2);
        assert_eq!(ppu.vram[0x800], 3);
        assert_eq!(ppu.vram[0xC00], 4);
    }

    #[test]
    fn mirror_vram_addr_matches_write_physical_index() {
        let ppu = PPU::new(vec![0u8; CHR_ROM_PAGE_SIZE], Mirror::Vertical);
        assert_eq!(ppu.mirror_vram_addr(0x2000), 0);
        assert_eq!(ppu.mirror_vram_addr(0x2800), 0);
        assert_eq!(ppu.mirror_vram_addr(0x2400), 0x400);
        assert_eq!(ppu.mirror_vram_addr(0x2C00), 0x400);
    }
}
