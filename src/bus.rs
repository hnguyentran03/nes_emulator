use crate::cpu::Mem;
use crate::rom::Rom;
use crate::rom::PRG_ROM_PAGE_SIZE;

const RAM: u16 = 0x0000;
const RAM_END: u16 = 0x1FFF;
const PPU_REGISTERS: u16 = 0x2000;
const PPU_REGISTERS_END: u16 = 0x3FFF;
const PRG_ROM: u16 = 0x8000;
const PRG_ROM_END: u16 = 0xFFFF;

impl Mem for Bus {
    fn mem_read(&self, addr: u16) -> u8 {
        match addr {
            RAM..=RAM_END => {
                // CPU RAM only has 11 bits of space
                let mirror_addr = addr & 0b0000_0111_1111_1111;
                self.cpu_vram[mirror_addr as usize]
            },
            PPU_REGISTERS..=PPU_REGISTERS_END => {
                let _mirror_addr = addr & 0b0010_0000_0000_0111;
                // todo!("PPU not yet implemented");
                0
            },
            PRG_ROM..=PRG_ROM_END => self.read_prg_rom(addr),
            _ => 0, // Ignore reads from other memory areas
        }
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        match addr {
            RAM..=RAM_END => {
                // CPU RAM only has 11 bits of space
                let mirror_addr = addr & 0b0000_0111_1111_1111;
                self.cpu_vram[mirror_addr as usize] = data;
            },
            PPU_REGISTERS..=PPU_REGISTERS_END => {
                let _mirror_addr = addr & 0b0010_0000_0000_0111;
                // todo!("PPU not yet implemented");
            },
            PRG_ROM..=PRG_ROM_END => {
                panic!("Attempting to write to PRG ROM");
            },
            _ => { /* Ignore writes to other memory areas */ },
        }
    }
}

pub struct Bus {
    cpu_vram: [u8; 0x800],
    rom: Rom,
}

impl Bus {
    pub fn new(rom: Rom) -> Self {
        Bus {
            cpu_vram: [0; 0x800],
            rom,
        }
    }

    fn read_prg_rom(&self, mut addr: u16) -> u8 {
        addr -= PRG_ROM;
        
        // Mirrors the PRG ROM if necessary
        if self.rom.prg_rom.len() == PRG_ROM_PAGE_SIZE && addr >= PRG_ROM_PAGE_SIZE as u16 {
            addr = addr % PRG_ROM_PAGE_SIZE as u16;
        }

        self.rom.prg_rom[addr as usize]
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