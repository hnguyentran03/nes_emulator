pub mod bus;
pub mod cpu;
pub mod opcodes;
pub mod ppu;
pub mod rom;
pub mod trace;

use bus::Bus;
use cpu::CPU;
use cpu::Mem;
use rom::Rom;

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate lazy_static;

use rand::prelude::*;
use sdl2::EventPump;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::pixels::PixelFormatEnum;

fn color_for_byte(byte: u8) -> Color {
    match byte {
        0 => sdl2::pixels::Color::BLACK,
        1 => sdl2::pixels::Color::WHITE,
        2 | 9 => sdl2::pixels::Color::RED,
        3 | 10 => sdl2::pixels::Color::GREEN,
        4 | 11 => sdl2::pixels::Color::BLUE,
        5 | 12 => sdl2::pixels::Color::YELLOW,
        6 | 13 => sdl2::pixels::Color::MAGENTA,
        7 | 14 => sdl2::pixels::Color::YELLOW,
        _ => sdl2::pixels::Color::CYAN,
    }
}

fn read_screen(cpu: &mut CPU, frame: &mut [u8; 32 * 3 * 32]) -> bool {
    let mut frame_idx = 0;
    let mut update = false;
    for i in 0x0200..0x0600 {
        let color_idx = cpu.mem_read(i);
        let (r, g, b) = color_for_byte(color_idx).rgb();
        if r != frame[frame_idx] || g != frame[frame_idx + 1] || b != frame[frame_idx + 2] {
            frame[frame_idx] = r;
            frame[frame_idx + 1] = g;
            frame[frame_idx + 2] = b;
            update = true;
        }
        frame_idx += 3;
    }
    update
}

fn handle_user_input(cpu: &mut CPU, event_pump: &mut EventPump) {
    for event in event_pump.poll_iter() {
        match event {
            Event::Quit { .. }
            | Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => std::process::exit(1),
            Event::KeyDown {
                keycode: Some(Keycode::W),
                ..
            } => cpu.mem_write(0xff, 0x77),
            Event::KeyDown {
                keycode: Some(Keycode::S),
                ..
            } => cpu.mem_write(0xff, 0x73),
            Event::KeyDown {
                keycode: Some(Keycode::A),
                ..
            } => cpu.mem_write(0xff, 0x61),
            Event::KeyDown {
                keycode: Some(Keycode::D),
                ..
            } => cpu.mem_write(0xff, 0x64),
            _ => {}
        }
    }
}

fn main() {
    // Initialize SDL2 for the graphics
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window("NES Emulator", 32 * 10, 32 * 10)
        .position_centered()
        .build()
        .unwrap();
    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    let mut event_pump = sdl_context.event_pump().unwrap();
    canvas.set_scale(10.0, 10.0).unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_static(PixelFormatEnum::RGB24, 32, 32)
        .unwrap();

    // Get the ROM path from the command line arguments and run the ROM
    let rom_path = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap();

    let rom_data = std::fs::read(&rom_path).unwrap();

    let rom = Rom::new(&rom_data).unwrap();
    let bus = Bus::new(rom);
    let mut cpu = cpu::CPU::new(bus);
    cpu.reset();

    let mut screen = [0; 32 * 3 * 32];
    let mut rng = rand::rng();

    cpu.run_with_callback(move |cpu| {
        // println!("{}", trace::trace(cpu));

        handle_user_input(cpu, &mut event_pump);
        cpu.mem_write(0xFE, rng.random_range(1..=16));

        if read_screen(cpu, &mut screen) {
            texture.update(None, &screen, 32 * 3).unwrap();
            canvas.copy(&texture, None, None).unwrap();
            canvas.present();
        }

        std::thread::sleep(std::time::Duration::new(0, 70_000));
    });
}
