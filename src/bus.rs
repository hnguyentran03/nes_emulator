use crate::cpu::Mem;
use crate::rom::Rom;
use crate::rom::{PRG_ROM_PAGE_SIZE, PRG_ROM, PRG_ROM_END};
use crate::ppu::PPU;
use crate::ppu::PPU_REGISTERS_END;

const RAM: u16 = 0x0000;
const RAM_END: u16 = 0x1FFF;
const RAM_MIRROR_MASK: u16 = 0b0000_0111_1111_1111; // CPU RAM only has 11 bits of space

// Addresses for each of the PPU Registers
const CONTROL_REGISTER: u16 = 0x2000;
const MASK_REGISTER: u16 = 0x2001;
const STATUS_REGISTER: u16 = 0x2002;
const OAM_ADDR: u16 = 0x2003;
const OAM_DATA: u16 = 0x2004;
const SCROLL_REGISTER: u16 = 0x2005;
const PPU_ADDR: u16 = 0x2006;
const PPU_DATA: u16 = 0x2007;
const OAM_DMA: u16 = 0x4014;
const PPU_MEM: u16 = 0x2008;
const PPU_MEM_END: u16 = PPU_REGISTERS_END;

const PPU_MIRROR_MASK: u16 = 0b0010_0000_0000_0111;

impl Mem for Bus {
    fn mem_read(&mut self, addr: u16) -> u8 {
        match addr {
            RAM..=RAM_END => {
                let mirror_addr = addr & RAM_MIRROR_MASK;
                self.cpu_vram[mirror_addr as usize]
            }

            CONTROL_REGISTER | MASK_REGISTER | OAM_ADDR | SCROLL_REGISTER | PPU_ADDR | OAM_DMA => {
                panic!("Attempt to read from write-only PPU address {:x}", addr);
            }

            STATUS_REGISTER => self.ppu.read_status(),

            OAM_DATA => self.ppu.read_oam_data(),

            PPU_DATA => self.ppu.read_data(),

            PPU_MEM..=PPU_REGISTERS_END => {
                let mirror_addr = addr & PPU_MIRROR_MASK;
                self.mem_read(mirror_addr)
            }

            PRG_ROM..=PRG_ROM_END => self.read_prg_rom(addr),

            _ => 0, // Ignore reads from other memory areas
        }
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        match addr {
            RAM..=RAM_END => {
                let mirror_addr = addr & RAM_MIRROR_MASK;
                self.cpu_vram[mirror_addr as usize] = data;
            }

            CONTROL_REGISTER => self.ppu.write_to_control(data),

            MASK_REGISTER => self.ppu.write_to_mask(data),

            STATUS_REGISTER => panic!("attempt to write to PPU status register"),

            OAM_ADDR => self.ppu.write_to_oam_addr(data),

            OAM_DMA => self.ppu.write_to_oam_data(data),

            SCROLL_REGISTER => self.ppu.write_to_scroll(data),

            PPU_ADDR => self.ppu.write_to_addr(data),

            PPU_DATA => self.ppu.write_to_data(data),

            PPU_MEM..=PPU_MEM_END => {
                let mirror_addr = addr & PPU_MIRROR_MASK;
                self.mem_write(mirror_addr, data);
            }

            PRG_ROM..=PRG_ROM_END => panic!("Attempting to write to PRG ROM"),

            _ => { /* Ignore writes to other memory areas */ }
        }
    }
}

pub struct Bus {
    cpu_vram: [u8; 0x800],
    prg_rom: Vec<u8>,
    ppu: PPU,
}

impl Bus {
    pub fn new(rom: Rom) -> Self {
        let ppu = PPU::new(rom.chr_rom, rom.screen_mirror);

        Bus {
            cpu_vram: [0; 0x800],
            prg_rom: rom.prg_rom,
            ppu,
        }
    }

    fn read_prg_rom(&self, mut addr: u16) -> u8 {
        addr -= PRG_ROM;

        // Mirrors the PRG ROM if necessary
        if self.prg_rom.len() == PRG_ROM_PAGE_SIZE && addr >= PRG_ROM_PAGE_SIZE as u16 {
            addr %= PRG_ROM_PAGE_SIZE as u16;
        }

        self.prg_rom[addr as usize]
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rom::test::build_test_rom;

    #[test]
    fn test_mem_read_write_to_ram() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(0x01, 0x55);
        assert_eq!(bus.mem_read(0x01), 0x55);
    }
}
