use std::collections::HashMap;
use crate::opcodes;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CPUFlags: u8 {
        const CARRY     = 0b00000001;
        const ZERO      = 0b00000010;
        const INTERRUPT = 0b00000100;
        const DECIMAL   = 0b00001000;
        const BREAK     = 0b00010000;
        const UNUSED    = 0b00100000;
        const OVERFLOW  = 0b01000000;
        const NEGATIVE  = 0b10000000;
    }
}

pub struct CPU {
    pub register_a: u8,
    pub register_x: u8,
    pub register_y: u8,
    pub flags: CPUFlags,
    pub program_counter: u16,
    memory: [u8; 0xFFFF],
}

#[derive(Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub enum AddressingMode {
    Immedtiate,
    ZeroPage,
    ZeroPage_X,
    ZeroPage_Y,
    Absolute,
    Absolute_X,
    Absolute_Y,
    Indirect_X,
    Indirect_Y,
    NoneAddressing,
}

trait Mem {
    fn mem_read(&self, addr: u16) -> u8;

    fn mem_write(&mut self, addr: u16, data: u8);

        fn mem_read_u16(&self, pos: u16) -> u16 {
        let lo = self.mem_read(pos) as u16;
        let hi = self.mem_read(pos + 1) as u16;
        (hi << 8) | lo
    }

    fn mem_write_u16(&mut self, pos: u16, data: u16) {
        let lo = (data & 0xff) as u8;
        let hi = (data >> 8) as u8;
        self.mem_write(pos, lo);
        self.mem_write(pos + 1, hi);
    }
}

impl Mem for CPU {
    fn mem_read(&self, addr: u16) -> u8 {
        self.memory[addr as usize]
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        self.memory[addr as usize] = data;
    }
}

impl CPU {
    pub fn new() -> Self {
        CPU {
            register_a: 0,
            register_x: 0,
            register_y: 0,
            flags: CPUFlags::empty(),
            program_counter: 0,
            memory: [0; 0xFFFF],
        }
    }

    fn get_operand_address(&self, mode: &AddressingMode) -> u16 {
        match mode {
            AddressingMode::Immedtiate => self.program_counter,
            
            AddressingMode::ZeroPage => self.mem_read(self.program_counter) as u16,
            AddressingMode::ZeroPage_X => {
                let pos = self.mem_read(self.program_counter);
                let addr = pos.wrapping_add(self.register_x) as u16;
                addr
            },
            AddressingMode::ZeroPage_Y => {
                    let pos = self.mem_read(self.program_counter);
                let addr = pos.wrapping_add(self.register_y) as u16;
                addr
            },
            
            AddressingMode::Absolute => self.mem_read_u16(self.program_counter),
            AddressingMode::Absolute_X => {
                let base = self.mem_read_u16(self.program_counter);
                let addr = base.wrapping_add(self.register_x as u16);
                addr
            },
            AddressingMode::Absolute_Y => {
                let base = self.mem_read_u16(self.program_counter);
                let addr = base.wrapping_add(self.register_y as u16);
                addr
            },
            
            AddressingMode::Indirect_X => {
                let base = self.mem_read(self.program_counter);

                let ptr = base.wrapping_add(self.register_x);
                let lo = self.mem_read(ptr as u16);
                let hi = self.mem_read(ptr.wrapping_add(1) as u16);
                (hi as u16) << 8 | (lo as u16)
            },
            AddressingMode::Indirect_Y => {
                let base = self.mem_read(self.program_counter);

                let lo = self.mem_read(base as u16);
                let hi = self.mem_read(base.wrapping_add(1) as u16);
                let deref_base = (hi as u16) << 8 | (lo as u16);
                let deref = deref_base.wrapping_add(self.register_y as u16);
                deref
            },

            AddressingMode::NoneAddressing => panic!("mode {:?} is not supported", mode),
        }
    }

    fn lda(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn tax(&mut self) {
        self.register_x = self.register_a;
        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn inx(&mut self) {
        self.register_x = self.register_x.wrapping_add(1);
        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn sta(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_a);
    }

    // The zero flag is at the least significant bit
    fn update_zero_flag(&mut self, result: u8) {
        if result == 0 {
            self.flags.insert(CPUFlags::ZERO);
        } else {
            self.flags.remove(CPUFlags::ZERO);
        }
    }

    // The negative flag is at the most significant bit
    fn update_negative_flag(&mut self, result: u8) {
        if result & 0b1000_0000 != 0 {
            self.flags.insert(CPUFlags::NEGATIVE);
        } else {
            self.flags.remove(CPUFlags::NEGATIVE)
        }
    }

    pub fn reset(&mut self) {
        self.register_a = 0;
        self.register_x = 0;
        self.register_y = 0;
        self.flags = CPUFlags::empty();

        self.program_counter = self.mem_read_u16(0xFFFC);
    }

    pub fn load_and_run(&mut self, program: Vec<u8>) {
        self.load(program);
        self.reset();
        self.run();
    }

    pub fn load(&mut self, program: Vec<u8>) {
        // [0x8000 .. 0xFFFF] is reserved for Program ROM
        self.memory[0x8000 .. (0x8000 + program.len())].copy_from_slice(&program[..]);
        self.mem_write_u16(0xFFFC, 0x8000);
    }

    // Fetches the next instruction, decodes it and executies it.
    pub fn run(&mut self) {
        let ref opcodes: HashMap<u8, opcodes::OpCode> = *opcodes::OP_CODES;

        loop {
            let code = self.mem_read(self.program_counter);
            self.program_counter += 1;
            let pc = self.program_counter;

            let opcode = opcodes.get(&code).expect(&format!("Invalid OpCode {:x}", code));

            match code {
                // LDA
                0xA9 | 0xA5| 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => self.lda(&opcode.mode),

                // STA
                0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => self.sta(&opcode.mode),

                //TAX
                0xAA => self.tax(),

                //INX
                0xE8 => self.inx(),

                //BRK
                0x00 => return,

                _ => todo!(),
            }

            if pc == self.program_counter {
                self.program_counter += (opcode.bytes - 1) as u16
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_0xa9_lda_load_data() {
        let mut cpu = CPU::new();
        // LDA $05, BRK
        let program = vec![0xa9, 0x05, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x05);
        assert!(!cpu.flags.contains(CPUFlags::ZERO));
        assert!(!cpu.flags.contains(CPUFlags::NEGATIVE));
    }

     #[test]
    fn test_0xa9_lda_zero_flag() {
        let mut cpu = CPU::new();
        // LDA $00, BRK
        let program = vec![0xa9, 0x00, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::ZERO));
    }

    #[test]
    fn test_lda_from_memory() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0x55);
        // LDA $10, BRK
        let program = vec![0xa5, 0x10, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x55);
    }

    #[test]
    fn test_inx_overflow() {
        let mut cpu = CPU::new();
        // LDA $FF, TAX, INX, INX, BRK
        let program = vec![0xa9, 0xff, 0xaa, 0xe8, 0xe8, 0x00];

        cpu.load_and_run(program);
        
        assert_eq!(cpu.register_x, 1);
    }
    
    #[test]
    fn test_lda_tax_inx_brk() {
        let mut cpu = CPU::new();
        // LDA $C0, TAX, INX, BRK
        let program = vec![0xa9, 0xc0, 0xaa, 0xe8, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0xc1);
    }

    #[test]
    fn test_sta_zero_page() {
        let mut cpu = CPU::new();
        // LDA $42, STA $10, BRK
        let program = vec![0xa9, 0x42, 0x85, 0x10, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x10), 0x42);
    }

    #[test]
    fn test_sta_absolute() {
        let mut cpu = CPU::new();
        // LDA $55, STA $2000, BRK
        let program = vec![0xa9, 0x55, 0x8d, 0x00, 0x20, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x2000), 0x55);
    }
}