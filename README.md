# NES Emulator

An NES-style emulator written in Rust with an SDL2 window and audio. Runs snake for the demo.

## Requirements

- **Rust**
- **SDL2** — development libraries must be available to the linker; both the video and audio subsystems are used.

## Running the Emulator

```bash
cargo build
cargo run
```

## Controls

| Key | Action |
|-----|--------|
| **W** / **A** / **S** / **D** | Movement |
| **Escape** / Close window | Quit |

## Tests for the CPU

```bash
cargo test
```
