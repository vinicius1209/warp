pub use pathfinder_color::ColorU;
pub use pathfinder_geometry::rect::RectF;
pub use pathfinder_geometry::vector::{vec2f, Vector2F};

pub use crate::core::{
    AppContext, Entity, GetSingletonModelHandle as _, ModelContext, ModelHandle, SingletonEntity,
    TypedActionView, View, ViewContext, ViewHandle,
};
pub use crate::platform::Cursor;

#[cfg(not(feature = "tui"))]
mod gui;
#[cfg(not(feature = "tui"))]
pub use gui::*;

// The tui prelude is an empty stub until M8 lands the in-core TUI element
// library; M8 adds `pub use tui::*;` once it has content (a glob re-export of
// an empty module trips `unused_imports`).
#[cfg(feature = "tui")]
mod tui;
