//! Body view for the "Start Mission" modal: pick a project directory and a
//! mission template, write the briefing, and kick off the mission.
//!
//! The workspace wraps this in a [`crate::modal::Modal`] and calls
//! [`StartMissionModal::on_open`] / [`StartMissionModal::on_close`] around the
//! modal's visibility (same hosting pattern as
//! [`crate::tab_configs::params_modal::TabConfigParamsModal`]).

use std::path::Path;

use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warp_editor::editor::NavigationKey;
use warpui::elements::{
    Border, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Flex,
    Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::keymap::macros::*;
use warpui::keymap::{FixedBinding, Keystroke};
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, FocusContext, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use crate::appearance::Appearance;
use crate::editor::{
    EditorOptions, EditorView, EnterAction, EnterSettings, Event as EditorEvent,
    PropagateAndNoOpNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::missions::templates::{load_mission_templates, MissionTemplate};
use crate::modal::ModalAction;
use crate::view_components::action_button::{
    ActionButton, KeystrokeSource, NakedTheme, PrimaryTheme,
};
use crate::view_components::dropdown::{Dropdown, DropdownItem};

/// Registers fixed keybindings for the Start Mission modal.
pub fn init(app: &mut AppContext) {
    app.register_fixed_bindings(vec![
        FixedBinding::new(
            "escape",
            StartMissionModalAction::Escape,
            id!("StartMissionModal"),
        ),
        // Enter submits only while no EditorView descendant is focused — the
        // briefing textarea uses Enter to insert newlines, and the directory
        // editor handles submit via its Enter event subscription instead.
        FixedBinding::new(
            "enter",
            StartMissionModalAction::Submit,
            id!("StartMissionModal") & !id!("EditorView"),
        ),
        // Cmd-enter always submits, regardless of which field is focused.
        FixedBinding::new(
            "cmdorctrl-enter",
            StartMissionModalAction::Submit,
            id!("StartMissionModal"),
        ),
    ]);
}

const CONTENT_HORIZONTAL_PADDING: f32 = 24.;
const SECTION_GAP: f32 = 16.;
const LABEL_BOTTOM_MARGIN: f32 = 4.;
const HEADER_TITLE_FONT_SIZE: f32 = 16.;
const ERROR_FONT_SIZE: f32 = 12.;
const BRIEFING_EDITOR_HEIGHT: f32 = 110.;
const DROPDOWN_WIDTH: f32 = 412.;
const DIR_PLACEHOLDER: &str = "~/projetos/meu-projeto";
const BRIEFING_PLACEHOLDER: &str = "Descreva o que você quer que a missão realize…";

pub struct StartMissionModal {
    dir_editor: ViewHandle<EditorView>,
    briefing_editor: ViewHandle<EditorView>,
    template_dropdown: ViewHandle<Dropdown<StartMissionModalAction>>,
    /// Templates loaded from disk when the modal opens.
    templates: Vec<MissionTemplate>,
    selected_template_index: usize,
    /// Inline validation error shown above the footer.
    error: Option<String>,
    cancel_button: ViewHandle<ActionButton>,
    submit_button: ViewHandle<ActionButton>,
    close_button_mouse_state: MouseStateHandle,
}

pub enum StartMissionModalEvent {
    Confirmed {
        /// Tilde-expanded, validated project directory.
        project_dir: String,
        template: MissionTemplate,
        briefing: String,
    },
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StartMissionModalAction {
    Cancel,
    Submit,
    Escape,
    /// Dispatched by the template dropdown when an item is selected.
    SelectTemplate(usize),
}

impl StartMissionModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let text_options = TextOptions::ui_font_size(Appearance::as_ref(ctx));

        let dir_editor = ctx.add_typed_action_view(|ctx| {
            let options = SingleLineEditorOptions {
                text: text_options,
                propagate_and_no_op_vertical_navigation_keys:
                    PropagateAndNoOpNavigationKeys::Always,
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text(DIR_PLACEHOLDER, ctx);
            editor
        });
        ctx.subscribe_to_view(&dir_editor, |me, _, event, ctx| {
            me.handle_dir_editor_event(event, ctx);
        });

        let text_options = TextOptions::ui_font_size(Appearance::as_ref(ctx));
        let briefing_editor = ctx.add_typed_action_view(|ctx| {
            let options = EditorOptions {
                soft_wrap: true,
                text: text_options,
                // Enter inserts a newline in the briefing; submit is via
                // cmd-enter (see the editor event subscription and `init`).
                enter_settings: EnterSettings {
                    enter: EnterAction::InsertNewLineIfMultiLine,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut editor = EditorView::new(options, ctx);
            editor.set_placeholder_text(BRIEFING_PLACEHOLDER, ctx);
            editor
        });
        ctx.subscribe_to_view(&briefing_editor, |me, _, event, ctx| {
            me.handle_briefing_editor_event(event, ctx);
        });

        let template_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(DROPDOWN_WIDTH);
            dropdown.set_menu_width(DROPDOWN_WIDTH, ctx);
            dropdown
        });

        let cancel_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Cancel", NakedTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(StartMissionModalAction::Cancel);
            })
        });
        let submit_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Start Mission", PrimaryTheme)
                .with_keybinding(
                    KeystrokeSource::Fixed(Keystroke::parse("enter").unwrap_or_default()),
                    ctx,
                )
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(StartMissionModalAction::Submit);
                })
        });

        Self {
            dir_editor,
            briefing_editor,
            template_dropdown,
            templates: Vec::new(),
            selected_template_index: 0,
            error: None,
            cancel_button,
            submit_button,
            close_button_mouse_state: Default::default(),
        }
    }

    /// Called by the workspace before making the modal visible. `initial_dir`
    /// prefills the project directory field (the active session's cwd).
    pub fn on_open(&mut self, initial_dir: Option<String>, ctx: &mut ViewContext<Self>) {
        self.templates = load_mission_templates();
        self.selected_template_index = 0;
        self.error = None;

        let items: Vec<DropdownItem<StartMissionModalAction>> = self
            .templates
            .iter()
            .enumerate()
            .map(|(i, template)| {
                // No tooltip: the selected template's description renders
                // below the dropdown, and the tooltip overflows the modal.
                DropdownItem::new(
                    template.name.clone(),
                    StartMissionModalAction::SelectTemplate(i),
                )
            })
            .collect();
        let has_templates = !items.is_empty();
        self.template_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_items(items, ctx);
            if has_templates {
                dropdown.set_selected_by_index(0, ctx);
            } else {
                dropdown.set_selected_to_none(ctx);
            }
        });

        self.dir_editor.update(ctx, |editor, ctx| {
            match &initial_dir {
                Some(dir) if !dir.is_empty() => editor.system_reset_buffer_text(dir, ctx),
                _ => editor.clear_buffer_and_reset_undo_stack(ctx),
            }
            ctx.notify();
        });
        self.briefing_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });

        ctx.focus(&self.dir_editor);
        ctx.notify();
    }

    /// Called by the workspace when the modal is dismissed.
    pub fn on_close(&mut self, ctx: &mut ViewContext<Self>) {
        self.error = None;
        ctx.notify();
    }

    fn handle_dir_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Navigate(NavigationKey::Tab) => ctx.focus(&self.briefing_editor),
            EditorEvent::Navigate(NavigationKey::ShiftTab) => ctx.focus(&self.briefing_editor),
            EditorEvent::Enter | EditorEvent::CmdEnter => self.try_submit(ctx),
            EditorEvent::Escape => ctx.emit(StartMissionModalEvent::Closed),
            EditorEvent::Edited(_) => {
                self.error = None;
                ctx.notify();
            }
            _ => {}
        }
    }

    fn handle_briefing_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::CmdEnter => self.try_submit(ctx),
            EditorEvent::Escape => ctx.emit(StartMissionModalEvent::Closed),
            EditorEvent::Edited(_) => {
                self.error = None;
                ctx.notify();
            }
            _ => {}
        }
    }

    fn try_submit(&mut self, ctx: &mut ViewContext<Self>) {
        let dir_text = self.dir_editor.as_ref(ctx).buffer_text(ctx);
        let dir_text = dir_text.trim();
        if dir_text.is_empty() {
            self.error = Some("Enter a project directory.".to_string());
            ctx.notify();
            return;
        }
        let expanded = shellexpand::tilde(dir_text).into_owned();
        if !Path::new(&expanded).is_dir() {
            self.error = Some(format!("Not an existing directory: {expanded}"));
            ctx.notify();
            return;
        }

        let briefing = self.briefing_editor.as_ref(ctx).buffer_text(ctx);
        let briefing = briefing.trim();
        if briefing.is_empty() {
            self.error = Some("Enter a briefing for the mission.".to_string());
            ctx.notify();
            return;
        }

        let Some(template) = self.templates.get(self.selected_template_index) else {
            self.error = Some("No mission template selected.".to_string());
            ctx.notify();
            return;
        };

        ctx.emit(StartMissionModalEvent::Confirmed {
            project_dir: expanded,
            template: template.clone(),
            briefing: briefing.to_string(),
        });
    }

    fn render_section_label(
        text: &str,
        top_margin: f32,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Text::new_inline(
                text.to_string(),
                appearance.ui_font_family(),
                appearance.ui_font_size(),
            )
            .with_color(theme.active_ui_text_color().into())
            .finish(),
        )
        .with_margin_top(top_margin)
        .with_margin_bottom(LABEL_BOTTOM_MARGIN)
        .finish()
    }
}

impl Entity for StartMissionModal {
    type Event = StartMissionModalEvent;
}

impl View for StartMissionModal {
    fn ui_name() -> &'static str {
        "StartMissionModal"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        // When focus arrives directly at this view (e.g. the Modal wrapper
        // re-focuses the body after the dropdown closes), send it to the
        // directory field so the user can keep typing.
        if focus_ctx.is_self_focused() {
            ctx.focus(&self.dir_editor);
        }
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let sub_text = theme.sub_text_color(theme.background());

        // ── Header ───────────────────────────────────────────────────────
        let header = {
            let title = Text::new_inline(
                "Start Mission".to_string(),
                appearance.ui_font_family(),
                HEADER_TITLE_FONT_SIZE,
            )
            .with_color(theme.active_ui_text_color().into())
            .with_style(Properties::default().weight(Weight::Bold))
            .finish();

            let esc_badge = Container::new(
                ConstrainedBox::new(
                    Text::new_inline("ESC".to_string(), appearance.ui_font_family(), 10.)
                        .with_color(theme.foreground().into())
                        .finish(),
                )
                .with_height(14.)
                .finish(),
            )
            .with_horizontal_padding(2.)
            .with_background(internal_colors::neutral_2(theme))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
            .finish();

            let close_icon = ConstrainedBox::new(Icon::X.to_warpui_icon(sub_text).finish())
                .with_width(14.)
                .with_height(14.)
                .finish();

            let close_button = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(2.)
                .with_child(close_icon)
                .with_child(esc_badge)
                .finish();

            let close_hoverable =
                Hoverable::new(self.close_button_mouse_state.clone(), move |_state| {
                    close_button
                })
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(ModalAction::Close);
                })
                .with_cursor(Cursor::PointingHand)
                .finish();

            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(Shrinkable::new(1., title).finish())
                    .with_child(close_hoverable)
                    .finish(),
            )
            .with_padding(
                Padding::uniform(0.)
                    .with_top(24.)
                    .with_bottom(12.)
                    .with_left(CONTENT_HORIZONTAL_PADDING)
                    .with_right(CONTENT_HORIZONTAL_PADDING),
            )
            .finish()
        };

        // ── Form body ────────────────────────────────────────────────────
        let mut form = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        form.add_child(Self::render_section_label(
            "Project directory",
            0.,
            appearance,
        ));
        form.add_child(
            appearance
                .ui_builder()
                .text_input(self.dir_editor.clone())
                .build()
                .finish(),
        );

        form.add_child(Self::render_section_label(
            "Template",
            SECTION_GAP,
            appearance,
        ));
        form.add_child(ChildView::new(&self.template_dropdown).finish());
        if let Some(template) = self.templates.get(self.selected_template_index) {
            if !template.description.is_empty() {
                form.add_child(
                    Container::new(
                        Text::new_inline(
                            template.description.clone(),
                            appearance.ui_font_family(),
                            appearance.ui_font_size() - 1.,
                        )
                        .with_color(sub_text.into())
                        .finish(),
                    )
                    .with_margin_top(LABEL_BOTTOM_MARGIN)
                    .finish(),
                );
            }
        }

        form.add_child(Self::render_section_label(
            "Briefing",
            SECTION_GAP,
            appearance,
        ));
        form.add_child(
            appearance
                .ui_builder()
                .text_input(self.briefing_editor.clone())
                .with_style(UiComponentStyles {
                    height: Some(BRIEFING_EDITOR_HEIGHT),
                    ..Default::default()
                })
                .build()
                .finish(),
        );

        if let Some(error) = &self.error {
            form.add_child(
                Container::new(
                    Text::new_inline(error.clone(), appearance.ui_font_family(), ERROR_FONT_SIZE)
                        .with_color(theme.ui_error_color())
                        .finish(),
                )
                .with_margin_top(LABEL_BOTTOM_MARGIN)
                .finish(),
            );
        }

        let body_container = Container::new(form.finish())
            .with_padding(
                Padding::uniform(0.)
                    .with_left(CONTENT_HORIZONTAL_PADDING)
                    .with_right(CONTENT_HORIZONTAL_PADDING)
                    .with_bottom(16.),
            )
            .finish();

        // ── Footer ───────────────────────────────────────────────────────
        let button_row = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(ChildView::new(&self.cancel_button).finish())
                .with_child(ChildView::new(&self.submit_button).finish())
                .finish(),
        )
        .with_padding(
            Padding::uniform(12.)
                .with_left(CONTENT_HORIZONTAL_PADDING)
                .with_right(CONTENT_HORIZONTAL_PADDING),
        )
        .finish();

        let footer = Container::new(button_row)
            .with_border(Border::top(1.).with_border_fill(theme.outline()))
            .finish();

        // ── Assemble ─────────────────────────────────────────────────────
        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header)
            .with_child(body_container)
            .with_child(footer)
            .finish()
    }
}

impl TypedActionView for StartMissionModal {
    type Action = StartMissionModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            StartMissionModalAction::Cancel | StartMissionModalAction::Escape => {
                ctx.emit(StartMissionModalEvent::Closed);
            }
            StartMissionModalAction::Submit => self.try_submit(ctx),
            StartMissionModalAction::SelectTemplate(index) => {
                self.selected_template_index = *index;
                self.error = None;
                ctx.notify();
            }
        }
    }
}
