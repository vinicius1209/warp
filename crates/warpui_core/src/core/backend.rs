//! The `Backend` marker trait: the remaining parameterization seam between the
//! GUI and TUI builds.
//!
//! After the view-trait unification there is a single [`View`](crate::View) /
//! [`AnyView`](crate::AnyView) pair shared by both backends, so `Backend` no
//! longer carries a type-erased view object. What still differs per backend is:
//!
//! - [`RenderOutput`](Self::RenderOutput): what a view renders to. The public
//!   [`RenderOutput`](crate::RenderOutput) alias projects this through the
//!   active backend, so view code never names the backend.
//! - [`Presenter`](Self::Presenter): the backend's presentation state, stored
//!   on [`AppContextImpl<B>`](super::AppContextImpl) as `B::Presenter`.

#[cfg(not(feature = "tui"))]
mod gui;
#[cfg(feature = "tui")]
mod tui;

#[cfg(not(feature = "tui"))]
pub use gui::GuiPresenterState;
#[cfg(feature = "tui")]
pub use tui::TuiBackend;

/// Marker trait selecting a UI backend (GUI or TUI).
pub trait Backend: Sized + 'static {
    /// What a view of this backend renders to. GUI: `Box<dyn Element>`.
    type RenderOutput;

    /// The backend's presentation layer (lays out + paints a window's view tree)
    /// plus the bookkeeping that drives it. GUI: [`GuiPresenterState`].
    ///
    /// Hoisted here so the generic core stores `B::Presenter` and never names a
    /// backend-specific concrete presentation type; the GUI presenter API is
    /// reached only through methods on the `AppContext` alias whose signatures
    /// resolve through this associated type.
    ///
    /// `Default` lets the generic constructor build the presentation state
    /// without naming the concrete type.
    type Presenter: Default;
}

/// The GUI backend marker.
pub struct GuiBackend;
