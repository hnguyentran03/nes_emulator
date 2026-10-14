pub mod bus;
pub mod cpu;
pub mod opcodes;
pub mod ppu;
pub mod render;
pub mod rom;
pub mod tile_viewer;
pub mod trace;

use bus::Bus;
use cpu::CPU;
use cpu::Mem;
use ppu::PPU;
use render::frame::Frame;
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

fn run_snake(rom_path: String) {
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

    let rom_data = std::fs::read(&rom_path).unwrap();

    let rom = Rom::new(&rom_data).unwrap();
    let bus = Bus::new(rom, |_| {});
    let mut cpu = CPU::new(bus);
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

const GAME_SCALE: f32 = 3.0;
const DEFAULT_GAME_ROM: &str = "roms/Alter_Ego.nes";

// Runs a ROM through the full CPU/PPU pipeline. The bus invokes the gameloop
// callback once per frame (on the vblank NMI), which is where the PPU state is
// rendered into a frame buffer and pushed to the SDL window.
fn run_game(rom_path: String) {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window(
            "NES Emulator",
            (Frame::WIDTH as f32 * GAME_SCALE) as u32,
            (Frame::HEIGHT as f32 * GAME_SCALE) as u32,
        )
        .position_centered()
        .build()
        .unwrap();
    let mut canvas = window.into_canvas().present_vsync().build().unwrap();
    let mut event_pump = sdl_context.event_pump().unwrap();
    canvas.set_scale(GAME_SCALE, GAME_SCALE).unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_static(
            PixelFormatEnum::RGB24,
            Frame::WIDTH as u32,
            Frame::HEIGHT as u32,
        )
        .unwrap();

    let rom_data =
        std::fs::read(&rom_path).unwrap_or_else(|e| panic!("failed to read {rom_path}: {e}"));
    let rom = Rom::new(&rom_data).unwrap();

    let mut frame = Frame::new();

    let bus = Bus::new(rom, move |ppu: &PPU| {
        render::render(ppu, &mut frame);
        texture.update(None, &frame.data, Frame::WIDTH * 3).unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => std::process::exit(0),
                _ => {}
            }
        }
    });

    let mut cpu = CPU::new(bus);
    cpu.reset();
    cpu.run();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("snake") => {
            let rom = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "roms/snake.nes".to_string());
            run_snake(rom);
        }
        Some("tiles") => tile_viewer::run(),
        Some(rom) => run_game(rom.to_string()),
        None => run_game(DEFAULT_GAME_ROM.to_string()),
    }
}
