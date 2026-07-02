use crate::cpu::Mem;
use crate::ppu::PPU;
use crate::ppu::PPU_REGISTERS_END;
use crate::rom::Rom;
use crate::rom::{PRG_ROM, PRG_ROM_END, PRG_ROM_PAGE_SIZE};

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
const PPU_CYCLE_PER_CPU_CYCLE: usize = 3;

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
    cycles: usize,
    irq_line: bool,
}

impl Bus {
    pub fn new(rom: Rom) -> Self {
        let ppu = PPU::new(rom.chr_rom, rom.screen_mirror);

        Bus {
            cpu_vram: [0; 0x800],
            prg_rom: rom.prg_rom,
            ppu,
            cycles: 0,
            irq_line: false,
        }
    }

    pub fn tick(&mut self, cycles: usize) {
        self.cycles += cycles;
        self.ppu.tick(cycles * PPU_CYCLE_PER_CPU_CYCLE);
    }

    pub fn poll_nmi_interupted(&mut self) -> Option<u8> {
        self.ppu.poll_nmi_interupted()
    }

    pub fn poll_irq(&mut self) -> bool {
        self.irq_line
    }

    pub fn set_irq(&mut self, level: bool) {
        self.irq_line = level;
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
    use crate::ppu::registers::control::ControlRegister;
    use crate::ppu::registers::status::StatusRegister;
    use crate::rom::CHR_ROM_PAGE_SIZE;
    use crate::rom::test::build_test_rom;

    fn single_bank_test_rom(prg_first: u8) -> Rom {
        let header: [u8; 16] = [
            0x4E, 0x45, 0x53, 0x1A, 0x01, 0x01, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let mut prg = vec![0u8; PRG_ROM_PAGE_SIZE];
        prg[0] = prg_first;
        prg[0x0123] = 0x77;
        let chr = vec![0u8; CHR_ROM_PAGE_SIZE];
        let mut data = header.to_vec();
        data.extend_from_slice(&prg);
        data.extend_from_slice(&chr);
        Rom::new(&data).unwrap()
    }

    #[test]
    fn test_mem_read_write_to_ram() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(0x01, 0x55);
        assert_eq!(bus.mem_read(0x01), 0x55);
    }

    #[test]
    fn ram_mirrors_at_2k_boundary() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(0x0001, 0xAA);
        bus.mem_write(0x0801, 0xBB);
        assert_eq!(bus.mem_read(0x0001), 0xBB);
        assert_eq!(bus.mem_read(0x1001), 0xBB);
        assert_eq!(bus.mem_read(0x1801), 0xBB);
    }

    #[test]
    fn prg_rom_read_two_banks() {
        let mut bus = Bus::new(build_test_rom(vec![0xEA, 0x4C, 0x00, 0x80]));
        assert_eq!(bus.mem_read(PRG_ROM), 0xEA);
        assert_eq!(bus.mem_read(PRG_ROM + 0x4000), bus.prg_rom[0x4000]);
    }

    #[test]
    fn prg_rom_single_bank_mirrors_into_c000() {
        let mut bus = Bus::new(single_bank_test_rom(0xAB));
        assert_eq!(bus.mem_read(PRG_ROM), 0xAB);
        assert_eq!(bus.mem_read(0xC000), 0xAB);
        assert_eq!(bus.mem_read(PRG_ROM + 0x0123), 0x77);
        assert_eq!(bus.mem_read(0xC000 + 0x0123), 0x77);
    }

    #[test]
    fn unmapped_read_returns_open_bus_zero() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        assert_eq!(bus.mem_read(0x5000), 0);
    }

    #[test]
    fn ppu_mirrored_register_write_aliases_base_port() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(0x2008, 0x80);
        bus.mem_write(0x2018, 0x80);
        assert_eq!(bus.ppu.control.bits(), 0x80);
    }

    #[test]
    fn ppu_status_read_via_cpu_and_clears_vblank() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.ppu.status.set_vblank_status(true);
        let raw = bus.mem_read(STATUS_REGISTER);
        assert_ne!(raw & StatusRegister::VBLANK_STARTED.bits(), 0);
        assert!(!bus.ppu.status.is_in_vblank());
    }

    #[test]
    fn ppu_oam_4014_write_path() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(OAM_ADDR, 0x02);
        bus.mem_write(OAM_DMA, 0x11);
        bus.mem_write(OAM_DMA, 0x22);
        assert_eq!(bus.ppu.oam_addr, 0x04);
        bus.mem_write(OAM_ADDR, 0x02);
        assert_eq!(bus.mem_read(OAM_DATA), 0x11);
    }

    #[test]
    fn ppu_addr_and_data_reach_vram_through_bus() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(
            CONTROL_REGISTER,
            ControlRegister::VRAM_ADD_INCREMENT.bits(),
        );
        bus.mem_write(PPU_ADDR, 0x20);
        bus.mem_write(PPU_ADDR, 0x00);
        bus.mem_write(PPU_DATA, 0xEE);
        assert_eq!(bus.ppu.vram[0], 0xEE);
    }

    #[test]
    #[should_panic(expected = "Attempt to read from write-only PPU address 2000")]
    fn panic_on_read_write_only_ppu_control() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        let _ = bus.mem_read(CONTROL_REGISTER);
    }

    #[test]
    #[should_panic(expected = "attempt to write to PPU status register")]
    fn panic_on_write_ppu_status() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(STATUS_REGISTER, 0);
    }

    #[test]
    #[should_panic(expected = "Attempting to write to PRG ROM")]
    fn panic_on_write_prg_rom() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(PRG_ROM, 0xFF);
    }
}
