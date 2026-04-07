use crate::rom::Mirror;
use crate::ppu::registers::addr::AddrRegister;
use crate::ppu::registers::control::ControlRegister;

pub mod registers;

pub const PPU_REGISTERS: u16 = 0x2000;
pub const PPU_REGISTERS_END: u16 = 0x3FFF;

const CHR_ROM_START: u16 = 0x0000;
const CHR_ROM_END: u16 = 0x1FFF;
const VRAM_START: u16 = 0x2000;
const VRAM_END: u16 = 0x2FFF;

const PALETTE_TABLE_START: u16 = 0x3F00;
const PALETTE_TABLE_END: u16 = 0x3FFF;

const MIRROR_MASK: u16 = 0b1011_1111_1111_1111; // Mirrors from 0x3000-0x3EFF to 0x2000-0x2EFF
const SCREEN_STATE_SIZE: u16 = 0x400; // 1 KiB

pub struct PPU {
    pub chr_rom: Vec<u8>,
    pub palettes: [u8; 0x20],
    pub vram: [u8; 0x4000],
    pub oam: [u8; 0x100],
    pub mirror: Mirror,
    internal_data_buf: u8,

    pub addr: AddrRegister,
    pub control: ControlRegister,
}

impl PPU {
    pub fn new(chr_rom: Vec<u8>, mirror: Mirror) -> Self {
        PPU {
            chr_rom,
            palettes: [0; 0x20],
            vram: [0; 0x4000],
            oam: [0; 0x100],
            mirror,

            addr: AddrRegister::new(),
            control: ControlRegister::new(),
        }
    }

    fn write_to_addr(&mut self, value: u8) {
        self.addr.update(value);
    }

    fn write_to_control(&mut self, value: u8) {
        self.control.update(value);
    }

    fn increment_vram_addr(&mut self) {
        self.addr.increment(self.control.vram_add_inc());
    }

    fn read_data(&mut self) -> u8 {
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
                self.palettes[addr - PALETTE_TABLE_START as usize]
            }
            _ => {
                panic!("Unexpected address to mirrored space: {}", addr);
            }
        }
    }

    pub fn mirror_vram_addr(&self, addr: u16) -> u16 {
        let mirror_vram = addr & MIRROR_MASK;
        let vram_index = mirror_vram - VRAM_START;
        let name_table = vram_index / NAME_TABLE_SIZE;
        match (&self.mirror, name_table) {
            (Mirror::Vertical, 2) | (Mirror::Vertical, 3) => vram_index - 2 * SCREEN_STATE_SIZE,
            (Mirror::Horizontal, 1) | (Mirror::Horizontal, 2) => vram_index - SCREEN_STATE_SIZE,
            (Mirror::Horizontal, 3) => vram_index - 2 * SCREEN_STATE_SIZE,
            _ => vram_index,
        }
    }
}