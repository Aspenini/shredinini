#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(test)]
mod allocation_probe;
#[cfg(not(target_arch = "wasm32"))]
mod audio;
#[cfg(not(target_arch = "wasm32"))]
use shredini_core::dsp;
#[cfg(not(target_arch = "wasm32"))]
use shredini_core::media;
#[cfg(not(target_arch = "wasm32"))]
use shredini_core::midi;
#[cfg(not(target_arch = "wasm32"))]
use shredini_core::model;
#[cfg(not(target_arch = "wasm32"))]
mod storage;
#[cfg(not(target_arch = "wasm32"))]
mod ui;

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    if std::env::args().any(|arg| arg == "--probe-midi") {
        if let Err(e) = midi::probe(15) {
            eprintln!("{e:#}");
        }
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--list-midi") {
        let (ports, note) = midi::enumerate();
        for port in ports {
            println!("{} | {}", port.name, port.id);
        }
        if !note.is_empty() {
            println!("{note}");
        }
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--list-devices") {
        audio::print_devices();
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Shredini")
            .with_inner_size([1200.0, 900.0])
            .with_min_inner_size([1024.0, 740.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Shredini",
        options,
        Box::new(|cc| Ok(Box::new(ui::Studio::new(cc)))),
    )
}
