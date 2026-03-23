use std::path::{Path, PathBuf};

use clap::ValueEnum;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum Board {
    Esp32C3,
    PiPico,
    ArduinoUno,
}

impl Board {
    pub fn slug(self) -> &'static str {
        match self {
            Board::Esp32C3 => "esp32-c3",
            Board::PiPico => "pi-pico",
            Board::ArduinoUno => "arduino-uno",
        }
    }

    pub fn firmware_files(self) -> &'static [&'static str] {
        match self {
            Board::Esp32C3 => &["firmware.elf"],
            Board::PiPico => &["firmware.uf2", "firmware.elf"],
            Board::ArduinoUno => &["firmware.hex", "firmware.elf"],
        }
    }

    pub fn default_out_dir(self, repo_root: &Path) -> PathBuf {
        repo_root.join("target").join("wokwi").join(self.slug())
    }
}
