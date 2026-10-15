# NES Emulator

A Nintendo Entertainment System emulator written in Rust. It emulates the 6502 CPU, the PPU (graphics), the APU (sound) and the standard controller, and shows the picture in an SDL2 window with sound through SDL2 audio. It plays iNES 1.0 ROMs that use mapper 0 (NROM), such as the bundled `roms/Alter_Ego.nes`; ROMs that need a bank-switching mapper do not run yet.

Besides the full game loop there are two smaller modes: a snake demo that runs a 6502 program straight out of CPU RAM, and a static viewer for a ROM's CHR tiles.

## Install

Prebuilt binaries for macOS (Apple Silicon and Intel), Linux (x86_64) and Windows (x86_64) are attached to each [GitHub release](https://github.com/hnguyentran03/nes_emulator/releases). Download the archive for your platform and run the `nes_emulator` binary inside it, or use the installer scripts that each release ships:

```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hnguyentran03/nes_emulator/releases/latest/download/nes_emulator-installer.sh | sh
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hnguyentran03/nes_emulator/releases/latest/download/nes_emulator-installer.ps1 | iex"
```

SDL2 is linked into the binary, so nothing else needs to be installed. On macOS an unsigned download shows a Gatekeeper warning the first time; right-click the binary and choose Open, or run `xattr -d com.apple.quarantine nes_emulator`.

## Quick start

```bash
nes_emulator                       # plays the bundled Alter Ego
nes_emulator path/to/rom.nes       # plays any mapper-0 ROM
nes_emulator snake                 # snake demo
nes_emulator tiles [rom]           # shows a ROM's first CHR pattern table
```

Alter Ego and the snake ROM are embedded in the binary, so it runs from any directory.

## Building from source

Install Rust and CMake (`brew install cmake` on macOS; the `cmake` package on Linux, plus SDL2's development headers listed in `dist-workspace.toml`; the CMake installer or `winget install Kitware.CMake` on Windows). SDL2 itself is compiled from source and linked statically, so no SDL2 install is needed.

```bash
cargo build --release           # binary at target/release/nes_emulator
cargo run                       # same modes as above: cargo run [rom | snake [rom] | tiles [rom]]
cargo test                      # all unit tests
```

The first build compiles SDL2 and takes a few minutes; later builds are incremental.

## Controls

Game mode:

| Key | NES button |
|-----|------------|
| Arrow keys | D-pad |
| A | A |
| S | B |
| Space | Select |
| Return | Start |
| Escape / close window | Quit |

Snake demo: W / A / S / D move, Escape quits.

## Layout

- `src/cpu.rs`, `src/opcodes.rs` — the 6502 core and its opcode table.
- `src/bus.rs` — the CPU memory map that ties RAM, PPU, APU, controller and PRG ROM together.
- `src/ppu/` — picture processing unit and its registers.
- `src/apu/` — audio processing unit: channels, frame counter, mixer.
- `src/render/` — draws a frame from PPU state; palette and frame buffer.
- `src/rom.rs` — iNES file parsing. `src/joypad.rs` — controller 1.
- `src/main.rs` — SDL2 window, audio and the three run modes. `src/tile_viewer.rs` — CHR viewer.
- `roms/` — sample ROMs. 
- `dist-workspace.toml`, `.github/workflows/release.yml` — `cargo-dist` release configuration and the workflow that builds the binaries.

## Notes

- `.cargo/config.toml` sets `CMAKE_POLICY_VERSION_MINIMUM=3.5` so CMake 4 accepts the bundled SDL2 sources. Nothing else is needed for the build.
- Without a usable audio device the game still runs, silently, paced by vsync instead of the audio queue.
- Only the bundled freeware ROMs are embedded or shipped. Commercial ROMs are not included and must stay out of the repo.
