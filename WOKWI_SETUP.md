# Wokwi Setup for TheH

This repository now includes reusable Wokwi board profiles and a profile switch script.

## What was configured

- Board profiles in `wokwi-profiles/`:
  - `esp32-c3`
  - `pi-pico`
  - `arduino-uno`
- Auto-switch script: `scripts/select_wokwi_profile.ps1`
- VS Code tasks in `.vscode/tasks.json` for one-click profile selection.

## Quick start

1. Run one of these VS Code tasks:
   - `Wokwi: Select ESP32-C3 profile`
   - `Wokwi: Select Pi Pico profile`
   - `Wokwi: Select Arduino Uno profile`
2. The task copies profile files to workspace root as:
   - `wokwi.toml`
   - `diagram.json`
3. Press F1 and run `Wokwi: Start Simulator`.

## Firmware paths used

The selected profile expects firmware artifacts under `target/wokwi/<board>/`:

- ESP32-C3: `firmware.elf`
- Pi Pico: `firmware.uf2` and optional `firmware.elf`
- Arduino Uno: `firmware.hex` and optional `firmware.elf`

## Current limitation for direct H runtime on MCU targets

Your current `--native` backend generates a host runtime that uses desktop `std` APIs
(file I/O, process execution, etc.). This works for host binaries, but is not yet a
microcontroller firmware runtime.

So this Wokwi setup is ready for board simulation plumbing now, while we still need an
embedded runtime layer for true native H-on-MCU execution.

## Recommended next implementation step

Create a minimal embedded runtime backend for one board first (suggested: ESP32-C3):

1. Add an `embedded` codegen mode that avoids desktop-only builtins.
2. Provide board HAL bindings for GPIO/UART/timer builtins.
3. Emit board-compatible firmware (`.elf`/`.bin`) into `target/wokwi/esp32-c3/`.
4. Use current profile files unchanged to run in Wokwi.

## New separate microcontroller workspace

There is now a dedicated directory for the MCU-focused variant:

- `h-embedded/`

Quick run:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3
```

This command validates embedded-safe language usage and writes board output files under `target/wokwi/<board>/`.

Build real ESP32-C3 firmware with ESP-IDF:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --build
```

Auto flash to a connected ESP32-C3 board:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --flash --port COM5
```

Auto flash + serial monitor:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --flash --monitor --port COM5
```

Note: build/flash/monitor requires `idf.py` from ESP-IDF in PATH.
