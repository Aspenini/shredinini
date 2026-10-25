#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(test)]
mod allocation_probe;
mod audio;
mod dsp;
mod midi;
mod model;
mod storage;
mod ui;

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
            .with_inner_size([920.0, 860.0])
            .with_min_inner_size([700.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Shredini",
        options,
        Box::new(|cc| Ok(Box::new(ui::Studio::new(cc)))),
    )
}
