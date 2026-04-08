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

const MIRROR_MASK: u16 = 0b1011_1111_1111_1111; // Mirrors from 0x3000-0x3EFF to 0x2000-0x2EFF
const SCREEN_STATE_SIZE: u16 = 0x400; // 1 KiB

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
        }
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
                self.palette_table[(addr - PALETTE_TABLE_START) as usize]
            }

            _ => {
                panic!("Unexpected address to mirrored space: {}", addr);
            }
        }
    }

    pub fn write_to_addr(&mut self, data: u8) {
        self.addr.update(data);
    }

    pub fn write_to_control(&mut self, data: u8) {
        self.control.update(data);
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
    }

    pub fn write_to_data(&mut self, data: u8) {
        let addr = self.addr.get();
        match addr {
            CHR_ROM_START..=CHR_ROM_END => { /* Ignore attempts to write to CHR ROM */ } // TODO double check

            VRAM_START..=VRAM_END => {
                self.vram[self.mirror_vram_addr(addr) as usize] = data;
            }

            UNUSED_START..=UNUSED_END => { /* Ignore attempts to write to unused addrs */ }

            // Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
            0x3f10 | 0x3f14 | 0x3f18 | 0x3f1c => {
                let add_mirror = addr - 0x10;
                self.palette_table[(add_mirror - PALETTE_TABLE_START) as usize] = data;
            }

            PALETTE_TABLE_START..=PALETTE_TABLE_END => {
                self.palette_table[(addr - PALETTE_TABLE_START) as usize] = data;
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
        assert_eq!(ppu.addr.get(), 0x2020);

        set_vram_addr(&mut ppu, 0x2000);
        ppu.write_to_control(ControlRegister::VRAM_ADD_INCREMENT.bits());
        ppu.write_to_data(0x02);
        assert_eq!(ppu.addr.get(), 0x2001);
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

