use crate::cpu::AddressingMode;
use std::collections::HashMap;

pub struct OpCode {
    pub name: &'static str,
    pub code: u8,
    pub bytes: u8,
    pub mode: AddressingMode,
}

impl OpCode {
    fn new(name: &'static str, code: u8, bytes: u8, mode: AddressingMode) -> Self {
        OpCode {
            name,
            code,
            bytes,
            mode,
        }
    }
}

lazy_static! {
    static ref OP_CODES_DEF: Vec<(&'static str, u8, u8, AddressingMode)> = vec![
        // ADC: Add with Carry
        ("ADC", 0x69, 2, AddressingMode::Immedtiate),
        ("ADC", 0x65, 2, AddressingMode::ZeroPage),
        ("ADC", 0x75, 2, AddressingMode::ZeroPage_X),
        ("ADC", 0x6D, 3, AddressingMode::Absolute),
        ("ADC", 0x7D, 3, AddressingMode::Absolute_X),
        ("ADC", 0x79, 3, AddressingMode::Absolute_Y),
        ("ADC", 0x61, 2, AddressingMode::Indirect_X),
        ("ADC", 0x71, 2, AddressingMode::Indirect_Y),

        // AND: Logical AND
        ("AND", 0x29, 2, AddressingMode::Immedtiate),
        ("AND", 0x25, 2, AddressingMode::ZeroPage),
        ("AND", 0x35, 2, AddressingMode::ZeroPage_X),
        ("AND", 0x2D, 3, AddressingMode::Absolute),
        ("AND", 0x3D, 3, AddressingMode::Absolute_X),
        ("AND", 0x39, 3, AddressingMode::Absolute_Y),
        ("AND", 0x21, 2, AddressingMode::Indirect_X),
        ("AND", 0x31, 2, AddressingMode::Indirect_Y),

        // ASL: Arithmetic Shift Left
        ("ASL", 0x0A, 1, AddressingMode::Accumulator),
        ("ASL", 0x06, 2, AddressingMode::ZeroPage),
        ("ASL", 0x16, 2, AddressingMode::ZeroPage_X),
        ("ASL", 0x0E, 3, AddressingMode::Absolute),
        ("ASL", 0x1E, 3, AddressingMode::Absolute_X),

        // BCC: Branch if Carry Clear
        ("BCC", 0x90, 2, AddressingMode::Relative),

        // BCS: Branch if Carry Set
        ("BCS", 0xB0, 2, AddressingMode::Relative),

        // BEQ: Branch if Equal
        ("BEQ", 0xF0, 2, AddressingMode::Relative),

        // BIT: Bit Test
        ("BIT", 0x24, 2, AddressingMode::ZeroPage),
        ("BIT", 0x2C, 3, AddressingMode::Absolute),

        // BMI: Branch if Minus
        ("BMI", 0x30, 2, AddressingMode::Relative),

        // BNE: Branch if Not Equal
        ("BNE", 0xD0, 2, AddressingMode::Relative),

        // BPL: Branch if Positive
        ("BPL", 0x10, 2, AddressingMode::Relative),

        // BRK: Force Interrupt
        ("BRK", 0x00, 1, AddressingMode::Implied),

        // BVC: Branch if Overflow Clear
        ("BVC", 0x50, 2, AddressingMode::Relative),

        // BVS: Branch if Overflow Set
        ("BVS", 0x70, 2, AddressingMode::Relative),

        // CLC: Clear Carry Flag
        ("CLC", 0x18, 1, AddressingMode::Implied),

        // CLD: Clear Decimal Mode
        ("CLD", 0xD8, 1, AddressingMode::Implied),

        // CLI: Clear Interrupt Disable
        ("CLI", 0x58, 1, AddressingMode::Implied),

        // CLV: Clear Overflow Flag
        ("CLV", 0xB8, 1, AddressingMode::Implied),

        // CMP: Compare
        ("CMP", 0xC9, 2, AddressingMode::Immedtiate),
        ("CMP", 0xC5, 2, AddressingMode::ZeroPage),
        ("CMP", 0xD5, 2, AddressingMode::ZeroPage_X),
        ("CMP", 0xCD, 3, AddressingMode::Absolute),
        ("CMP", 0xDD, 3, AddressingMode::Absolute_X),
        ("CMP", 0xD9, 3, AddressingMode::Absolute_Y),
        ("CMP", 0xC1, 2, AddressingMode::Indirect_X),
        ("CMP", 0xD1, 2, AddressingMode::Indirect_Y),

        // CPX: Compare X Register
        ("CPX", 0xE0, 2, AddressingMode::Immedtiate),
        ("CPX", 0xE4, 2, AddressingMode::ZeroPage),
        ("CPX", 0xEC, 3, AddressingMode::Absolute),

        // CPY: Compare Y Register
        ("CPY", 0xC0, 2, AddressingMode::Immedtiate),
        ("CPY", 0xC4, 2, AddressingMode::ZeroPage),
        ("CPY", 0xCC, 3, AddressingMode::Absolute),

        // DEC: Decrement Memory
        ("DEC", 0xC6, 2, AddressingMode::ZeroPage),
        ("DEC", 0xD6, 2, AddressingMode::ZeroPage_X),
        ("DEC", 0xCE, 3, AddressingMode::Absolute),
        ("DEC", 0xDE, 3, AddressingMode::Absolute_X),

        // DEX: Decrement X Register
        ("DEX", 0xCA, 1, AddressingMode::Implied),

        // DEY: Decrement Y Register
        ("DEY", 0x88, 1, AddressingMode::Implied),

        // EOR: Exclusive OR
        ("EOR", 0x49, 2, AddressingMode::Immedtiate),
        ("EOR", 0x45, 2, AddressingMode::ZeroPage),
        ("EOR", 0x55, 2, AddressingMode::ZeroPage_X),
        ("EOR", 0x4D, 3, AddressingMode::Absolute),
        ("EOR", 0x5D, 3, AddressingMode::Absolute_X),
        ("EOR", 0x59, 3, AddressingMode::Absolute_Y),
        ("EOR", 0x41, 2, AddressingMode::Indirect_X),
        ("EOR", 0x51, 2, AddressingMode::Indirect_Y),

        // INC: Increment Memory
        ("INC", 0xE6, 2, AddressingMode::ZeroPage),
        ("INC", 0xF6, 2, AddressingMode::ZeroPage_X),
        ("INC", 0xEE, 3, AddressingMode::Absolute),
        ("INC", 0xFE, 3, AddressingMode::Absolute_X),

        // INX: Increment X Register
        ("INX", 0xE8, 1, AddressingMode::Implied),

        // INY: Increment Y Register
        ("INY", 0xC8, 1, AddressingMode::Implied),

        // JMP: Jump
        ("JMP", 0x4C, 3, AddressingMode::Absolute),
        ("JMP", 0x6C, 3, AddressingMode::Indirect),

        // JSR: Jump to Subroutine
        ("JSR", 0x20, 3, AddressingMode::Absolute),

        // LDA: Load Accumulator
        ("LDA", 0xA9, 2, AddressingMode::Immedtiate),
        ("LDA", 0xA5, 2, AddressingMode::ZeroPage),
        ("LDA", 0xB5, 2, AddressingMode::ZeroPage_X),
        ("LDA", 0xAD, 3, AddressingMode::Absolute),
        ("LDA", 0xBD, 3, AddressingMode::Absolute_X),
        ("LDA", 0xB9, 3, AddressingMode::Absolute_Y),
        ("LDA", 0xA1, 2, AddressingMode::Indirect_X),
        ("LDA", 0xB1, 2, AddressingMode::Indirect_Y),

        // LDX: Load X Register
        ("LDX", 0xA2, 2, AddressingMode::Immedtiate),
        ("LDX", 0xA6, 2, AddressingMode::ZeroPage),
        ("LDX", 0xB6, 2, AddressingMode::ZeroPage_Y),
        ("LDX", 0xAE, 3, AddressingMode::Absolute),
        ("LDX", 0xBE, 3, AddressingMode::Absolute_Y),

        // LDY: Load Y Register
        ("LDY", 0xA0, 2, AddressingMode::Immedtiate),
        ("LDY", 0xA4, 2, AddressingMode::ZeroPage),
        ("LDY", 0xB4, 2, AddressingMode::ZeroPage_X),
        ("LDY", 0xAC, 3, AddressingMode::Absolute),
        ("LDY", 0xBC, 3, AddressingMode::Absolute_X),

        // LSR: Logical Shift Right
        ("LSR", 0x4A, 1, AddressingMode::Accumulator),
        ("LSR", 0x46, 2, AddressingMode::ZeroPage),
        ("LSR", 0x56, 2, AddressingMode::ZeroPage_X),
        ("LSR", 0x4E, 3, AddressingMode::Absolute),
        ("LSR", 0x5E, 3, AddressingMode::Absolute_X),

        // NOP: No Operation
        ("NOP", 0xEA, 1, AddressingMode::Implied),

        // ORA: Logical Inclusive OR
        ("ORA", 0x09, 2, AddressingMode::Immedtiate),
        ("ORA", 0x05, 2, AddressingMode::ZeroPage),
        ("ORA", 0x15, 2, AddressingMode::ZeroPage_X),
        ("ORA", 0x0D, 3, AddressingMode::Absolute),
        ("ORA", 0x1D, 3, AddressingMode::Absolute_X),
        ("ORA", 0x19, 3, AddressingMode::Absolute_Y),
        ("ORA", 0x01, 2, AddressingMode::Indirect_X),
        ("ORA", 0x11, 2, AddressingMode::Indirect_Y),

        // PHA: Push Accumulator
        ("PHA", 0x48, 1, AddressingMode::Implied),

        // PHP: Push Processor Status
        ("PHP", 0x08, 1, AddressingMode::Implied),

        // PLA: Pull Accumulator
        ("PLA", 0x68, 1, AddressingMode::Implied),

        // PLP: Pull Processor Status
        ("PLP", 0x28, 1, AddressingMode::Implied),

        // ROL: Rotate Left
        ("ROL", 0x2A, 1, AddressingMode::Accumulator),
        ("ROL", 0x26, 2, AddressingMode::ZeroPage),
        ("ROL", 0x36, 2, AddressingMode::ZeroPage_X),
        ("ROL", 0x2E, 3, AddressingMode::Absolute),
        ("ROL", 0x3E, 3, AddressingMode::Absolute_X),

        // ROR: Rotate Right
        ("ROR", 0x6A, 1, AddressingMode::Accumulator),
        ("ROR", 0x66, 2, AddressingMode::ZeroPage),
        ("ROR", 0x76, 2, AddressingMode::ZeroPage_X),
        ("ROR", 0x6E, 3, AddressingMode::Absolute),
        ("ROR", 0x7E, 3, AddressingMode::Absolute_X),

        // RTI: Return from Interrupt
        ("RTI", 0x40, 1, AddressingMode::Implied),

        // RTS: Return from Subroutine
        ("RTS", 0x60, 1, AddressingMode::Implied),

        // SBC: Subtract with Carry
        ("SBC", 0xE9, 2, AddressingMode::Immedtiate),
        ("SBC", 0xE5, 2, AddressingMode::ZeroPage),
        ("SBC", 0xF5, 2, AddressingMode::ZeroPage_X),
        ("SBC", 0xED, 3, AddressingMode::Absolute),
        ("SBC", 0xFD, 3, AddressingMode::Absolute_X),
        ("SBC", 0xF9, 3, AddressingMode::Absolute_Y),
        ("SBC", 0xE1, 2, AddressingMode::Indirect_X),
        ("SBC", 0xF1, 2, AddressingMode::Indirect_Y),

        // SEC: Set Carry Flag
        ("SEC", 0x38, 1, AddressingMode::Implied),

        // SED: Set Decimal Flag
        ("SED", 0xF8, 1, AddressingMode::Implied),

        // SEI: Set Interrupt Disable
        ("SEI", 0x78, 1, AddressingMode::Implied),

        // STA: Store Accumulator
        ("STA", 0x85, 2, AddressingMode::ZeroPage),
        ("STA", 0x95, 2, AddressingMode::ZeroPage_X),
        ("STA", 0x8D, 3, AddressingMode::Absolute),
        ("STA", 0x9D, 3, AddressingMode::Absolute_X),
        ("STA", 0x99, 3, AddressingMode::Absolute_Y),
        ("STA", 0x81, 2, AddressingMode::Indirect_X),
        ("STA", 0x91, 2, AddressingMode::Indirect_Y),

        // STX: Store X Register
        ("STX", 0x86, 2, AddressingMode::ZeroPage),
        ("STX", 0x96, 2, AddressingMode::ZeroPage_Y),
        ("STX", 0x8E, 3, AddressingMode::Absolute),

        // STY: Store Y Register
        ("STY", 0x84, 2, AddressingMode::ZeroPage),
        ("STY", 0x94, 2, AddressingMode::ZeroPage_X),
        ("STY", 0x8C, 3, AddressingMode::Absolute),

        // TAX: Transfer A to X
        ("TAX", 0xAA, 1, AddressingMode::Implied),

        // TAY: Transfer A to Y
        ("TAY", 0xA8, 1, AddressingMode::Implied),

        // TSX: Transfer Stack Pointer to X
        ("TSX", 0xBA, 1, AddressingMode::Implied),

        // TXA: Transfer X to A
        ("TXA", 0x8A, 1, AddressingMode::Implied),

        // TXS: Transfer X to Stack Pointer
        ("TXS", 0x9A, 1, AddressingMode::Implied),

        // TYA: Transfer Y to A
        ("TYA", 0x98, 1, AddressingMode::Implied),
        ];
    
        pub static ref OP_CODES: HashMap<u8, OpCode> = {
            let mut map = HashMap::new();
            for (name, code, len, mode) in &*OP_CODES_DEF {
                map.insert(*code, OpCode::new(name, *code, *len, *mode));
            }
            map
        };
}