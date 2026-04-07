use crate::rom::Mirror;
use crate::ppu::registers::{
    addr::AddrRegister,
    control::ControlRegister,
    mask::MaskRegister,
    scroll::ScrollRegister,
    status::StatusRegister,
};

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

            _ => panic!("Unexpected acces to mirrored space {}", addr)
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