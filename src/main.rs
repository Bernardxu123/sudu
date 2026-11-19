#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unused_crate_dependencies)]

mod app;
mod doc;
mod offsets;
mod smooth;
mod theme;

use std::path::PathBuf;
use std::time::Instant;

fn main() -> iced::Result {
    let start = Instant::now();
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    let probe = std::env::var_os("SUDU_PROBE").map(PathBuf::from);
    let window = iced::window::Settings {
        size: iced::Size::new(1120.0, 820.0),
        min_size: Some(iced::Size::new(720.0, 480.0)),
        decorations: false,
        resizable: true,
        ..Default::default()
    };
    iced::application(
        move || {
            (
                app::App::new(initial.clone(), start, probe.clone()),
                iced::window::latest().map(app::Message::WindowId),
            )
        },
        app::App::update,
        app::App::view,
    )
    .title(app::App::title)
    .theme(app::App::theme)
    .subscription(app::App::subscription)
    .window(window)
    .default_font(theme::ui_font())
    .run()
}
