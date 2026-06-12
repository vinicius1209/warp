//! Confirmation dialog shown between mission stages (the human "gate").
//!
//! Follows the design pattern of
//! [`crate::workspace::rewind_confirmation_dialog::RewindConfirmationDialog`]:
//! a centered [`Dialog`] over a blurred backdrop, hosted by the workspace with
//! Confirm/Cancel actions re-emitted as events.

use warpui::elements::{
    Align, ChildAnchor, Container, MouseStateHandle, OffsetPositioning, ParentAnchor,
    ParentOffsetBounds, Stack,
};
use warpui::fonts::Weight;
use warpui::geometry::vector::vec2f;
use warpui::keymap::macros::*;
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use warp_core::ui::theme::Fill;

use crate::appearance::Appearance;
use crate::ui_components::dialog::{dialog_styles, Dialog};

/// Registers fixed keybindings for the mission gate dialog.
pub fn init(app: &mut AppContext) {
    app.register_fixed_bindings([
        FixedBinding::new(
            "escape",
            MissionGateDialogAction::Cancel,
            id!(MissionGateDialog::ui_name()),
        ),
        FixedBinding::new(
            "enter",
            MissionGateDialogAction::Confirm,
            id!(MissionGateDialog::ui_name()),
        ),
    ]);
}

const DIALOG_WIDTH: f32 = 460.;

pub enum MissionGateDialogEvent {
    Confirm,
    Cancel,
}

#[derive(Debug)]
pub enum MissionGateDialogAction {
    Confirm,
    Cancel,
}

pub struct MissionGateDialog {
    cancel_mouse_state: MouseStateHandle,
    confirm_mouse_state: MouseStateHandle,
    /// The rendered gate text for the stage awaiting confirmation. `None`
    /// until the dialog is first opened.
    message: Option<String>,
}

impl Default for MissionGateDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl MissionGateDialog {
    pub fn new() -> Self {
        Self {
            cancel_mouse_state: Default::default(),
            confirm_mouse_state: Default::default(),
            message: None,
        }
    }

    /// Called by the workspace before showing the dialog.
    pub fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }
}

impl Entity for MissionGateDialog {
    type Event = MissionGateDialogEvent;
}

impl View for MissionGateDialog {
    fn ui_name() -> &'static str {
        "MissionGateDialog"
    }

    fn on_focus(&mut self, _focus_ctx: &warpui::FocusContext, ctx: &mut ViewContext<Self>) {
        ctx.focus_self();
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);

        let button_style = UiComponentStyles {
            font_size: Some(14.),
            font_weight: Some(Weight::Bold),
            height: Some(40.),
            padding: Some(Coords {
                left: 16.,
                right: 16.,
                top: 0.,
                bottom: 0.,
            }),
            ..Default::default()
        };

        let confirm_button = appearance
            .ui_builder()
            .button(ButtonVariant::Accent, self.confirm_mouse_state.clone())
            .with_centered_text_label("Continue".into())
            .with_style(button_style)
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(MissionGateDialogAction::Confirm))
            .finish();

        let cancel_button = appearance
            .ui_builder()
            .button(ButtonVariant::Basic, self.cancel_mouse_state.clone())
            .with_centered_text_label("Cancel".into())
            .with_style(button_style)
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(MissionGateDialogAction::Cancel))
            .finish();

        let dialog = Dialog::new(
            "Advance mission?".into(),
            self.message.clone(),
            UiComponentStyles {
                width: Some(DIALOG_WIDTH),
                padding: Some(Coords::uniform(24.)),
                ..dialog_styles(appearance)
            },
        )
        .with_bottom_row_child(cancel_button)
        .with_bottom_row_child(confirm_button)
        .build()
        .finish();

        // Stack needed so that the dialog can get bounds information.
        let mut stack = Stack::new();
        stack.add_positioned_child(
            dialog,
            OffsetPositioning::offset_from_parent(
                vec2f(0., 0.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::Center,
                ChildAnchor::Center,
            ),
        );

        // This blurs the background and makes it uninteractable.
        Container::new(Align::new(stack.finish()).finish())
            .with_background_color(Fill::blur().into())
            .with_corner_radius(app.windows().window_corner_radius())
            .finish()
    }
}

impl TypedActionView for MissionGateDialog {
    type Action = MissionGateDialogAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            MissionGateDialogAction::Confirm => ctx.emit(MissionGateDialogEvent::Confirm),
            MissionGateDialogAction::Cancel => ctx.emit(MissionGateDialogEvent::Cancel),
        }
    }
}
