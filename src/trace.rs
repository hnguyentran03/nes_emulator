use crate::cpu::AddressingMode;
use crate::cpu::CPU;
use crate::cpu::Mem;

use crate::opcodes;
use std::collections::HashMap;

fn get_mem_str(
    cpu: &mut CPU,
    opcode: &opcodes::OpCode,
    pc: u16,
    mem_addr: u16,
    mem_data: u8,
) -> String {
    let addr = cpu.mem_read(pc + 1);
    let addr_u16 = cpu.mem_read_u16(pc + 1);

    match opcode.mode {
        AddressingMode::Implied => String::from(""),
        AddressingMode::Accumulator => String::from("A "),
        AddressingMode::Immediate => format!("#${:02x}", addr),
        AddressingMode::Relative => {
            let new_addr = (pc + 2).wrapping_add((addr as i8) as u16);
            format!("${:04x}", new_addr)
        }
        AddressingMode::ZeroPage => format!("${:02x} = {:02x}", mem_addr, mem_data),
        AddressingMode::ZeroPage_X => {
            format!("${:02x},X @ {:02x} = {:02x}", addr, mem_addr, mem_data)
        }
        AddressingMode::ZeroPage_Y => {
            format!("${:02x},Y @ {:02x} = {:02x}", addr, mem_addr, mem_data)
        }
        AddressingMode::Absolute => format!("${:04x}", mem_addr),
        AddressingMode::Absolute_X => {
            format!("${:04x},X @ {:04x} = {:02x}", addr_u16, mem_addr, mem_data)
        }
        AddressingMode::Absolute_Y => {
            format!("${:04x},Y @ {:04x} = {:02x}", addr_u16, mem_addr, mem_data)
        }
        AddressingMode::Indirect => {
            let jmp_addr = if addr_u16 & 0x00FF == 0x00FF {
                let lo = cpu.mem_read(addr_u16);
                let hi = cpu.mem_read(addr_u16 & 0xFF00);
                (hi as u16) << 8 | (lo as u16)
            } else {
                cpu.mem_read_u16(addr_u16)
            };

            // let jmp_addr = cpu.mem_read_u16(address);
            format!("(${:04x}) = {:04x}", addr_u16, jmp_addr)
        }
        AddressingMode::Indirect_X => format!(
            "(${:02x},X) @ {:02x} = {:04x} = {:02x}",
            addr,
            addr.wrapping_add(cpu.register_x),
            mem_addr,
            mem_data
        ),
        AddressingMode::Indirect_Y => format!(
            "(${:02x}),Y = {:04x} @ {:04x} = {:02x}",
            addr,
            mem_addr.wrapping_sub(cpu.register_y as u16),
            mem_addr,
            mem_data
        ),
    }
}

pub fn trace(cpu: &mut CPU) -> String {
    let opcodes: &HashMap<u8, opcodes::OpCode> = &opcodes::OP_CODES;
    let code = cpu.mem_read(cpu.program_counter);
    let opcode = opcodes.get(&code).unwrap();
    let pc = cpu.program_counter;

    let mut dump = vec![];
    dump.push(code);

    let (mem_addr, mem_data) = match opcode.mode {
        AddressingMode::Implied
        | AddressingMode::Accumulator
        | AddressingMode::Immediate
        | AddressingMode::Relative
        | AddressingMode::Indirect => (0, 0),
        _ => {
            let (addr, _) = cpu.get_absolute_address(&opcode.mode, pc + 1);
            (addr, cpu.mem_read(addr))
        }
    };

    let mem_str = match opcode.bytes {
        1 => get_mem_str(cpu, opcode, pc, mem_addr, mem_data),
        2 => {
            let addr = cpu.mem_read(pc + 1);
            dump.push(addr);
            get_mem_str(cpu, opcode, pc, mem_addr, mem_data)
        }
        3 => {
            let addr_lo = cpu.mem_read(pc + 1);
            let addr_hi = cpu.mem_read(pc + 2);
            dump.push(addr_lo);
            dump.push(addr_hi);

            get_mem_str(cpu, opcode, pc, mem_addr, mem_data)
        }
        _ => String::from(""),
    };

    let trace_str = dump
        .iter()
        .map(|z| format!("{:02x}", z))
        .collect::<Vec<String>>()
        .join(" ");

    let asm_str = format!(
        "{:04x}  {:8} {: >4} {}",
        pc, trace_str, opcode.name, mem_str
    )
    .trim()
    .to_string();

    format!(
        "{:47} A:{:02x} X:{:02x} Y:{:02x} P:{:02x} SP:{:02x}",
        asm_str,
        cpu.register_a,
        cpu.register_x,
        cpu.register_y,
        cpu.flags.bits(),
        cpu.stack_ptr,
    )
    .to_ascii_uppercase()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::bus::Bus;
    use crate::rom::test::build_test_rom;

    #[test]
    fn test_format_trace() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        bus.mem_write(100, 0xa2);
        bus.mem_write(101, 0x01);
        bus.mem_write(102, 0xca);
        bus.mem_write(103, 0x88);
        bus.mem_write(104, 0x00);

        let mut cpu = CPU::new(bus);
        cpu.program_counter = 0x64;
        cpu.register_a = 1;
        cpu.register_x = 2;
        cpu.register_y = 3;
        let mut result: Vec<String> = vec![];
        cpu.run_with_callback(|cpu| {
            result.push(trace(cpu));
        });
        assert_eq!(
            "0064  A2 01     LDX #$01                        A:01 X:02 Y:03 P:24 SP:FD",
            result[0]
        );
        assert_eq!(
            "0066  CA        DEX                             A:01 X:01 Y:03 P:24 SP:FD",
            result[1]
        );
        assert_eq!(
            "0067  88        DEY                             A:01 X:00 Y:03 P:26 SP:FD",
            result[2]
        );
    }

    #[test]
    fn test_format_mem_access() {
        let mut bus = Bus::new(build_test_rom(vec![]));
        // ORA ($33), Y
        bus.mem_write(100, 0x11);
        bus.mem_write(101, 0x33);

        //data
        bus.mem_write(0x33, 00);
        bus.mem_write(0x34, 04);

        //target cell
        bus.mem_write(0x400, 0xAA);

        let mut cpu = CPU::new(bus);
        cpu.program_counter = 0x64;
        cpu.register_y = 0;
        let mut result: Vec<String> = vec![];
        cpu.run_with_callback(|cpu| {
            result.push(trace(cpu));
        });
        assert_eq!(
            "0064  11 33     ORA ($33),Y = 0400 @ 0400 = AA  A:00 X:00 Y:00 P:24 SP:FD",
            result[0]
        );
    }
}
