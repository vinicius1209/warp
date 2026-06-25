//! Confirmation dialog shown between mission stages (the human "gate").
//!
//! Follows the design pattern of
//! [`crate::workspace::rewind_confirmation_dialog::RewindConfirmationDialog`]:
//! a centered [`Dialog`] over a blurred backdrop, hosted by the workspace with
//! Confirm/Cancel actions re-emitted as events.

use warpui::elements::{
    Align, Border, ChildAnchor, ChildView, ConstrainedBox, Container, CrossAxisAlignment, Flex,
    MainAxisSize, MouseStateHandle, OffsetPositioning, ParentAnchor, ParentElement,
    ParentOffsetBounds, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::geometry::vector::vec2f;
use warpui::keymap::macros::*;
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use warp_core::ui::theme::Fill;

use crate::appearance::Appearance;
use crate::editor::{EditorOptions, EditorView, EnterAction, EnterSettings, TextOptions};
use crate::missions::spec::CriteriaProgress;
use crate::ui_components::dialog::{dialog_styles, Dialog};

/// Registers fixed keybindings for the mission gate dialog.
pub fn init(app: &mut AppContext) {
    app.register_fixed_bindings([
        FixedBinding::new(
            "escape",
            MissionGateDialogAction::Cancel,
            id!(MissionGateDialog::ui_name()),
        ),
        // Enter confirms only while no EditorView is focused — the feedback
        // field uses Enter to insert newlines.
        FixedBinding::new(
            "enter",
            MissionGateDialogAction::Confirm,
            id!(MissionGateDialog::ui_name()) & !id!("EditorView"),
        ),
    ]);
}

const DIALOG_WIDTH: f32 = 460.;
/// Wider variant used when the gate shows a spec review block.
const DIALOG_WIDTH_WITH_SPEC: f32 = 560.;
const FEEDBACK_EDITOR_HEIGHT: f32 = 72.;
const FEEDBACK_PLACEHOLDER: &str =
    "Optional: describe what to change — the stage re-runs with this note.";

pub enum MissionGateDialogEvent {
    Confirm,
    Cancel,
    /// Re-run the current stage with the carried feedback (empty = plain redo).
    Revise(String),
}

#[derive(Debug)]
pub enum MissionGateDialogAction {
    Confirm,
    Cancel,
    Revise,
}

pub struct MissionGateDialog {
    cancel_mouse_state: MouseStateHandle,
    confirm_mouse_state: MouseStateHandle,
    request_changes_mouse_state: MouseStateHandle,
    /// Multi-line editor for the optional "request changes" feedback, re-run
    /// into the current stage's prompt when the user picks Request changes.
    feedback_editor: ViewHandle<EditorView>,
    /// The rendered gate text for the stage awaiting confirmation. `None`
    /// until the dialog is first opened.
    message: Option<String>,
    /// Bounded preview of the just-finished stage's spec (`spec.md`), shown so
    /// the human can review the artifact before approving. `None` hides the
    /// whole review block (e.g. before any spec exists).
    spec_preview: Option<String>,
    /// Acceptance-criteria tally for the spec preview. `None`, or empty, hides
    /// the "N/M criteria met" line while still showing the preview.
    criteria: Option<CriteriaProgress>,
}

impl MissionGateDialog {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let text_options = TextOptions::ui_font_size(Appearance::as_ref(ctx));
        let feedback_editor = ctx.add_typed_action_view(|ctx| {
            let options = EditorOptions {
                soft_wrap: true,
                text: text_options,
                // Enter inserts a newline; the user submits via Request changes.
                enter_settings: EnterSettings {
                    enter: EnterAction::InsertNewLineIfMultiLine,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut editor = EditorView::new(options, ctx);
            editor.set_placeholder_text(FEEDBACK_PLACEHOLDER, ctx);
            editor
        });
        Self {
            cancel_mouse_state: Default::default(),
            confirm_mouse_state: Default::default(),
            request_changes_mouse_state: Default::default(),
            feedback_editor,
            message: None,
            spec_preview: None,
            criteria: None,
        }
    }

    /// Called by the workspace before showing the dialog.
    pub fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }

    /// Sets (or clears) the spec review context shown beneath the gate message:
    /// a bounded `spec.md` preview and its acceptance-criteria tally. The
    /// workspace calls this on every gate open — passing `None` for the preview
    /// when the stage has no spec — so a stale preview never leaks across gates.
    pub fn set_review_context(
        &mut self,
        spec_preview: Option<String>,
        criteria: Option<CriteriaProgress>,
    ) {
        self.spec_preview = spec_preview;
        self.criteria = criteria;
    }

    /// Clears the feedback editor. Called by the workspace on every gate open so
    /// a previous gate's feedback never leaks into the next stage.
    pub fn reset_feedback(&mut self, ctx: &mut ViewContext<Self>) {
        self.feedback_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });
    }

    /// Builds the "Request changes" feedback field shown below the review block.
    fn render_feedback_field(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(6.)
            .with_child(
                Text::new_inline(
                    "Request changes (optional)".to_string(),
                    appearance.ui_font_family(),
                    13.,
                )
                .with_color(theme.active_ui_text_color().into())
                .with_style(Properties::default().weight(Weight::Bold))
                .finish(),
            )
            .with_child(
                Container::new(
                    ConstrainedBox::new(ChildView::new(&self.feedback_editor).finish())
                        .with_height(FEEDBACK_EDITOR_HEIGHT)
                        .finish(),
                )
                .with_vertical_padding(6.)
                .with_horizontal_padding(8.)
                .with_border(Border::all(1.).with_border_fill(theme.outline()))
                .finish(),
            )
            .finish()
    }

    /// Builds the spec review block (criteria tally + bounded excerpt) shown
    /// between the gate message and the buttons, or `None` when there's no spec.
    fn render_review_block(&self, appearance: &Appearance) -> Option<Box<dyn Element>> {
        let preview = self.spec_preview.as_ref()?;
        let theme = appearance.theme();
        let sub_text = theme.sub_text_color(theme.background());

        let header = match self.criteria.filter(|criteria| !criteria.is_empty()) {
            Some(criteria) => format!("Spec — {}/{} criteria met", criteria.met, criteria.total),
            None => "Spec".to_string(),
        };

        let excerpt_box = Container::new(
            Text::new(preview.clone(), appearance.ui_font_family(), 12.)
                .with_color(sub_text.into())
                .finish(),
        )
        .with_vertical_padding(8.)
        .with_horizontal_padding(10.)
        .with_border(Border::all(1.).with_border_fill(theme.outline()))
        .finish();

        Some(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(6.)
                .with_child(
                    Text::new_inline(header, appearance.ui_font_family(), 13.)
                        .with_color(theme.active_ui_text_color().into())
                        .with_style(Properties::default().weight(Weight::Bold))
                        .finish(),
                )
                .with_child(excerpt_box)
                .finish(),
        )
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

        let request_changes_button = appearance
            .ui_builder()
            .button(ButtonVariant::Basic, self.request_changes_mouse_state.clone())
            .with_centered_text_label("Request changes".into())
            .with_style(button_style)
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(MissionGateDialogAction::Revise))
            .finish();

        // Widen the dialog when a spec preview is present so the excerpt has
        // room to breathe; otherwise keep the compact confirmation width.
        let review_block = self.render_review_block(appearance);
        let width = if review_block.is_some() {
            DIALOG_WIDTH_WITH_SPEC
        } else {
            DIALOG_WIDTH
        };

        // The Dialog takes a single body child, so combine the optional review
        // block and the request-changes field into one column.
        let mut body = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(16.);
        if let Some(review_block) = review_block {
            body.add_child(review_block);
        }
        body.add_child(self.render_feedback_field(appearance));

        let dialog = Dialog::new(
            "Advance mission?".into(),
            self.message.clone(),
            UiComponentStyles {
                width: Some(width),
                padding: Some(Coords::uniform(24.)),
                ..dialog_styles(appearance)
            },
        )
        .with_child(body.finish())
        .with_bottom_row_child(cancel_button)
        .with_bottom_row_child(request_changes_button)
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
            MissionGateDialogAction::Revise => {
                let feedback = self.feedback_editor.as_ref(ctx).buffer_text(ctx);
                ctx.emit(MissionGateDialogEvent::Revise(feedback));
            }
        }
    }
}
