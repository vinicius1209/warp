#[macro_use]
extern crate num_derive;

pub mod accessibility;
pub mod actions;
mod app_focus_telemetry;
pub mod assets;
pub mod r#async;
pub mod clipboard;
pub mod clipboard_utils;
mod core;
#[cfg(not(feature = "tui"))]
mod debug;
pub mod elements;
pub mod event;
pub mod fonts;
pub mod image_cache;
#[cfg(not(feature = "tui"))]
pub mod integration;
pub mod keymap;
pub mod modals;
pub mod notification;
pub mod platform;
pub mod prelude;
#[cfg(not(feature = "tui"))]
pub mod presenter;
#[cfg(not(feature = "tui"))]
pub mod rendering;
#[cfg(not(feature = "tui"))]
pub mod scene;
pub mod telemetry;
#[cfg(test)]
mod test;
pub mod text;
#[cfg(not(feature = "tui"))]
pub mod text_layout;
pub mod text_selection_utils;
pub mod time;
pub mod traces;
#[cfg(not(feature = "tui"))]
pub mod ui_components;
pub mod units;
pub mod util;
pub mod windowing;
pub mod zoom;

pub use assets::AssetProvider;
pub use clipboard::Clipboard;
#[cfg(not(feature = "tui"))]
pub use elements::Element;
pub use event::Event;
pub use pathfinder_color as color;
// Keep `geometry` as its own public module alias alongside `color`.
pub use pathfinder_geometry as geometry;
#[cfg(not(feature = "tui"))]
pub use presenter::{
    AfterLayoutContext, EventContext, LayoutContext, PaintContext, Presenter, SizeConstraint,
};
#[cfg(not(feature = "tui"))]
pub use scene::{ClipBounds, Scene};
pub use zoom::ZoomFactor;

pub use crate::core::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gradient {
    pub start: color::ColorU,
    pub end: color::ColorU,
}
