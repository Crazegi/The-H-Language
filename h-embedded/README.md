# h-embedded

This is a separate microcontroller-focused H toolchain workspace.

It currently provides:

- Parse + semantic analysis using the main H frontend.
- Embedded profile linting that blocks desktop/script APIs.
- Board-target output layout for Wokwi directories.
- Real ESP32-C3 emission as an ESP-IDF firmware project (minimal GPIO/UART runtime).
- Repeat-loop blink lowering with delay (`sleep_ms`).
- GPIO/UART input support (`gpio.read`, `uart.read`).
- ESP32-C3 board profile selection (`--esp32c3-profile`).

## Supported board targets

- esp32-c3
- pi-pico
- arduino-uno

## Usage

From repository root:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3
```

Use board profile mappings (default is `devkit-m1`):

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --esp32c3-profile super-mini
```

Build real ESP32-C3 firmware (requires ESP-IDF `idf.py` in PATH):

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --build
```

Auto flash to hardware:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --flash --port COM5
```

Auto flash and open serial monitor:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board esp32-c3 --flash --monitor --port COM5
```

With custom output dir:

```powershell
cargo run --manifest-path h-embedded/Cargo.toml -- h-embedded/examples/blink_embedded.hl --board pi-pico --out-dir target/wokwi/pi-pico
```

## Important note

ESP32-C3 now emits a real ESP-IDF project under `target/wokwi/esp32-c3/esp32c3-idf`.
Pi Pico and Arduino Uno are still placeholders for now.
