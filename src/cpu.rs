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
        const BREAK2    = 0b00100000;
        const OVERFLOW  = 0b01000000;
        const NEGATIVE  = 0b10000000;
    }
}

const STACK: u16 = 0x0100;
const STACK_RESET: u8 = 0xFD;

pub struct CPU {
    pub register_a: u8,
    pub register_x: u8,
    pub register_y: u8,
    pub flags: CPUFlags,
    pub program_counter: u16,
    pub stack_ptr: u8,
    memory: [u8; 0xFFFF],
}

#[derive(Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub enum AddressingMode {
    Implied,
    Accumulator,
    Immedtiate,
    ZeroPage,
    ZeroPage_X,
    ZeroPage_Y,
    Relative,
    Absolute,
    Absolute_X,
    Absolute_Y,
    Indirect,
    Indirect_X,
    Indirect_Y,
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
        let lo = (data & 0xFF) as u8;
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

trait Stack {
    fn stack_push(&mut self, data: u8);
    fn stack_pop(&mut self) -> u8;

    fn stack_push_u16(&mut self, data: u16) {
        let lo = (data & 0xFF) as u8;
        let hi = (data >> 8) as u8;
        self.stack_push(hi);
        self.stack_push(lo);
    }
    fn stack_pop_u16(&mut self) -> u16 {
        let lo = self.stack_pop() as u16;
        let hi = self.stack_pop() as u16;
        (hi << 8) | lo
    }
}

impl Stack for CPU {
    fn stack_push(&mut self, data: u8) {
        self.mem_write(STACK as u16 + self.stack_ptr as u16, data);
        self.stack_ptr = self.stack_ptr.wrapping_sub(1);
    }

    fn stack_pop(&mut self) -> u8 {
        self.stack_ptr = self.stack_ptr.wrapping_add(1);
        self.mem_read(STACK as u16 + self.stack_ptr as u16)
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
            stack_ptr: STACK_RESET,
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

            AddressingMode::Relative => {
                let offset = self.mem_read(self.program_counter) as i8;
                let target = self.program_counter.wrapping_add(offset as u16).wrapping_add(1);
                target
            },

            AddressingMode::Indirect => {
                let ptr = self.mem_read_u16(self.program_counter);
                let lo = self.mem_read(ptr);
                let hi = self.mem_read(if ptr & 0x00FF == 0x00FF {
                    ptr & 0xFF00
                } else {
                    ptr.wrapping_add(1)
                });
                (hi as u16) << 8 | (lo as u16)
            },

            AddressingMode::Implied | AddressingMode::Accumulator => {
                panic!("mode {:?} should not be used as an operand address", mode)
            }
        }
    }

    // https://www.righto.com/2012/12/the-6502-overflow-flag-explained.html
    fn adc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let carry = if self.flags.contains(CPUFlags::CARRY) { 1 } else { 0 };
        let sum = self.register_a as u16 + value as u16 + carry as u16;

        if sum > 0xFF {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        let result = sum as u8;

        if (value ^ result) & (self.register_a ^ result) & 0x80 != 0 {
            self.flags.insert(CPUFlags::OVERFLOW);
        } else {
            self.flags.remove(CPUFlags::OVERFLOW);
        }

        self.register_a = result;

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }

    fn and(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = self.register_a & value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn asl(&mut self, mode: &AddressingMode) {
        match mode {
            AddressingMode::Accumulator => {
                let value = self.register_a;
                let result = value << 1;
                self.register_a = result;

                self.set_carry_from_bit7(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            },
            _ => {
                let addr = self.get_operand_address(mode);
                let value = self.mem_read(addr);
                let result = value << 1;
                self.mem_write(addr, result);

                self.set_carry_from_bit7(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            }
        }
    }

    fn bcc(&mut self) {
        if !self.flags.contains(CPUFlags::CARRY) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn bcs(&mut self) {
        if self.flags.contains(CPUFlags::CARRY) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn beq(&mut self) {
        if self.flags.contains(CPUFlags::ZERO) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn bit(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = self.register_a & value;

        self.update_zero_flag(result);
        self.update_overflow_flag(value);
        self.update_negative_flag(value);
    }

    fn bmi(&mut self) {
        if self.flags.contains(CPUFlags::NEGATIVE) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn bne(&mut self) {
        if !self.flags.contains(CPUFlags::ZERO) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn bpl(&mut self) {
        if !self.flags.contains(CPUFlags::NEGATIVE) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn brk(&mut self) {
        self.stack_push_u16(self.program_counter);
        self.stack_push(self.flags.bits());
        // self.program_counter = self.mem_read_u16(0xFFFE);
        self.flags.insert(CPUFlags::BREAK);
    }

    fn bvc(&mut self) {
        if !self.flags.contains(CPUFlags::OVERFLOW) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn bvs(&mut self) {
        if self.flags.contains(CPUFlags::OVERFLOW) {
            self.program_counter = self.get_operand_address(&AddressingMode::Relative);
        }
    }

    fn clc(&mut self) {
        self.flags.remove(CPUFlags::CARRY);
    }

    fn cld(&mut self) {
        self.flags.remove(CPUFlags::DECIMAL);
    }

    fn cli(&mut self) {
        self.flags.remove(CPUFlags::INTERRUPT);
    }

    fn clv(&mut self) {
        self.flags.remove(CPUFlags::OVERFLOW);
    }

    fn cmp(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = self.register_a.wrapping_sub(value);

        if self.register_a >= value {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }

    fn cpx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = self.register_x.wrapping_sub(value);

        if self.register_x >= value {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }

    fn cpy(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = self.register_y.wrapping_sub(value);

        if self.register_y >= value {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }

    fn dec(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = value.wrapping_sub(1);
        self.mem_write(addr, result);

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }

    fn dex(&mut self) {
        self.register_x = self.register_x.wrapping_sub(1);

        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn dey(&mut self) {
        self.register_y = self.register_y.wrapping_sub(1);

        self.update_zero_flag(self.register_y);
        self.update_negative_flag(self.register_y);
    }

    fn eor(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = self.register_a ^ value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn inc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = value.wrapping_add(1);
        self.mem_write(addr, result);

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }
    
    fn inx(&mut self) {
        self.register_x = self.register_x.wrapping_add(1);

        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn iny(&mut self) {
        self.register_y = self.register_y.wrapping_add(1);

        self.update_zero_flag(self.register_y);
        self.update_negative_flag(self.register_y);
    }

    fn jmp(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.program_counter = addr;
    }

    fn jsr(&mut self, mode: &AddressingMode) {
        self.stack_push_u16(self.program_counter + 2 - 1);
        let addr = self.get_operand_address(mode);
        self.program_counter = addr;
    }

    fn lda(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn ldx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_x = value;

        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn ldy(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_y = value;

        self.update_zero_flag(self.register_y);
        self.update_negative_flag(self.register_y);
    }

    fn lsr(&mut self, mode: &AddressingMode) {
        match mode {
            AddressingMode::Accumulator => {
                let value = self.register_a;
                let result = value >> 1;
                self.register_a = result;

                self.set_carry_from_bit0(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            },
            _ => {
                let addr = self.get_operand_address(mode);
                let value = self.mem_read(addr);
                let result = value >> 1;
                self.mem_write(addr, result);

                self.set_carry_from_bit0(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            }
        }
    }

    fn nop(&mut self) {
        return;
    }

    fn ora(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = self.register_a | value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn pha(&mut self) {
        self.stack_push(self.register_a);
    }

    fn php(&mut self) {
        let mut flags = self.flags.clone();
        flags.insert(CPUFlags::BREAK);
        flags.insert(CPUFlags::BREAK2);
        self.stack_push(flags.bits());
    }

    fn pla(&mut self) {
        self.register_a = self.stack_pop();

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn plp(&mut self) {
        let flags = self.stack_pop();
        self.flags = CPUFlags::from_bits_truncate(flags);
    }

    fn rol(&mut self, mode: &AddressingMode) {
        match mode {
            AddressingMode::Accumulator => {
                let value = self.register_a;
                let carry = self.flags.contains(CPUFlags::CARRY);
                let mut result = value << 1;
                
                if carry {
                    result |= 1;
                }

                self.register_a = result;

                self.set_carry_from_bit7(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            },
            _ => {
                let addr = self.get_operand_address(mode);
                let value = self.mem_read(addr);
                let carry = self.flags.contains(CPUFlags::CARRY);
                let mut result = value << 1;
                
                if carry {
                    result |= 1;
                }
                
                self.mem_write(addr, result);

                self.set_carry_from_bit7(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            }
        }
    }

    fn ror(&mut self, mode: &AddressingMode) {
        match mode {
            AddressingMode::Accumulator => {
                let value = self.register_a;
                let carry = self.flags.contains(CPUFlags::CARRY);
                let mut result = value >> 1;
                
                if carry {
                    result |= 0b1000_0000;
                }

                self.register_a = result;

                self.set_carry_from_bit0(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            },
            _ => {
                let addr = self.get_operand_address(mode);
                let value = self.mem_read(addr);
                let carry = self.flags.contains(CPUFlags::CARRY);
                let mut result = value >> 1;
                
                if carry {
                    result |= 0b1000_0000;
                }
                
                self.mem_write(addr, result);

                self.set_carry_from_bit0(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            }
        }
    }

    fn rti(&mut self) {
        self.flags = CPUFlags::from_bits_truncate(self.stack_pop());
        self.flags.remove(CPUFlags::BREAK);
        self.flags.insert(CPUFlags::BREAK2);
        self.program_counter = self.stack_pop_u16();
    }

    fn rts(&mut self) {
        self.program_counter = self.stack_pop_u16() + 1;
    }

    fn sbc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let carry = if self.flags.contains(CPUFlags::CARRY) { 1 } else { 0 };
        let difference = self.register_a as u16 - value as u16 - (1 - carry) as u16;

        if difference > 0 {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        let result = difference as u8;

        if (value ^ result) & (self.register_a ^ result) & 0x80 != 0 {
            self.flags.insert(CPUFlags::OVERFLOW);
        } else {
            self.flags.remove(CPUFlags::OVERFLOW);
        }

        self.register_a = result;

        self.update_zero_flag(result);
        self.update_negative_flag(result);
    }

    fn sec(&mut self) {
        self.flags.insert(CPUFlags::CARRY);
    }

    fn sed(&mut self) {
        self.flags.insert(CPUFlags::DECIMAL);
    }

    fn sei(&mut self) {
        self.flags.insert(CPUFlags::INTERRUPT);
    }

    fn sta(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_a);
    }

    fn stx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_x);
    }

    fn sty(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_y);
    }

    fn tax(&mut self) {
        self.register_x = self.register_a;

        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn tay(&mut self) {
        self.register_y = self.register_a;

        self.update_zero_flag(self.register_y);
        self.update_negative_flag(self.register_y);
    }

    fn tsx(&mut self) {
        self.register_x = self.stack_ptr;

        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);
    }

    fn txa(&mut self) {
        self.register_a = self.register_x;
        
        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn txs(&mut self) {
        self.stack_ptr = self.register_x;
    }

    fn tya(&mut self) {
        self.register_a = self.register_y;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);
    }

    fn set_carry_from_bit7(&mut self, value: u8) {
        if value & 0x80 != 0 {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }
    }

    fn set_carry_from_bit0(&mut self, value: u8) {
        if value & 0x01 != 0 {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }
    }

    fn update_zero_flag(&mut self, result: u8) {
        if result == 0 {
            self.flags.insert(CPUFlags::ZERO);
        } else {
            self.flags.remove(CPUFlags::ZERO);
        }
    }

    fn update_overflow_flag(&mut self, value: u8) {
        if value & 0x40 != 0 {
            self.flags.insert(CPUFlags::OVERFLOW);
        } else {
            self.flags.remove(CPUFlags::OVERFLOW);
        }
    }

    fn update_negative_flag(&mut self, result: u8) {
        if result & 0x80 != 0 {
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
        self.stack_ptr = STACK_RESET;
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
                // ADC
                0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => self.adc(&opcode.mode),

                // AND
                0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => self.and(&opcode.mode),

                // ASL
                0x0A | 0x06 | 0x16 | 0x0E | 0x1E => self.asl(&opcode.mode),

                // BCC
                0x90 => self.bcc(),

                // BCS
                0xB0 => self.bcs(),

                // BEQ
                0xF0 => self.beq(),

                // BIT
                0x24 | 0x2C => self.bit(&opcode.mode),

                // BMI
                0x30 => self.bmi(),

                // BNE
                0xD0 => self.bne(),

                // BPL
                0x10 => self.bpl(),

                // BRK
                0x00 => {
                    self.brk();
                    return;
                }

                // BVC
                0x50 => self.bvc(),

                // BVS
                0x70 => self.bvs(),

                // CLC
                0x18 => self.clc(),

                // CLD
                0xD8 => self.cld(),

                // CLI
                0x58 => self.cli(),

                // CLV
                0xB8 => self.clv(),

                // CMP
                0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => self.cmp(&opcode.mode),

                // CPX
                0xE0 | 0xE4 | 0xEC => self.cpx(&opcode.mode),

                // CPY
                0xC0 | 0xC4 | 0xCC => self.cpy(&opcode.mode),

                // DEC
                0xC6 | 0xD6 | 0xCE | 0xDE => self.dec(&opcode.mode),

                // DEX
                0xCA => self.dex(),

                // DEY
                0x88 => self.dey(),

                // EOR
                0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => self.eor(&opcode.mode),

                // INC
                0xE6 | 0xF6 | 0xEE | 0xFE => self.inc(&opcode.mode),

                // INX
                0xE8 => self.inx(),

                // INY
                0xC8 => self.iny(),

                // JMP
                0x4C | 0x6C => self.jmp(&opcode.mode),

                // JSR
                0x20 => self.jsr(&opcode.mode),

                // LDA
                0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => self.lda(&opcode.mode),

                // LDX
                0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => self.ldx(&opcode.mode),

                // LDY
                0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => self.ldy(&opcode.mode),

                // LSR
                0x4A | 0x46 | 0x56 | 0x4E | 0x5E => self.lsr(&opcode.mode),

                // NOP
                0xEA => self.nop(),

                // ORA
                0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => self.ora(&opcode.mode),

                // PHA
                0x48 => self.pha(),

                // PHP
                0x08 => self.php(),

                // PLA
                0x68 => self.pla(),

                // PLP
                0x28 => self.plp(),

                // ROL
                0x2A | 0x26 | 0x36 | 0x2E | 0x3E => self.rol(&opcode.mode),

                // ROR
                0x6A | 0x66 | 0x76 | 0x6E | 0x7E => self.ror(&opcode.mode),

                // RTI
                0x40 => self.rti(),

                // RTS
                0x60 => self.rts(),

                // SBC
                0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 => self.sbc(&opcode.mode),

                // SEC
                0x38 => self.sec(),

                // SED
                0xF8 => self.sed(),

                // SEI
                0x78 => self.sei(),

                // STA
                0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => self.sta(&opcode.mode),

                // STX
                0x86 | 0x96 | 0x8E => self.stx(&opcode.mode),

                // STY
                0x84 | 0x94 | 0x8C => self.sty(&opcode.mode),

                // TAX
                0xAA => self.tax(),

                // TAY
                0xA8 => self.tay(),

                // TSX
                0xBA => self.tsx(),

                // TXA
                0x8A => self.txa(),

                // TXS
                0x9A => self.txs(),

                // TYA
                0x98 => self.tya(),

                // Unknown
                _ => todo!("Unknown opcode: {:02x}", opcode.code),
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
    fn adc_immediate_adds_with_carry() {
        let mut cpu = CPU::new();
        // LDA #$10, ADC #$20, BRK
        let program = vec![0xa9, 0x10, 0x69, 0x20, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x30);
        assert!(!cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn and_immediate_masks() {
        let mut cpu = CPU::new();
        // LDA #$F0, AND #$0F, BRK
        let program = vec![0xa9, 0xf0, 0x29, 0x0f, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x00);
        assert!(cpu.flags.contains(CPUFlags::ZERO));
    }

    #[test]
    fn asl_accumulator_shifts_left() {
        let mut cpu = CPU::new();
        // LDA #$41, ASL A, BRK
        let program = vec![0xa9, 0x41, 0x0a, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x82);
        assert!(!cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn bcc_branches_when_carry_clear() {
        let mut cpu = CPU::new();
        // CLC, BCC +3, NOP NOP NOP, BRK
        let program = vec![0x18, 0x90, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8007);
    }

    #[test]
    fn bcs_branches_when_carry_set() {
        let mut cpu = CPU::new();
        // SEC, BCS +3, NOP NOP NOP, BRK
        let program = vec![0x38, 0xb0, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8007);
    }

    #[test]
    fn beq_branches_when_zero_set() {
        let mut cpu = CPU::new();
        // LDA #$00, BEQ +3, NOP NOP NOP, BRK
        let program = vec![0xa9, 0x00, 0xf0, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8008);
    }

    #[test]
    fn bit_zero_page_sets_flags_from_memory() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0xc0);
        // LDA #$FF, BIT $10, BRK
        let program = vec![0xa9, 0xff, 0x24, 0x10, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::NEGATIVE));
        assert!(cpu.flags.contains(CPUFlags::OVERFLOW));
    }

    #[test]
    fn bmi_branches_when_negative_set() {
        let mut cpu = CPU::new();
        // LDA #$80, BMI +3, NOP NOP NOP, BRK
        let program = vec![0xa9, 0x80, 0x30, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8008);
    }

    #[test]
    fn bne_branches_when_not_zero() {
        let mut cpu = CPU::new();
        // LDA #$01, BNE +3, NOP NOP NOP, BRK
        let program = vec![0xa9, 0x01, 0xd0, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8008);
    }

    #[test]
    fn bpl_branches_when_positive() {
        let mut cpu = CPU::new();
        // LDA #$01, BPL +3, NOP NOP NOP, BRK
        let program = vec![0xa9, 0x01, 0x10, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8008);
    }

    #[test]
    fn brk_stops_execution() {
        let mut cpu = CPU::new();
        // NOP, BRK
        let program = vec![0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8002);
    }

    #[test]
    fn bvc_branches_when_overflow_clear() {
        let mut cpu = CPU::new();
        // BVC +3, NOP NOP NOP, BRK
        let program = vec![0x50, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8006);
    }

    #[test]
    fn bvs_branches_when_overflow_set() {
        let mut cpu = CPU::new();
        // LDA #$40, ADC #$40, BVS +3, NOP NOP NOP, BRK
        let program = vec![0xa9, 0x40, 0x69, 0x40, 0x70, 0x03, 0xea, 0xea, 0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x800a);
    }

    #[test]
    fn clc_clears_carry() {
        let mut cpu = CPU::new();
        // SEC, CLC, BRK
        let program = vec![0x38, 0x18, 0x00];

        cpu.load_and_run(program);

        assert!(!cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn cld_clears_decimal() {
        let mut cpu = CPU::new();
        // SED, CLD, BRK
        let program = vec![0xf8, 0xd8, 0x00];

        cpu.load_and_run(program);

        assert!(!cpu.flags.contains(CPUFlags::DECIMAL));
    }

    #[test]
    fn cli_clears_interrupt_disable() {
        let mut cpu = CPU::new();
        // SEI, CLI, BRK
        let program = vec![0x78, 0x58, 0x00];

        cpu.load_and_run(program);

        assert!(!cpu.flags.contains(CPUFlags::INTERRUPT));
    }

    #[test]
    fn clv_clears_overflow() {
        let mut cpu = CPU::new();
        // LDA #$40, ADC #$40, CLV, BRK
        let program = vec![0xa9, 0x40, 0x69, 0x40, 0xb8, 0x00];

        cpu.load_and_run(program);

        assert!(!cpu.flags.contains(CPUFlags::OVERFLOW));
    }

    #[test]
    fn cmp_sets_zero_when_equal() {
        let mut cpu = CPU::new();
        // LDA #$05, CMP #$05, BRK
        let program = vec![0xa9, 0x05, 0xc9, 0x05, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::ZERO));
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn cpx_sets_carry_when_x_gte() {
        let mut cpu = CPU::new();
        // LDX #$0A, CPX #$05, BRK
        let program = vec![0xa2, 0x0a, 0xe0, 0x05, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn cpy_sets_carry_when_y_gte() {
        let mut cpu = CPU::new();
        // LDY #$0A, CPY #$05, BRK
        let program = vec![0xa0, 0x0a, 0xc0, 0x05, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn dec_decrements_memory() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0x02);
        // DEC $10, BRK
        let program = vec![0xc6, 0x10, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x10), 0x01);
    }

    #[test]
    fn dex_decrements_x() {
        let mut cpu = CPU::new();
        // LDX #$02, DEX, BRK
        let program = vec![0xa2, 0x02, 0xca, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0x01);
    }

    #[test]
    fn dey_decrements_y() {
        let mut cpu = CPU::new();
        // LDY #$02, DEY, BRK
        let program = vec![0xa0, 0x02, 0x88, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_y, 0x01);
    }

    #[test]
    fn eor_immediate_xors() {
        let mut cpu = CPU::new();
        // LDA #$AA, EOR #$55, BRK
        let program = vec![0xa9, 0xaa, 0x49, 0x55, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0xff);
    }

    #[test]
    fn inc_increments_memory() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0xfe);
        // INC $10, BRK
        let program = vec![0xe6, 0x10, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x10), 0xff);
    }

    #[test]
    fn inx_increments_x() {
        let mut cpu = CPU::new();
        // LDX #$3F, INX, BRK
        let program = vec![0xa2, 0x3f, 0xe8, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0x40);
    }

    #[test]
    fn iny_increments_y() {
        let mut cpu = CPU::new();
        // LDY #$3F, INY, BRK
        let program = vec![0xa0, 0x3f, 0xc8, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_y, 0x40);
    }

    #[test]
    fn jmp_absolute() {
        let mut cpu = CPU::new();
        // JMP $8008, padding, LDA #$42, BRK
        let program = vec![
            0x4c, 0x08, 0x80, 0xea, 0xea, 0xea, 0xea, 0xea, 0xa9, 0x42, 0x00,
        ];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x42);
    }

    #[test]
    fn jmp_indirect() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x30, 0x08);
        cpu.mem_write(0x31, 0x80);
        // JMP ($0030), padding, LDA #$42, BRK
        let program = vec![
            0x6c, 0x30, 0x00, 0xea, 0xea, 0xea, 0xea, 0xea, 0xa9, 0x42, 0x00,
        ];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x42);
    }

    #[test]
    fn jsr_rts_returns() {
        let mut cpu = CPU::new();
        // JSR $8009, LDA #$11, BRK, padding, RTS
        let program = vec![0x20, 0x09, 0x80, 0xa9, 0x11, 0x00, 0xea, 0xea, 0xea, 0x60];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x11);
    }

    #[test]
    fn lda_immediate_loads_a() {
        let mut cpu = CPU::new();
        // LDA #$05, BRK
        let program = vec![0xa9, 0x05, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x05);
        assert!(!cpu.flags.contains(CPUFlags::ZERO));
        assert!(!cpu.flags.contains(CPUFlags::NEGATIVE));
    }

    #[test]
    fn lda_zero_sets_zero_flag() {
        let mut cpu = CPU::new();
        // LDA #$00, BRK
        let program = vec![0xa9, 0x00, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::ZERO));
    }

    #[test]
    fn lda_zero_page() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0x55);
        // LDA $10, BRK
        let program = vec![0xa5, 0x10, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x55);
    }

    #[test]
    fn ldx_immediate() {
        let mut cpu = CPU::new();
        // LDX #$77, BRK
        let program = vec![0xa2, 0x77, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0x77);
    }

    #[test]
    fn ldy_immediate() {
        let mut cpu = CPU::new();
        // LDY #$88, BRK
        let program = vec![0xa0, 0x88, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_y, 0x88);
    }

    #[test]
    fn lsr_accumulator_shifts_right() {
        let mut cpu = CPU::new();
        // LDA #$03, LSR A, BRK
        let program = vec![0xa9, 0x03, 0x4a, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x01);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn nop_advances_pc() {
        let mut cpu = CPU::new();
        // NOP, BRK
        let program = vec![0xea, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.program_counter, 0x8002);
    }

    #[test]
    fn ora_immediate_ors() {
        let mut cpu = CPU::new();
        // LDA #$0F, ORA #$F0, BRK
        let program = vec![0xa9, 0x0f, 0x09, 0xf0, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0xff);
    }

    #[test]
    fn pha_pla_round_trip() {
        let mut cpu = CPU::new();
        // LDA #$37, PHA, LDA #$00, PLA, BRK
        let program = vec![0xa9, 0x37, 0x48, 0xa9, 0x00, 0x68, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x37);
    }

    #[test]
    fn php_plp_round_trip() {
        let mut cpu = CPU::new();
        // SEC, PHP, CLD, PLP, BRK
        let program = vec![0x38, 0x08, 0xd8, 0x28, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::CARRY));
        assert!(!cpu.flags.contains(CPUFlags::DECIMAL));
    }

    #[test]
    fn rol_rotates_left_through_carry() {
        let mut cpu = CPU::new();
        // SEC, LDA #$80, ROL A, BRK
        let program = vec![0x38, 0xa9, 0x80, 0x2a, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x01);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn ror_rotates_right_through_carry() {
        let mut cpu = CPU::new();
        // SEC, LDA #$01, ROR A, BRK
        let program = vec![0x38, 0xa9, 0x01, 0x6a, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x80);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn rti_restores_pc_and_flags() {
        let mut cpu = CPU::new();
        cpu.load(vec![0x40, 0xea, 0x00]);
        cpu.reset();
        cpu.mem_write(0x01fb, 0x34);
        cpu.mem_write(0x01fc, 0x02);
        cpu.mem_write(0x01fd, 0x80);
        cpu.stack_ptr = 0xfa;
        cpu.run();

        assert_eq!(cpu.program_counter, 0x8003);
    }

    #[test]
    fn rts_returns_from_subroutine() {
        let mut cpu = CPU::new();
        // JSR $8006, LDA #$99, BRK, LDA #$55, RTS
        let program = vec![0x20, 0x06, 0x80, 0xa9, 0x99, 0x00, 0xa9, 0x55, 0x60, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x99);
    }

    #[test]
    fn sbc_immediate_subtracts() {
        let mut cpu = CPU::new();
        // SEC, LDA #$05, SBC #$03, BRK
        let program = vec![0x38, 0xa9, 0x05, 0xe9, 0x03, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0x02);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn sec_sets_carry() {
        let mut cpu = CPU::new();
        // SEC, BRK
        let program = vec![0x38, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn sed_sets_decimal() {
        let mut cpu = CPU::new();
        // SED, BRK
        let program = vec![0xf8, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::DECIMAL));
    }

    #[test]
    fn sei_sets_interrupt_disable() {
        let mut cpu = CPU::new();
        // SEI, BRK
        let program = vec![0x78, 0x00];

        cpu.load_and_run(program);

        assert!(cpu.flags.contains(CPUFlags::INTERRUPT));
    }

    #[test]
    fn sta_zero_page() {
        let mut cpu = CPU::new();
        // LDA #$42, STA $10, BRK
        let program = vec![0xa9, 0x42, 0x85, 0x10, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x10), 0x42);
    }

    #[test]
    fn sta_absolute() {
        let mut cpu = CPU::new();
        // LDA #$55, STA $2000, BRK
        let program = vec![0xa9, 0x55, 0x8d, 0x00, 0x20, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x2000), 0x55);
    }

    #[test]
    fn stx_zero_page() {
        let mut cpu = CPU::new();
        // LDX #$33, STX $20, BRK
        let program = vec![0xa2, 0x33, 0x86, 0x20, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x20), 0x33);
    }

    #[test]
    fn sty_zero_page() {
        let mut cpu = CPU::new();
        // LDY #$44, STY $21, BRK
        let program = vec![0xa0, 0x44, 0x84, 0x21, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.mem_read(0x21), 0x44);
    }

    #[test]
    fn tax_copies_a_to_x() {
        let mut cpu = CPU::new();
        // LDA #$AB, TAX, BRK
        let program = vec![0xa9, 0xab, 0xaa, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0xab);
    }

    #[test]
    fn tay_copies_a_to_y() {
        let mut cpu = CPU::new();
        // LDA #$BC, TAY, BRK
        let program = vec![0xa9, 0xbc, 0xa8, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_y, 0xbc);
    }

    #[test]
    fn tsx_loads_stack_ptr_into_x() {
        let mut cpu = CPU::new();
        // TSX, BRK
        let program = vec![0xba, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, STACK_RESET);
    }

    #[test]
    fn txa_copies_x_to_a() {
        let mut cpu = CPU::new();
        // LDX #$CD, TXA, BRK
        let program = vec![0xa2, 0xcd, 0x8a, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0xcd);
    }

    #[test]
    fn txs_sets_stack_ptr() {
        let mut cpu = CPU::new();
        // LDX #$FA, TXS, TSX, BRK
        let program = vec![0xa2, 0xfa, 0x9a, 0xba, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0xfa);
    }

    #[test]
    fn tya_copies_y_to_a() {
        let mut cpu = CPU::new();
        // LDY #$DE, TYA, BRK
        let program = vec![0xa0, 0xde, 0x98, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_a, 0xde);
    }

    #[test]
    fn inx_overflow_wraps() {
        let mut cpu = CPU::new();
        // LDA #$FF, TAX, INX, INX, BRK
        let program = vec![0xa9, 0xff, 0xaa, 0xe8, 0xe8, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 1);
    }

    #[test]
    fn lda_tax_inx_sequence() {
        let mut cpu = CPU::new();
        // LDA #$C0, TAX, INX, BRK
        let program = vec![0xa9, 0xc0, 0xaa, 0xe8, 0x00];

        cpu.load_and_run(program);

        assert_eq!(cpu.register_x, 0xc1);
    }
}