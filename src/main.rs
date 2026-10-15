pub mod apu;
pub mod bus;
pub mod cpu;
pub mod joypad;
pub mod opcodes;
pub mod ppu;
pub mod render;
pub mod rom;
pub mod tile_viewer;
pub mod trace;

use bus::Bus;
use cpu::CPU;
use cpu::Mem;
use joypad::{Joypad, JoypadButton};
use ppu::PPU;
use render::frame::Frame;
use rom::Rom;
use std::collections::HashMap;

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate lazy_static;

use rand::prelude::*;
use sdl2::EventPump;
use sdl2::audio::{AudioQueue, AudioSpecDesired};
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

fn run_snake(rom_data: &[u8]) {
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

    let rom = Rom::new(rom_data).unwrap();
    let bus = Bus::new(rom, |_, _| {});
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

// Host keyboard -> NES controller 1.
fn joypad_key_map() -> HashMap<Keycode, JoypadButton> {
    HashMap::from([
        (Keycode::Up, JoypadButton::UP),
        (Keycode::Down, JoypadButton::DOWN),
        (Keycode::Left, JoypadButton::LEFT),
        (Keycode::Right, JoypadButton::RIGHT),
        (Keycode::Space, JoypadButton::SELECT),
        (Keycode::Return, JoypadButton::START),
        (Keycode::A, JoypadButton::BUTTON_A),
        (Keycode::S, JoypadButton::BUTTON_B),
    ])
}

const GAME_SCALE: f32 = 3.0;
// The audio queue paces emulation: after queueing a frame's samples the
// callback waits until the device has drained to the target depth, so the
// game runs at the APU's rate rather than the display's refresh rate. The
// target is 50 ms or three device buffers, whichever is larger, so a single
// device pull never empties the queue.
const AUDIO_DEVICE_BUFFER_SAMPLES: u16 = 512;
const AUDIO_QUEUE_TARGET_SECONDS: f64 = 0.05;
const AUDIO_QUEUE_MIN_DEVICE_BUFFERS: u32 = 3;
const AUDIO_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1);
// A device that stops draining must not freeze the window.
const AUDIO_WAIT_LIMIT: std::time::Duration = std::time::Duration::from_millis(100);
// Freeware homebrew ROMs embedded as the no-argument defaults, so a
// downloaded binary runs without a roms/ directory next to it.
const DEFAULT_GAME_ROM: &[u8] = include_bytes!("../roms/Alter_Ego.nes");
const DEFAULT_SNAKE_ROM: &[u8] = include_bytes!("../roms/snake.nes");

fn audio_queue_target_bytes(freq: i32, device_buffer_samples: u16) -> u32 {
    let target_samples = ((freq as f64 * AUDIO_QUEUE_TARGET_SECONDS) as u32)
        .max(AUDIO_QUEUE_MIN_DEVICE_BUFFERS * device_buffer_samples as u32);
    target_samples * size_of::<f32>() as u32
}

// Opens a mono f32 queue at the APU's sample rate. Without a usable audio
// device the game still runs, silently, paced by vsync instead.
fn open_audio_queue(sdl_context: &sdl2::Sdl) -> Option<AudioQueue<f32>> {
    let audio_spec = AudioSpecDesired {
        freq: Some(apu::SAMPLE_RATE_HZ as i32),
        channels: Some(1),
        samples: Some(AUDIO_DEVICE_BUFFER_SAMPLES),
    };
    match sdl_context
        .audio()
        .and_then(|audio| audio.open_queue(None, &audio_spec))
    {
        Ok(audio_queue) => {
            audio_queue.resume();
            Some(audio_queue)
        }
        Err(e) => {
            eprintln!("audio unavailable, running without sound: {e}");
            None
        }
    }
}

// Runs a ROM through the full CPU/PPU pipeline. The bus invokes the gameloop
// callback once per frame (on the vblank NMI), which is where the PPU state is
// rendered into a frame buffer and pushed to the SDL window.
fn run_game(rom_data: &[u8]) {
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
    // With audio the queue depth paces emulation; without it, vsync does.
    let audio_queue = open_audio_queue(&sdl_context);
    let canvas_builder = window.into_canvas();
    let mut canvas = if audio_queue.is_some() {
        canvas_builder.build()
    } else {
        canvas_builder.present_vsync().build()
    }
    .unwrap();
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

    let rom = Rom::new(rom_data).unwrap();

    let mut frame = Frame::new();
    let key_map = joypad_key_map();

    let mut bus = Bus::new(rom, move |ppu: &PPU, joypad: &mut Joypad| {
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
                Event::KeyDown {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(button) = key_map.get(&keycode) {
                        joypad.set_button_pressed_status(*button, true);
                    }
                }
                Event::KeyUp {
                    keycode: Some(keycode),
                    ..
                } => {
                    if let Some(button) = key_map.get(&keycode) {
                        joypad.set_button_pressed_status(*button, false);
                    }
                }
                _ => {}
            }
        }
    });

    if let Some(audio_queue) = audio_queue {
        let spec = audio_queue.spec();
        let target_bytes = audio_queue_target_bytes(spec.freq, spec.samples);
        bus.set_audio_callback(move |samples: &[f32]| {
            audio_queue.queue_audio(samples).unwrap();
            let deadline = std::time::Instant::now() + AUDIO_WAIT_LIMIT;
            while audio_queue.size() > target_bytes && std::time::Instant::now() < deadline {
                std::thread::sleep(AUDIO_POLL_INTERVAL);
            }
            if audio_queue.size() > target_bytes {
                audio_queue.clear(); // the device stalled; do not let the queue grow
            }
        });
    }

    let mut cpu = CPU::new(bus);
    cpu.reset();
    cpu.run();
}

// Reads the ROM at `path`, or returns the embedded default when no path was given.
fn load_rom(path: Option<&String>, default: &'static [u8]) -> Vec<u8> {
    match path {
        Some(path) => std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}")),
        None => default.to_vec(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("snake") => run_snake(&load_rom(args.get(2), DEFAULT_SNAKE_ROM)),
        Some("tiles") => tile_viewer::run(&load_rom(args.get(2), DEFAULT_GAME_ROM)),
        Some(_) => run_game(&load_rom(args.get(1), DEFAULT_GAME_ROM)),
        None => run_game(DEFAULT_GAME_ROM),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn audio_target_is_50ms_unless_three_device_buffers_are_larger() {
        // 50 ms at 44.1 kHz is 2205 samples of 4 bytes.
        assert_eq!(audio_queue_target_bytes(44_100, 512), 2205 * 4);
        // A 2048-sample device buffer needs three buffers of margin instead.
        assert_eq!(audio_queue_target_bytes(44_100, 2048), 6144 * 4);
    }
}
