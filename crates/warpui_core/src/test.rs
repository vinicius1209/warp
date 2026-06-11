use ctor::ctor;

// Initialize the logger before running tests.
#[ctor]
fn init() {
    simplelog::SimpleLogger::init(simplelog::LevelFilter::Info, simplelog::Config::default())
        .unwrap()
}

/// Produces a minimal render output for the active backend. Shared by all
/// core test files so neutral tests run under both cfgs.
#[cfg(not(feature = "tui"))]
pub(crate) fn empty_render_output() -> crate::RenderOutput {
    use crate::elements::{Element, Empty};
    Empty::new().finish()
}

#[cfg(feature = "tui")]
pub(crate) fn empty_render_output() -> crate::RenderOutput {
    Box::new(())
}
