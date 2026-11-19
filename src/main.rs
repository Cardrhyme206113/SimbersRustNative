#![cfg_attr(target_os="windows", windows_subsystem="windows")]
mod audio;
mod analysis;
mod corpus;
mod engine;
mod export;
mod model;
mod ui;
fn main()->eframe::Result<()> {
    let opts=eframe::NativeOptions {
        viewport:eframe::egui::ViewportBuilder::default().with_title("Simbers Native")
        .with_inner_size([1220.,745.]).with_min_inner_size([880.,590.]),
        ..Default::default()
    };
    eframe::run_native("Simbers Native",opts,Box::new(|_|Ok(Box::<ui::App>::default())))
}
