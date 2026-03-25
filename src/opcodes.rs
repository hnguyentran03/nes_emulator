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
        ("BRK", 0x00, 1, AddressingMode::NoneAddressing),
        ("TAX", 0xAA, 1, AddressingMode::NoneAddressing),
        ("INX", 0xE8, 1, AddressingMode::NoneAddressing),
    
        ("LDA", 0xA9, 2, AddressingMode::Immedtiate),
        ("LDA", 0xA5, 2, AddressingMode::ZeroPage),
        ("LDA", 0xB5, 2, AddressingMode::ZeroPage_X),
        ("LDA", 0xAD, 3, AddressingMode::Absolute),
        ("LDA", 0xBD, 3, AddressingMode::Absolute_X),
        ("LDA", 0xB0, 3, AddressingMode::Absolute_Y),
        ("LDA", 0xA1, 2, AddressingMode::Indirect_X),
        ("LDA", 0xB1, 2, AddressingMode::Indirect_Y),
    
        ("STA", 0x85, 2, AddressingMode::ZeroPage),
        ("STA", 0x95, 2, AddressingMode::ZeroPage_X),
        ("STA", 0x8D, 3, AddressingMode::Absolute),
        ("STA", 0x9D, 3, AddressingMode::Absolute_X),
        ("STA", 0x99, 3, AddressingMode::Absolute_Y),
        ("STA", 0x81, 2, AddressingMode::Indirect_X),
        ("STA", 0x91, 2, AddressingMode::Indirect_Y),
        ];
    
        pub static ref OP_CODES: HashMap<u8, OpCode> = {
            let mut map = HashMap::new();
            for (name, code, len, mode) in &*OP_CODES_DEF {
                map.insert(*code, OpCode::new(name, *code, *len, *mode));
            }
            map
        };
}