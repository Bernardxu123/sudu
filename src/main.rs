#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unused_crate_dependencies)]

mod app;
mod doc;
mod doc_widget;
mod editor;
mod images;
mod offsets;
mod session;
mod smooth;
mod table;
mod table_widget;
mod theme;

use std::path::PathBuf;
use std::time::Instant;

fn main() -> iced::Result {
    let start = Instant::now();
    let mut initial = None;
    let mut start_edit = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--edit" {
            start_edit = true;
        } else if initial.is_none() {
            initial = Some(PathBuf::from(arg));
        }
    }
    let probe = std::env::var_os("SUDU_PROBE").map(PathBuf::from);
    let restored = if std::env::var_os("SUDU_NO_SESSION").is_none() {
        session::load()
    } else {
        session::Session::default()
    };
    let position = restored
        .window_pos
        .filter(|[x, y]| x.is_finite() && y.is_finite() && *x > -4000.0 && *y > -4000.0)
        .map(|[x, y]| iced::window::Position::Specific(iced::Point::new(x, y)))
        .unwrap_or(iced::window::Position::Centered);
    let window = iced::window::Settings {
        size: iced::Size::new(
            restored.window_size[0].clamp(720.0, 3840.0),
            restored.window_size[1].clamp(480.0, 2160.0),
        ),
        min_size: Some(iced::Size::new(720.0, 480.0)),
        position,
        maximized: restored.maximized,
        decorations: false,
        exit_on_close_request: false,
        resizable: true,
        icon: window_icon(),
        ..Default::default()
    };
    iced::application(
        move || {
            let (mut app_state, startup_task) =
                app::App::new(initial.clone(), restored.clone(), start, probe.clone());
            if start_edit {
                app_state.enter_edit_on_start();
            }
            (
                app_state,
                iced::Task::batch([
                    iced::window::latest().map(app::Message::WindowId),
                    startup_task,
                ]),
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

fn window_icon() -> Option<iced::window::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/icon-256.png")).ok()?;
    let image = image.to_rgba8();
    let (width, height) = image.dimensions();
    iced::window::icon::from_rgba(image.into_raw(), width, height).ok()
}
