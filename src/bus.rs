use crate::apu::APU;
use crate::cpu::Mem;
use crate::joypad::Joypad;
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
const JOYPAD_1: u16 = 0x4016;
const JOYPAD_2: u16 = 0x4017;
const PPU_MEM: u16 = 0x2008;
const PPU_MEM_END: u16 = PPU_REGISTERS_END;

// Addresses for the APU registers
const APU_REGISTERS: u16 = 0x4000;
const APU_REGISTERS_END: u16 = 0x4013;
const APU_STATUS: u16 = 0x4015;
const APU_FRAME_COUNTER: u16 = 0x4017; // shares the address with joypad 2 (read)

const PPU_MIRROR_MASK: u16 = 0b0010_0000_0000_0111;
const PPU_CYCLE_PER_CPU_CYCLE: usize = 3;

impl Mem for Bus<'_> {
    fn mem_read(&mut self, addr: u16) -> u8 {
        match addr {
            RAM..=RAM_END => {
                let mirror_addr = addr & RAM_MIRROR_MASK;
                self.cpu_vram[mirror_addr as usize]
            }

            CONTROL_REGISTER | MASK_REGISTER | OAM_ADDR | SCROLL_REGISTER | PPU_ADDR | OAM_DMA => {
                panic!("Attempt to read from write-only PPU address {:x}", addr);
            }

            APU_REGISTERS..=APU_REGISTERS_END => {
                panic!("Attempt to read from write-only APU address {:x}", addr);
            }

            APU_STATUS => self.apu.read_status(),

            STATUS_REGISTER => self.ppu.read_status(),

            OAM_DATA => self.ppu.read_oam_data(),

            PPU_DATA => self.ppu.read_data(),

            JOYPAD_1 => self.joypad1.read(),

            JOYPAD_2 => 0, // second controller not emulated

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

            OAM_DATA => self.ppu.write_to_oam_data(data),

            // Writing page number N to $4014 copies CPU memory $NN00-$NNFF into OAM.
            // https://www.nesdev.org/wiki/PPU_registers#OAMDMA_-_Sprite_DMA_($4014_write)
            // TODO: the DMA stalls the CPU for 513/514 cycles; not modelled yet.
            OAM_DMA => {
                let hi = (data as u16) << 8;
                let mut buffer = [0u8; 0x100];
                for (i, slot) in buffer.iter_mut().enumerate() {
                    *slot = self.mem_read(hi + i as u16);
                }
                self.ppu.write_oam_dma(&buffer);
            }

            SCROLL_REGISTER => self.ppu.write_to_scroll(data),

            PPU_ADDR => self.ppu.write_to_addr(data),

            PPU_DATA => self.ppu.write_to_data(data),

            // Bit 0 is the controller strobe; the same write also reaches
            // joypad 2, which is not emulated.
            JOYPAD_1 => self.joypad1.write(data),

            APU_REGISTERS..=APU_REGISTERS_END => self.apu.write_to_register(addr, data),

            APU_STATUS => self.apu.write_to_status(data),

            // $4017 is the frame counter when written and joypad 2 when read.
            APU_FRAME_COUNTER => self.apu.write_to_frame_counter(data),

            PPU_MEM..=PPU_MEM_END => {
                let mirror_addr = addr & PPU_MIRROR_MASK;
                self.mem_write(mirror_addr, data);
            }

            PRG_ROM..=PRG_ROM_END => panic!("Attempting to write to PRG ROM"),

            _ => { /* Ignore writes to other memory areas */ }
        }
    }
}

// Invoked once per frame, on the rising edge of the PPU's vblank NMI.
pub type GameloopCallback<'call> = Box<dyn FnMut(&PPU, &mut Joypad) + 'call>;

// Invoked once per frame, when the PPU finishes its last scanline, with every
// sample the APU produced since the previous call. Unlike the gameloop
// callback it does not need the ROM to enable NMI.
pub type AudioCallback<'call> = Box<dyn FnMut(&[f32]) + 'call>;

pub struct Bus<'call> {
    cpu_vram: [u8; 0x800],
    prg_rom: Vec<u8>,
    ppu: PPU,
    apu: APU,
    cycles: usize,
    irq_line: bool,
    joypad1: Joypad,
    gameloop_callback: GameloopCallback<'call>,
    audio_callback: Option<AudioCallback<'call>>,
}

impl<'call> Bus<'call> {
    pub fn new<F>(rom: Rom, gameloop_callback: F) -> Self
    where
        F: FnMut(&PPU, &mut Joypad) + 'call,
    {
        let ppu = PPU::new(rom.chr_rom, rom.screen_mirror);

        Bus {
            cpu_vram: [0; 0x800],
            prg_rom: rom.prg_rom,
            ppu,
            apu: APU::new(),
            cycles: 0,
            irq_line: false,
            joypad1: Joypad::new(),
            gameloop_callback: Box::from(gameloop_callback),
            audio_callback: None,
        }
    }

    pub fn set_audio_callback<F>(&mut self, audio_callback: F)
    where
        F: FnMut(&[f32]) + 'call,
    {
        self.audio_callback = Some(Box::from(audio_callback));
    }

    pub fn tick(&mut self, cycles: usize) {
        self.cycles += cycles;

        self.apu.tick(cycles);
        // The DMC cannot read CPU memory itself; fetch the byte it asked for.
        // One request per tick is enough: its shortest period (54 cycles) is
        // longer than any instruction.
        if let Some(addr) = self.apu.dmc_fetch_request() {
            let data = self.mem_read(addr);
            self.apu.dmc_fetch_complete(data);
        }

        let nmi_before = self.ppu.nmi_interrupted.is_some();
        let frame_done = self.ppu.tick(cycles * PPU_CYCLE_PER_CPU_CYCLE);
        let nmi_after = self.ppu.nmi_interrupted.is_some();

        if !nmi_before && nmi_after {
            (self.gameloop_callback)(&self.ppu, &mut self.joypad1);
        }

        if frame_done {
            if let Some(audio_callback) = self.audio_callback.as_mut() {
                audio_callback(self.apu.samples());
            }
            self.apu.clear_samples();
        }
    }

    pub fn poll_nmi_interrupted(&mut self) -> Option<u8> {
        self.ppu.poll_nmi_interrupted()
    }

    // The APU's frame counter and DMC drive the line; `set_irq` remains for
    // tests and any future mapper.
    pub fn poll_irq(&mut self) -> bool {
        self.irq_line || self.apu.irq_pending()
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
    use crate::joypad::JoypadButton;
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
    fn gameloop_callback_fires_once_per_vblank_nmi() {
        use std::cell::Cell;
        use std::rc::Rc;

        let frames = Rc::new(Cell::new(0));
        let counter = Rc::clone(&frames);
        let mut bus = Bus::new(
            build_test_rom(vec![]),
            move |_ppu: &PPU, _joypad: &mut Joypad| {
                counter.set(counter.get() + 1);
            },
        );

        // Tick a couple of CPU cycles at a time (like the run loop does) for
        // one full frame: 341 PPU cycles x 262 scanlines = 89342 PPU cycles.
        let run_frame = |bus: &mut Bus| {
            for _ in 0..(341 * 262 / PPU_CYCLE_PER_CPU_CYCLE / 2 + 1) {
                bus.tick(2);
                bus.poll_nmi_interrupted();
            }
        };

        // Without NMI enabled a whole frame produces no callback.
        run_frame(&mut bus);
        assert_eq!(frames.get(), 0);

        bus.mem_write(CONTROL_REGISTER, ControlRegister::GENERATE_NMI.bits());
        run_frame(&mut bus);
        run_frame(&mut bus);
        assert_eq!(frames.get(), 2);
    }

    #[test]
    fn test_mem_read_write_to_ram() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(0x01, 0x55);
        assert_eq!(bus.mem_read(0x01), 0x55);
    }

    #[test]
    fn ram_mirrors_at_2k_boundary() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(0x0001, 0xAA);
        bus.mem_write(0x0801, 0xBB);
        assert_eq!(bus.mem_read(0x0001), 0xBB);
        assert_eq!(bus.mem_read(0x1001), 0xBB);
        assert_eq!(bus.mem_read(0x1801), 0xBB);
    }

    #[test]
    fn prg_rom_read_two_banks() {
        let mut bus = Bus::new(build_test_rom(vec![0xEA, 0x4C, 0x00, 0x80]), |_, _| {});
        assert_eq!(bus.mem_read(PRG_ROM), 0xEA);
        assert_eq!(bus.mem_read(PRG_ROM + 0x4000), bus.prg_rom[0x4000]);
    }

    #[test]
    fn prg_rom_single_bank_mirrors_into_c000() {
        let mut bus = Bus::new(single_bank_test_rom(0xAB), |_, _| {});
        assert_eq!(bus.mem_read(PRG_ROM), 0xAB);
        assert_eq!(bus.mem_read(0xC000), 0xAB);
        assert_eq!(bus.mem_read(PRG_ROM + 0x0123), 0x77);
        assert_eq!(bus.mem_read(0xC000 + 0x0123), 0x77);
    }

    #[test]
    fn unmapped_read_returns_open_bus_zero() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        assert_eq!(bus.mem_read(0x5000), 0);
    }

    #[test]
    fn ppu_mirrored_register_write_aliases_base_port() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(0x2008, 0x80);
        bus.mem_write(0x2018, 0x80);
        assert_eq!(bus.ppu.control.bits(), 0x80);
    }

    #[test]
    fn ppu_status_read_via_cpu_and_clears_vblank() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.ppu.status.set_vblank_status(true);
        let raw = bus.mem_read(STATUS_REGISTER);
        assert_ne!(raw & StatusRegister::VBLANK_STARTED.bits(), 0);
        assert!(!bus.ppu.status.is_in_vblank());
    }

    #[test]
    fn ppu_oam_data_write_through_2004_auto_increments() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(OAM_ADDR, 0x02);
        bus.mem_write(OAM_DATA, 0x11);
        bus.mem_write(OAM_DATA, 0x22);
        assert_eq!(bus.ppu.oam_addr, 0x04);
        bus.mem_write(OAM_ADDR, 0x02);
        assert_eq!(bus.mem_read(OAM_DATA), 0x11);
        bus.mem_write(OAM_ADDR, 0x03);
        assert_eq!(bus.mem_read(OAM_DATA), 0x22);
    }

    #[test]
    fn ppu_oam_dma_4014_copies_a_cpu_ram_page_into_oam() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        for i in 0..0x100u16 {
            bus.mem_write(0x0200 + i, i as u8);
        }
        bus.mem_write(OAM_ADDR, 0x00);
        bus.mem_write(OAM_DMA, 0x02);
        assert_eq!(bus.ppu.oam_data[0x00], 0x00);
        assert_eq!(bus.ppu.oam_data[0x7F], 0x7F);
        assert_eq!(bus.ppu.oam_data[0xFF], 0xFF);
        // 256 writes wrap the OAM address back to where it started.
        assert_eq!(bus.ppu.oam_addr, 0x00);
    }

    #[test]
    fn joypad1_strobe_protocol_through_4016() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.joypad1
            .set_button_pressed_status(JoypadButton::BUTTON_A, true);
        bus.joypad1
            .set_button_pressed_status(JoypadButton::START, true);
        bus.joypad1
            .set_button_pressed_status(JoypadButton::LEFT, true);

        bus.mem_write(JOYPAD_1, 1);
        bus.mem_write(JOYPAD_1, 0);
        // A, B, Select, Start, Up, Down, Left, Right
        let expected = [1, 0, 0, 1, 0, 0, 1, 0];
        for want in expected {
            assert_eq!(bus.mem_read(JOYPAD_1), want);
        }
        assert_eq!(bus.mem_read(JOYPAD_1), 1);
    }

    #[test]
    fn joypad2_reads_zero() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        assert_eq!(bus.mem_read(JOYPAD_2), 0);
    }

    #[test]
    fn gameloop_callback_can_press_buttons_seen_on_next_read() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_ppu, joypad| {
            joypad.set_button_pressed_status(JoypadButton::BUTTON_B, true);
        });
        bus.mem_write(CONTROL_REGISTER, ControlRegister::GENERATE_NMI.bits());
        for _ in 0..(341 * 262 / PPU_CYCLE_PER_CPU_CYCLE / 2 + 1) {
            bus.tick(2);
            bus.poll_nmi_interrupted();
        }
        bus.mem_write(JOYPAD_1, 1);
        bus.mem_write(JOYPAD_1, 0);
        assert_eq!(bus.mem_read(JOYPAD_1), 0); // A
        assert_eq!(bus.mem_read(JOYPAD_1), 1); // B
    }

    #[test]
    fn ppu_addr_and_data_reach_vram_through_bus() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(CONTROL_REGISTER, ControlRegister::VRAM_ADD_INCREMENT.bits());
        bus.mem_write(PPU_ADDR, 0x20);
        bus.mem_write(PPU_ADDR, 0x00);
        bus.mem_write(PPU_DATA, 0xEE);
        assert_eq!(bus.ppu.vram[0], 0xEE);
    }

    #[test]
    #[should_panic(expected = "Attempt to read from write-only PPU address 2000")]
    fn panic_on_read_write_only_ppu_control() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        let _ = bus.mem_read(CONTROL_REGISTER);
    }

    #[test]
    #[should_panic(expected = "attempt to write to PPU status register")]
    fn panic_on_write_ppu_status() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(STATUS_REGISTER, 0);
    }

    #[test]
    #[should_panic(expected = "Attempting to write to PRG ROM")]
    fn panic_on_write_prg_rom() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(PRG_ROM, 0xFF);
    }
    // Ticks like the run loop does, a couple of CPU cycles at a time.
    fn tick_cycles(bus: &mut Bus, cycles: usize) {
        for _ in 0..cycles / 2 {
            bus.tick(2);
        }
    }

    // One PPU frame: 341 x 262 PPU cycles, rounded up to whole 2-cycle ticks.
    const CPU_CYCLES_PER_FRAME: usize = (341 * 262 / PPU_CYCLE_PER_CPU_CYCLE / 2 + 1) * 2;

    #[test]
    fn apu_register_writes_reach_the_channels() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(0x4006, 0xAB);
        assert_eq!(bus.apu.pulse_2.timer_period, 0xAB);
    }

    #[test]
    fn apu_status_round_trips_through_4015() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(APU_STATUS, 0b0000_0001);
        bus.mem_write(0x4003, 0); // pulse 1 length 10
        assert_eq!(bus.mem_read(APU_STATUS) & 0b0001_1111, 0b0000_0001);
    }

    #[test]
    fn frame_counter_write_reaches_the_apu_and_4017_read_is_still_joypad_2() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(APU_FRAME_COUNTER, 0b1000_0000);
        assert!(bus.apu.frame_counter.five_step_mode);
        assert_eq!(bus.mem_read(JOYPAD_2), 0);
    }

    #[test]
    #[should_panic(expected = "Attempt to read from write-only APU address 4000")]
    fn panic_on_read_apu_channel_register() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        let _ = bus.mem_read(APU_REGISTERS);
    }

    #[test]
    fn frame_irq_raises_poll_irq_until_4015_is_read() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        tick_cycles(&mut bus, 29830);
        assert!(bus.poll_irq());
        assert_ne!(bus.mem_read(APU_STATUS) & 0b0100_0000, 0);
        assert!(!bus.poll_irq());
    }

    #[test]
    fn frame_irq_is_suppressed_by_4017_bit_6() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.mem_write(APU_FRAME_COUNTER, 0b0100_0000);
        tick_cycles(&mut bus, 40000);
        assert!(!bus.poll_irq());
    }

    #[test]
    fn set_irq_line_still_works_alongside_the_apu() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        assert!(!bus.poll_irq());
        bus.set_irq(true);
        assert!(bus.poll_irq());
    }

    #[test]
    fn dmc_fetch_is_serviced_within_one_bus_tick() {
        let mut prg = vec![0u8; 0x4001];
        prg[0x4000] = 0xAB; // lands at $C000
        let mut bus = Bus::new(build_test_rom(prg), |_, _| {});
        bus.mem_write(0x4012, 0x00);
        bus.mem_write(0x4013, 0x00);
        bus.mem_write(APU_STATUS, 0b0001_0000);
        bus.tick(7);
        assert_eq!(bus.apu.dmc.sample_buffer, Some(0xAB));
        assert_eq!(bus.apu.dmc_fetch_request(), None);
    }

    #[test]
    fn audio_callback_receives_a_frames_worth_of_samples() {
        use std::cell::Cell;
        use std::rc::Rc;

        let samples = Rc::new(Cell::new(0usize));
        let counter = Rc::clone(&samples);
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        bus.set_audio_callback(move |frame: &[f32]| {
            counter.set(counter.get() + frame.len());
        });

        tick_cycles(&mut bus, CPU_CYCLES_PER_FRAME);
        // 29782 CPU cycles / 40.58 cycles per sample.
        assert!(
            (700..=760).contains(&samples.get()),
            "{} samples",
            samples.get()
        );
        assert!(bus.apu.samples().len() < 10);
    }

    #[test]
    fn sample_buffer_is_cleared_each_frame_without_an_audio_callback() {
        let mut bus = Bus::new(build_test_rom(vec![]), |_, _| {});
        tick_cycles(&mut bus, 2 * CPU_CYCLES_PER_FRAME);
        assert!(bus.apu.samples().len() < 800);
    }
}
