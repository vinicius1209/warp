#[cfg(not(feature = "tui"))]
mod app;
mod delegate;
#[cfg(not(feature = "tui"))]
mod gui;

#[cfg(not(feature = "tui"))]
pub use app::App;
pub(crate) use delegate::WindowManager;
pub use delegate::{AppDelegate, FontDB, IntegrationTestDelegate};
