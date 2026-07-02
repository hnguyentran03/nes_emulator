use crate::bus::Bus;
use crate::opcodes;

use std::collections::HashMap;

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
const RESET_VECTOR: u16 = 0xFFFC;
const NMI_INTERRUPT_VECTOR: u16 = 0xFFFA;
const IRQ_BRK_VECTOR: u16 = 0xFFFE;
pub const PROGRAM_START: u16 = 0x8000;

// Shared description of a 6502 interrupt sequence
struct Interrupt {
    vector: u16,
    flag_mask: u8,
    cycles: u8,
}

const NMI: Interrupt = Interrupt {
    vector: NMI_INTERRUPT_VECTOR,
    flag_mask: 0b0010_0000,
    cycles: 7,
};

const IRQ: Interrupt = Interrupt {
    vector: IRQ_BRK_VECTOR,
    flag_mask: 0b0010_0000,
    cycles: 7,
};

pub struct CPU {
    pub register_a: u8,
    pub register_x: u8,
    pub register_y: u8,
    pub flags: CPUFlags,
    pub program_counter: u16,
    pub stack_ptr: u8,
    pub bus: Bus,
}

#[derive(Debug, Clone, Copy)]
#[allow(non_camel_case_types)]
pub enum AddressingMode {
    Implied,
    Accumulator,
    Immediate,
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

fn page_cross(addr1: u16, addr2: u16) -> bool {
    addr1 & 0xFF00 != addr2 & 0xFF00
}

pub trait Mem {
    fn mem_read(&mut self, addr: u16) -> u8;

    fn mem_write(&mut self, addr: u16, data: u8);

    fn mem_read_u16(&mut self, pos: u16) -> u16 {
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
    fn mem_read(&mut self, addr: u16) -> u8 {
        self.bus.mem_read(addr)
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        self.bus.mem_write(addr, data);
    }

    fn mem_read_u16(&mut self, pos: u16) -> u16 {
        self.bus.mem_read_u16(pos)
    }

    fn mem_write_u16(&mut self, pos: u16, data: u16) {
        self.bus.mem_write_u16(pos, data);
    }
}

pub trait Stack {
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
        self.mem_write(STACK + self.stack_ptr as u16, data);
        self.stack_ptr = self.stack_ptr.wrapping_sub(1);
    }

    fn stack_pop(&mut self) -> u8 {
        self.stack_ptr = self.stack_ptr.wrapping_add(1);
        self.mem_read(STACK + self.stack_ptr as u16)
    }
}

impl CPU {
    pub fn new(bus: Bus) -> Self {
        CPU {
            register_a: 0,
            register_x: 0,
            register_y: 0,
            flags: CPUFlags::from_bits_truncate(0b100100),
            program_counter: PROGRAM_START,
            stack_ptr: STACK_RESET,
            bus,
        }
    }

    /*
        Implementation of 6502 Instructions
        Reference: https://www.masswerk.at/6502/6502_instruction_set.html
    */

    // Addressing Modes help determine the operand address for the instruction
    pub fn get_absolute_address(&mut self, mode: &AddressingMode, addr: u16) -> (u16, bool) {
        match mode {
            AddressingMode::ZeroPage => (self.mem_read(addr) as u16, false),
            AddressingMode::ZeroPage_X => {
                let pos = self.mem_read(addr);
                let addr = pos.wrapping_add(self.register_x) as u16;
                (addr, false)
            }
            AddressingMode::ZeroPage_Y => {
                let pos = self.mem_read(addr);
                let addr = pos.wrapping_add(self.register_y) as u16;
                (addr, false)
            }

            AddressingMode::Absolute => (self.mem_read_u16(addr), false),
            AddressingMode::Absolute_X => {
                let base = self.mem_read_u16(addr);
                let addr = base.wrapping_add(self.register_x as u16);
                (addr, page_cross(base, addr))
            }
            AddressingMode::Absolute_Y => {
                let base = self.mem_read_u16(addr);
                let addr = base.wrapping_add(self.register_y as u16);
                (addr, page_cross(base, addr))
            }

            AddressingMode::Indirect_X => {
                let base = self.mem_read(addr);

                let ptr = base.wrapping_add(self.register_x);
                let lo = self.mem_read(ptr as u16);
                let hi = self.mem_read(ptr.wrapping_add(1) as u16);
                ((hi as u16) << 8 | (lo as u16), false)
            }
            AddressingMode::Indirect_Y => {
                let base = self.mem_read(addr);

                let lo = self.mem_read(base as u16);
                let hi = self.mem_read(base.wrapping_add(1) as u16);
                let deref_base = (hi as u16) << 8 | (lo as u16);
                let deref = deref_base.wrapping_add(self.register_y as u16);
                (deref, page_cross(deref, deref_base))
            }

            AddressingMode::Relative => {
                let offset = self.mem_read(addr) as i8;
                let result = self
                    .program_counter
                    .wrapping_add(offset as u16)
                    .wrapping_add(1);
                (result, page_cross(self.program_counter, result))
            }

            AddressingMode::Indirect => {
                let ptr = self.mem_read_u16(addr);
                let lo = self.mem_read(ptr);
                let hi = self.mem_read(if ptr & 0xFF == 0xFF {
                    ptr & 0xFF00
                } else {
                    ptr.wrapping_add(1)
                });
                ((hi as u16) << 8 | (lo as u16), false)
            }

            AddressingMode::Implied | AddressingMode::Accumulator => {
                panic!("mode {:?} should not be used as an operand address", mode)
            }
            _ => panic!("mode {:?} not supported", mode),
        }
    }

    fn get_operand_address(&mut self, mode: &AddressingMode) -> (u16, bool) {
        match mode {
            AddressingMode::Immediate => (self.program_counter, false),

            _ => self.get_absolute_address(mode, self.program_counter),
        }
    }

    // Math behind ADC: https://www.righto.com/2012/12/the-6502-overflow-flag-explained.html
    fn adc(&mut self, mode: &AddressingMode) {
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let carry = if self.flags.contains(CPUFlags::CARRY) {
            1
        } else {
            0
        };
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

        if page_crossed {
            self.bus.tick(1);
        }
    }

    fn and(&mut self, mode: &AddressingMode) {
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a &= value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);

        if page_crossed {
            self.bus.tick(1);
        }
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
            }
            _ => {
                let (addr, _) = self.get_operand_address(mode);
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
        self.branch(!self.flags.contains(CPUFlags::CARRY));
    }

    fn bcs(&mut self) {
        self.branch(self.flags.contains(CPUFlags::CARRY));
    }

    fn beq(&mut self) {
        self.branch(self.flags.contains(CPUFlags::ZERO));
    }

    fn bit(&mut self, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = self.register_a & value;

        self.update_zero_flag(result);
        self.update_overflow_flag(value);
        self.update_negative_flag(value);
    }

    fn bmi(&mut self) {
        self.branch(self.flags.contains(CPUFlags::NEGATIVE));
    }

    fn bne(&mut self) {
        self.branch(!self.flags.contains(CPUFlags::ZERO));
    }

    fn bpl(&mut self) {
        self.branch(!self.flags.contains(CPUFlags::NEGATIVE));
    }

    fn brk(&mut self) {
        self.stack_push_u16(self.program_counter);
        self.stack_push(self.flags.bits());
        // self.program_counter = self.mem_read_u16(INTERRUPT_VECTOR);
        self.flags.insert(CPUFlags::BREAK);
    }

    fn bvc(&mut self) {
        self.branch(!self.flags.contains(CPUFlags::OVERFLOW));
    }

    fn bvs(&mut self) {
        self.branch(self.flags.contains(CPUFlags::OVERFLOW));
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
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = self.register_a.wrapping_sub(value);

        if self.register_a >= value {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        self.update_zero_flag(result);
        self.update_negative_flag(result);

        if page_crossed {
            self.bus.tick(1);
        }
    }

    fn cpx(&mut self, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(mode);
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
        let (addr, _) = self.get_operand_address(mode);
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
        let (addr, _) = self.get_operand_address(mode);
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
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a ^= value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);

        if page_crossed {
            self.bus.tick(1);
        }
    }

    fn inc(&mut self, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(mode);
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
        let (addr, _) = self.get_operand_address(mode);
        self.program_counter = addr;
    }

    fn jsr(&mut self, mode: &AddressingMode) {
        self.stack_push_u16(self.program_counter + 2 - 1);
        let (addr, _) = self.get_operand_address(mode);
        self.program_counter = addr;
    }

    fn lda(&mut self, mode: &AddressingMode) {
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);

        if page_crossed {
            self.bus.tick(1);
        }
    }

    fn ldx(&mut self, mode: &AddressingMode) {
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_x = value;

        self.update_zero_flag(self.register_x);
        self.update_negative_flag(self.register_x);

        if page_crossed {
            self.bus.tick(1);
        }
    }

    fn ldy(&mut self, mode: &AddressingMode) {
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_y = value;

        self.update_zero_flag(self.register_y);
        self.update_negative_flag(self.register_y);

        if page_crossed {
            self.bus.tick(1);
        }
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
            }
            _ => {
                let (addr, _) = self.get_operand_address(mode);
                let value = self.mem_read(addr);
                let result = value >> 1;
                self.mem_write(addr, result);

                self.set_carry_from_bit0(value);
                self.update_zero_flag(result);
                self.update_negative_flag(result);
            }
        }
    }

    fn nop(&mut self) {}

    fn ora(&mut self, mode: &AddressingMode) {
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a |= value;

        self.update_zero_flag(self.register_a);
        self.update_negative_flag(self.register_a);

        if page_crossed {
            self.bus.tick(1);
        }
    }

    fn pha(&mut self) {
        self.stack_push(self.register_a);
    }

    fn php(&mut self) {
        let mut flags = self.flags;
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
            }
            _ => {
                let (addr, _) = self.get_operand_address(mode);
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
            }
            _ => {
                let (addr, _) = self.get_operand_address(mode);
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
        let (addr, page_crossed) = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let carry = if self.flags.contains(CPUFlags::CARRY) {
            1
        } else {
            0
        };
        let difference = self.register_a.wrapping_sub(value).wrapping_sub(1 - carry);

        if (self.register_a as u16) >= (value as u16).wrapping_add((1 - carry) as u16) {
            self.flags.insert(CPUFlags::CARRY);
        } else {
            self.flags.remove(CPUFlags::CARRY);
        }

        let result = difference;

        if (value ^ result) & (self.register_a ^ result) & 0x80 != 0 {
            self.flags.insert(CPUFlags::OVERFLOW);
        } else {
            self.flags.remove(CPUFlags::OVERFLOW);
        }

        self.register_a = result;

        self.update_zero_flag(result);
        self.update_negative_flag(result);

        if page_crossed {
            self.bus.tick(1);
        }
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
        let (addr, _) = self.get_operand_address(mode);
        self.mem_write(addr, self.register_a);
    }

    fn stx(&mut self, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(mode);
        self.mem_write(addr, self.register_x);
    }

    fn sty(&mut self, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(mode);
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

    fn branch(&mut self, condition: bool) {
        if condition {
            self.bus.tick(1);

            let (jump_addr, page_crossed) = self.get_operand_address(&AddressingMode::Relative);

            if page_crossed {
                self.bus.tick(1);
            }

            self.program_counter = jump_addr;
        }
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

    /*
        Logic for Running the CPU
    */

    pub fn reset(&mut self) {
        self.register_a = 0;
        self.register_x = 0;
        self.register_y = 0;
        self.flags = CPUFlags::from_bits_truncate(0b100100);
        self.stack_ptr = STACK_RESET;
        self.program_counter = self.mem_read_u16(RESET_VECTOR);
    }

    pub fn load(&mut self, program: Vec<u8>) {
        for i in 0..program.len() as u16 {
            self.mem_write(PROGRAM_START + i, program[i as usize]);
        }
        // self.mem_write_u16(RESET_VECTOR, PROGRAM_START);
    }

    fn interrupt(&mut self, interrupt: &Interrupt) {
        self.stack_push_u16(self.program_counter);

        let mut flags = self.flags;
        flags.set(CPUFlags::BREAK, interrupt.flag_mask & CPUFlags::BREAK.bits() != 0);
        flags.set(CPUFlags::BREAK2, interrupt.flag_mask & CPUFlags::BREAK2.bits() != 0);
        self.stack_push(flags.bits());

        self.flags.insert(CPUFlags::INTERRUPT);
        self.bus.tick(interrupt.cycles as usize);
        self.program_counter = self.mem_read_u16(interrupt.vector);
    }

    pub fn run(&mut self) {
        self.run_with_callback(|_| {});
    }

    // Fetches the next instruction, decodes it and executies it.
    pub fn run_with_callback<F: FnMut(&mut CPU)>(&mut self, mut callback: F) {
        let opcodes: &HashMap<u8, opcodes::OpCode> = &opcodes::OP_CODES;

        loop {
            // NMI takes priority over IRQ
            if self.bus.poll_nmi_interupted().is_some() {
                self.interrupt(&NMI);
            } else if self.bus.poll_irq() && !self.flags.contains(CPUFlags::INTERRUPT) {
                self.interrupt(&IRQ);
            }

            callback(self);

            let code = self.mem_read(self.program_counter);
            self.program_counter += 1;
            let pc = self.program_counter;

            let opcode = opcodes
                .get(&code)
                .unwrap_or_else(|| panic!("Invalid OpCode {:x}", code));

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

            self.bus.tick(opcode.cycles.into());

            if pc == self.program_counter {
                self.program_counter += (opcode.bytes - 1) as u16
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bus::Bus;
    use crate::rom::test::build_test_rom;
    use crate::rom::{Rom, CHR_ROM_PAGE_SIZE, PRG_ROM_PAGE_SIZE};

    #[test]
    fn adc_immediate_adds_with_carry() {
        // LDA #$10, ADC #$20, BRK
        let program = vec![0xA9, 0x10, 0x69, 0x20, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x30);
        assert!(!cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn and_immediate_masks() {
        // LDA #$F0, AND #$0F, BRK
        let program = vec![0xA9, 0xF0, 0x29, 0x0F, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x00);
        assert!(cpu.flags.contains(CPUFlags::ZERO));
    }

    #[test]
    fn asl_accumulator_shifts_left() {
        // LDA #$41, ASL A, BRK
        let program = vec![0xA9, 0x41, 0x0A, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x82);
        assert!(!cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn bcc_branches_when_carry_clear() {
        // CLC, BCC +3, NOP NOP NOP, BRK
        let program = vec![0x18, 0x90, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 7);
    }

    #[test]
    fn bcs_branches_when_carry_set() {
        // SEC, BCS +3, NOP NOP NOP, BRK
        let program = vec![0x38, 0xB0, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 7);
    }

    #[test]
    fn beq_branches_when_zero_set() {
        // LDA #$00, BEQ +3, NOP NOP NOP, BRK
        let program = vec![0xA9, 0x00, 0xF0, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 8);
    }

    #[test]
    fn bit_zero_page_sets_flags_from_memory() {
        // LDA #$FF, BIT $F0, BRK
        let program = vec![0xA9, 0xFF, 0x24, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        cpu.mem_write(0xF0, 0xC0);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::NEGATIVE));
        assert!(cpu.flags.contains(CPUFlags::OVERFLOW));
    }

    #[test]
    fn bmi_branches_when_negative_set() {
        // LDA #$80, BMI +3, NOP NOP NOP, BRK
        let program = vec![0xA9, 0x80, 0x30, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 8);
    }

    #[test]
    fn bne_branches_when_not_zero() {
        // LDA #$01, BNE +3, NOP NOP NOP, BRK
        let program = vec![0xA9, 0x01, 0xD0, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 8);
    }

    #[test]
    fn bpl_branches_when_positive() {
        // LDA #$01, BPL +3, NOP NOP NOP, BRK
        let program = vec![0xA9, 0x01, 0x10, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 8);
    }

    #[test]
    fn brk_stops_execution() {
        // NOP, BRK
        let program = vec![0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 2);
    }

    #[test]
    fn bvc_branches_when_overflow_clear() {
        // BVC +3, NOP NOP NOP, BRK
        let program = vec![0x50, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 6);
    }

    #[test]
    fn bvs_branches_when_overflow_set() {
        // LDA #$40, ADC #$40, BVS +3, NOP NOP NOP, BRK
        let program = vec![0xA9, 0x40, 0x69, 0x40, 0x70, 0x03, 0xEA, 0xEA, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 10);
    }

    #[test]
    fn clc_clears_carry() {
        // SEC, CLC, BRK
        let program = vec![0x38, 0x18, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(!cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn cld_clears_decimal() {
        // SED, CLD, BRK
        let program = vec![0xF8, 0xD8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(!cpu.flags.contains(CPUFlags::DECIMAL));
    }

    #[test]
    fn cli_clears_interrupt_disable() {
        // SEI, CLI, BRK
        let program = vec![0x78, 0x58, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(!cpu.flags.contains(CPUFlags::INTERRUPT));
    }

    #[test]
    fn clv_clears_overflow() {
        // LDA #$40, ADC #$40, CLV, BRK
        let program = vec![0xA9, 0x40, 0x69, 0x40, 0xB8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(!cpu.flags.contains(CPUFlags::OVERFLOW));
    }

    #[test]
    fn cmp_sets_zero_when_equal() {
        // LDA #$05, CMP #$05, BRK
        let program = vec![0xA9, 0x05, 0xC9, 0x05, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::ZERO));
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn cpx_sets_carry_when_x_gte() {
        // LDX #$0A, CPX #$05, BRK
        let program = vec![0xA2, 0x0A, 0xE0, 0x05, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn cpy_sets_carry_when_y_gte() {
        // LDY #$0A, CPY #$05, BRK
        let program = vec![0xA0, 0x0A, 0xC0, 0x05, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn dec_decrements_memory() {
        // DEC $F0, BRK
        let program = vec![0xC6, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        cpu.mem_write(0xF0, 0x02);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.mem_read(0xF0), 0x01);
    }

    #[test]
    fn dex_decrements_x() {
        // LDX #$02, DEX, BRK
        let program = vec![0xA2, 0x02, 0xCA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 0x01);
    }

    #[test]
    fn dey_decrements_y() {
        // LDY #$02, DEY, BRK
        let program = vec![0xA0, 0x02, 0x88, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_y, 0x01);
    }

    #[test]
    fn eor_immediate_xors() {
        // LDA #$AA, EOR #$55, BRK
        let program = vec![0xA9, 0xAA, 0x49, 0x55, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0xFF);
    }

    #[test]
    fn inc_increments_memory() {
        // INC $F0, BRK
        let program = vec![0xE6, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        cpu.mem_write(0xF0, 0xFE);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.mem_read(0xF0), 0xFF);
    }

    #[test]
    fn inx_increments_x() {
        // LDX #$3F, INX, BRK
        let program = vec![0xA2, 0x3F, 0xE8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 0x40);
    }

    #[test]
    fn iny_increments_y() {
        // LDY #$3F, INY, BRK
        let program = vec![0xA0, 0x3F, 0xC8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_y, 0x40);
    }

    #[test]
    fn jmp_absolute() {
        let target = PROGRAM_START + 8;
        let (target_lo, target_hi) = (target as u8, (target >> 8) as u8);
        // JMP target, padding, LDA #$42, BRK
        let program = vec![
            0x4C, target_lo, target_hi, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xA9, 0x42, 0x00,
        ];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x42);
    }

    #[test]
    fn jmp_indirect() {
        let target = PROGRAM_START + 8;
        let (target_lo, target_hi) = (target as u8, (target >> 8) as u8);
        // JMP ($00F2), padding, LDA #$42, BRK
        let program = vec![
            0x6C, 0xF2, 0x00, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xA9, 0x42, 0x00,
        ];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        cpu.mem_write(0xF2, target_lo);
        cpu.mem_write(0xF3, target_hi);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x42);
    }

    #[test]
    fn jsr_rts_returns() {
        let sub = PROGRAM_START + 9;
        let (sub_lo, sub_hi) = (sub as u8, (sub >> 8) as u8);
        // JSR sub, LDA #$11, BRK, padding, RTS
        let program = vec![
            0x20, sub_lo, sub_hi, 0xA9, 0x11, 0x00, 0xEA, 0xEA, 0xEA, 0x60,
        ];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x11);
    }

    #[test]
    fn lda_immediate_loads_a() {
        // LDA #$05, BRK
        let program = vec![0xA9, 0x05, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x05);
        assert!(!cpu.flags.contains(CPUFlags::ZERO));
        assert!(!cpu.flags.contains(CPUFlags::NEGATIVE));
    }

    #[test]
    fn lda_zero_sets_zero_flag() {
        // LDA #$00, BRK
        let program = vec![0xA9, 0x00, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::ZERO));
    }

    #[test]
    fn lda_zero_page() {
        // LDA $F0, BRK
        let program = vec![0xA5, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        cpu.mem_write(0xF0, 0x55);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x55);
    }

    #[test]
    fn ldx_immediate() {
        // LDX #$77, BRK
        let program = vec![0xA2, 0x77, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 0x77);
    }

    #[test]
    fn ldy_immediate() {
        // LDY #$88, BRK
        let program = vec![0xA0, 0x88, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_y, 0x88);
    }

    #[test]
    fn lsr_accumulator_shifts_right() {
        // LDA #$03, LSR A, BRK
        let program = vec![0xA9, 0x03, 0x4A, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x01);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn nop_advances_pc() {
        // NOP, BRK
        let program = vec![0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 2);
    }

    #[test]
    fn ora_immediate_ors() {
        // LDA #$0F, ORA #$F0, BRK
        let program = vec![0xA9, 0x0F, 0x09, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0xFF);
    }

    #[test]
    fn pha_pla_round_trip() {
        // LDA #$37, PHA, LDA #$00, PLA, BRK
        let program = vec![0xA9, 0x37, 0x48, 0xA9, 0x00, 0x68, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x37);
    }

    #[test]
    fn php_plp_round_trip() {
        // SEC, PHP, CLD, PLP, BRK
        let program = vec![0x38, 0x08, 0xD8, 0x28, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::CARRY));
        assert!(!cpu.flags.contains(CPUFlags::DECIMAL));
    }

    #[test]
    fn rol_rotates_left_through_carry() {
        // SEC, LDA #$80, ROL A, BRK
        let program = vec![0x38, 0xA9, 0x80, 0x2A, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x01);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn ror_rotates_right_through_carry() {
        // SEC, LDA #$01, ROR A, BRK
        let program = vec![0x38, 0xA9, 0x01, 0x6A, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x80);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn rti_restores_pc_and_flags() {
        // RTI, NOP, BRK
        let program = vec![0x40, 0xEA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);
        // cpu.reset();

        let resume = PROGRAM_START + 2;
        let (pc_lo, pc_hi) = (resume as u8, (resume >> 8) as u8);
        cpu.mem_write(0x01FB, 0x34);
        cpu.mem_write(0x01FC, pc_lo);
        cpu.mem_write(0x01FD, pc_hi);
        cpu.stack_ptr = 0xFA;
        cpu.run();

        assert_eq!(cpu.program_counter, PROGRAM_START + 3);
    }

    #[test]
    fn rts_returns_from_subroutine() {
        let sub = PROGRAM_START + 6;
        let (sub_lo, sub_hi) = (sub as u8, (sub >> 8) as u8);
        // JSR sub, LDA #$99, BRK, LDA #$55, RTS
        let program = vec![
            0x20, sub_lo, sub_hi, 0xA9, 0x99, 0x00, 0xA9, 0x55, 0x60, 0x00,
        ];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x99);
    }

    #[test]
    fn sbc_immediate_subtracts() {
        // SEC, LDA #$05, SBC #$03, BRK
        let program = vec![0x38, 0xA9, 0x05, 0xE9, 0x03, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0x02);
        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn sec_sets_carry() {
        // SEC, BRK
        let program = vec![0x38, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::CARRY));
    }

    #[test]
    fn sed_sets_decimal() {
        // SED, BRK
        let program = vec![0xF8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::DECIMAL));
    }

    #[test]
    fn sei_sets_interrupt_disable() {
        // SEI, BRK
        let program = vec![0x78, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert!(cpu.flags.contains(CPUFlags::INTERRUPT));
    }

    #[test]
    fn sta_zero_page() {
        // LDA #$42, STA $F0, BRK
        let program = vec![0xA9, 0x42, 0x85, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.mem_read(0xF0), 0x42);
    }

    #[test]
    fn sta_absolute() {
        // LDA #$55, STA $0200, BRK
        let program = vec![0xA9, 0x55, 0x8D, 0x00, 0x02, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.mem_read(0x0200), 0x55);
    }

    #[test]
    fn stx_zero_page() {
        // LDX #$33, STX $F0, BRK
        let program = vec![0xA2, 0x33, 0x86, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.mem_read(0xF0), 0x33);
    }

    #[test]
    fn sty_zero_page() {
        // LDY #$44, STY $F0, BRK
        let program = vec![0xA0, 0x44, 0x84, 0xF0, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.mem_read(0xF0), 0x44);
    }

    #[test]
    fn tax_copies_a_to_x() {
        // LDA #$AB, TAX, BRK
        let program = vec![0xA9, 0xAB, 0xAA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 0xAB);
    }

    #[test]
    fn tay_copies_a_to_y() {
        // LDA #$BC, TAY, BRK
        let program = vec![0xA9, 0xBC, 0xA8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_y, 0xBC);
    }

    #[test]
    fn tsx_loads_stack_ptr_into_x() {
        // TSX, BRK
        let program = vec![0xBA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, STACK_RESET);
    }

    #[test]
    fn txa_copies_x_to_a() {
        // LDX #$CD, TXA, BRK
        let program = vec![0xA2, 0xCD, 0x8A, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0xCD);
    }

    #[test]
    fn txs_sets_stack_ptr() {
        // LDX #$FA, TXS, TSX, BRK
        let program = vec![0xA2, 0xFA, 0x9A, 0xBA, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 0xFA);
    }

    #[test]
    fn tya_copies_y_to_a() {
        // LDY #$DE, TYA, BRK
        let program = vec![0xA0, 0xDE, 0x98, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_a, 0xDE);
    }

    #[test]
    fn inx_overflow_wraps() {
        // LDA #$FF, TAX, INX, INX, BRK
        let program = vec![0xA9, 0xFF, 0xAA, 0xE8, 0xE8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 1);
    }

    #[test]
    fn lda_tax_inx_sequence() {
        // LDA #$C0, TAX, INX, BRK
        let program = vec![0xA9, 0xC0, 0xAA, 0xE8, 0x00];
        let rom = build_test_rom(program);
        let bus = Bus::new(rom);
        let mut cpu = CPU::new(bus);

        // cpu.reset();
        cpu.run();

        assert_eq!(cpu.register_x, 0xC1);
    }

    // Builds a 2-bank (32 KiB) PRG image from `prg` and points the NMI (0xFFFA)
    // and IRQ/BRK (0xFFFE) vectors at the given handler addresses. RESET is left
    // zero; tests start at PROGRAM_START without calling reset().
    fn rom_with_vectors(prg: Vec<u8>, nmi_handler: u16, irq_handler: u16) -> Rom {
        let mut image = prg;
        image.resize(2 * PRG_ROM_PAGE_SIZE, 0);

        let put = |image: &mut [u8], vector: u16, handler: u16| {
            let off = (vector - PROGRAM_START) as usize;
            image[off] = (handler & 0xFF) as u8;
            image[off + 1] = (handler >> 8) as u8;
        };
        put(&mut image, NMI_INTERRUPT_VECTOR, nmi_handler);
        put(&mut image, IRQ_BRK_VECTOR, irq_handler);

        let mut data = vec![
            0x4E, 0x45, 0x53, 0x1A, 0x02, 0x01, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        data.extend_from_slice(&image);
        data.extend(std::iter::repeat(0u8).take(CHR_ROM_PAGE_SIZE));
        Rom::new(&data).unwrap()
    }

    #[test]
    fn interrupt_pushes_status_and_jumps_through_vector() {
        let rom = rom_with_vectors(vec![], 0, 0x9000);
        let mut cpu = CPU::new(Bus::new(rom));

        let sp_before = cpu.stack_ptr;
        cpu.interrupt(&IRQ);

        assert_eq!(cpu.program_counter, 0x9000);
        assert!(cpu.flags.contains(CPUFlags::INTERRUPT));
        assert_eq!(cpu.stack_ptr, sp_before.wrapping_sub(3));

        let pushed_status = cpu.mem_read(STACK + cpu.stack_ptr.wrapping_add(1) as u16);
        assert_eq!(pushed_status & CPUFlags::BREAK.bits(), 0);
        assert_ne!(pushed_status & CPUFlags::BREAK2.bits(), 0);
    }

    #[test]
    fn irq_is_masked_while_interrupt_flag_set() {
        let mut prg = vec![0u8; 2 * PRG_ROM_PAGE_SIZE];
        prg[0x0000..0x0005].copy_from_slice(&[0xA9, 0x01, 0x85, 0x11, 0x00]);
        prg[0x1000..0x1005].copy_from_slice(&[0xA9, 0xEE, 0x85, 0x10, 0x00]);

        let rom = rom_with_vectors(prg, 0, 0x9000);
        let mut cpu = CPU::new(Bus::new(rom));
        assert!(cpu.flags.contains(CPUFlags::INTERRUPT));

        cpu.bus.set_irq(true);
        cpu.run();

        assert_eq!(cpu.mem_read(0x11), 0x01); // main ran
        assert_eq!(cpu.mem_read(0x10), 0x00); // handler did not
    }

    #[test]
    fn irq_vectors_once_interrupt_flag_cleared() {
        let mut prg = vec![0u8; 2 * PRG_ROM_PAGE_SIZE];
        prg[0x0000..0x0002].copy_from_slice(&[0x58, 0x00]);
        prg[0x1000..0x1005].copy_from_slice(&[0xA9, 0xEE, 0x85, 0x10, 0x00]);

        let rom = rom_with_vectors(prg, 0, 0x9000);
        let mut cpu = CPU::new(Bus::new(rom));

        cpu.bus.set_irq(true);
        cpu.run();

        assert_eq!(cpu.mem_read(0x10), 0xEE);
        assert!(cpu.flags.contains(CPUFlags::INTERRUPT));
    }

    #[test]
    fn nmi_at_vblank_vectors_through_fffa() {
        // Full path: the CPU spins in a JMP-to-self loop while the bus ticks the
        // PPU forward; at scanline 241 the PPU raises an NMI, which the run loop
        // polls and services by vectoring through 0xFFFA.
        // Main @ 0x8000: LDA #$80, STA $2000 (enable vblank NMI), JMP $8005 (spin).
        // Handler @ 0x9000: LDA #$EE, STA $10, BRK (records arrival and halts).
        let mut prg = vec![0u8; 2 * PRG_ROM_PAGE_SIZE];
        prg[0x0000..0x0008].copy_from_slice(&[
            0xA9, 0x80, 0x8D, 0x00, 0x20, 0x4C, 0x05, 0x80,
        ]);
        prg[0x1000..0x1005].copy_from_slice(&[0xA9, 0xEE, 0x85, 0x10, 0x00]);

        let rom = rom_with_vectors(prg, 0x9000, 0);
        let mut cpu = CPU::new(Bus::new(rom));
        cpu.run();

        assert_eq!(cpu.mem_read(0x10), 0xEE); // NMI handler ran
        assert!(cpu.flags.contains(CPUFlags::INTERRUPT)); // disabled on entry
    }

    #[test]
    fn nmi_from_control_write_during_vblank_reaches_cpu() {
        // Enabling NMI via a PPUCTRL write *while already in vblank* must raise an
        // NMI immediately, and that NMI must reach the CPU. Handler @ 0x9000
        // records arrival and halts.
        let mut prg = vec![0u8; 2 * PRG_ROM_PAGE_SIZE];
        prg[0x1000..0x1005].copy_from_slice(&[0xA9, 0xEE, 0x85, 0x10, 0x00]);

        let rom = rom_with_vectors(prg, 0x9000, 0);
        let mut cpu = CPU::new(Bus::new(rom));

        // Advance to scanline 241 with NMI disabled: sets the vblank flag but
        // raises no NMI. tick() advances one scanline per call, and each CPU
        // cycle is 3 PPU cycles, so 114 CPU cycles (342 PPU) crosses one line.
        for _ in 0..241 {
            cpu.bus.tick(114);
        }

        // Now enable NMI mid-vblank; write_to_control raises it on the spot.
        cpu.mem_write(0x2000, 0x80);
        cpu.run();

        assert_eq!(cpu.mem_read(0x10), 0xEE);
    }
}
