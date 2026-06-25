//! Body view for the "Start Mission" modal: pick a project directory and a
//! mission template, write the briefing, and kick off the mission.
//!
//! The workspace wraps this in a [`crate::modal::Modal`] and calls
//! [`StartMissionModal::on_open`] / [`StartMissionModal::on_close`] around the
//! modal's visibility (same hosting pattern as
//! [`crate::tab_configs::params_modal::TabConfigParamsModal`]).
//!
//! ## Project picker (Cockpit "Gap B")
//!
//! The primary way to choose a project is a visual list of recent projects
//! (sourced from [`ProjectManagementModel`], the same model the welcome palette
//! uses) plus a "Browse…" button that opens the native folders-only picker
//! (the same `ctx.open_file_picker(.., FilePickerConfiguration::new().folders_only())`
//! call used in `welcome_view.rs`). A non-developer never has to type a
//! filesystem path. A small "Enter a path manually" escape hatch remains for
//! power users and dirs that aren't in the recent list.
//!
//! ## Harness pre-flight (Cockpit "Gap C")
//!
//! The modal surfaces, inline, whether the agent that will run the mission is
//! installed (`missions::check_harness`). The launch itself is gated in
//! `Workspace::start_mission`; this is just an early, reassuring signal.

use std::path::{Path, PathBuf};

use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warp_editor::editor::NavigationKey;
use warpui::elements::{
    Border, ChildView, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
    CornerRadius, CrossAxisAlignment, Flex, Hoverable, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, Padding, ParentElement, Radius, ScrollbarWidth, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::keymap::macros::*;
use warpui::keymap::{FixedBinding, Keystroke};
use warpui::platform::{Cursor, FilePickerConfiguration};
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, FocusContext, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use warp_cli::agent::Harness;

use crate::appearance::Appearance;
use crate::editor::{
    EditorOptions, EditorView, EnterAction, EnterSettings, Event as EditorEvent,
    PropagateAndNoOpNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::missions::templates::{load_mission_templates, MissionTemplate};
use crate::missions::{check_harness, pick_default_harness, HarnessAvailability};
use crate::modal::ModalAction;
use crate::projects::ProjectManagementModel;
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
        // briefing textarea uses Enter to insert newlines, and the manual-path
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
const PROJECT_LIST_MAX_HEIGHT: f32 = 168.;
const PROJECT_ROW_VERTICAL_PADDING: f32 = 8.;
const PROJECT_ROW_CORNER_RADIUS: f32 = 6.;
const DIR_PLACEHOLDER: &str = "~/projetos/meu-projeto";
const BRIEFING_PLACEHOLDER: &str = "Descreva o que você quer que a missão realize…";

/// A recent project shown as a selectable row in the picker.
#[derive(Clone)]
struct RecentProject {
    /// Absolute path to the project directory.
    path: String,
    /// Display name: the last path component (bold in the row).
    name: String,
    /// Dimmed full path shown under the name.
    full_path: String,
}

pub struct StartMissionModal {
    /// Escape-hatch single-line editor for typing a path manually. Hidden by
    /// default; the project list is the primary surface.
    dir_editor: ViewHandle<EditorView>,
    briefing_editor: ViewHandle<EditorView>,
    template_dropdown: ViewHandle<Dropdown<StartMissionModalAction>>,
    /// Templates loaded from disk when the modal opens.
    templates: Vec<MissionTemplate>,
    selected_template_index: usize,
    /// Recent projects loaded from [`ProjectManagementModel`] on open.
    recent_projects: Vec<RecentProject>,
    /// Long-lived hover/click state per project row. Sized to match
    /// `recent_projects` in [`Self::on_open`] so these handles are never
    /// recreated per render (recreating them silently breaks hover/click —
    /// see `.cockpit/COCKPIT.md` rule 2).
    project_row_mouse_states: Vec<MouseStateHandle>,
    /// Scroll state for the (possibly long) project list.
    project_list_scroll_state: ClippedScrollStateHandle,
    /// The currently selected project directory (absolute path).
    selected_dir: Option<String>,
    /// When true, the manual-path editor is shown instead of the picker list.
    manual_path_mode: bool,
    /// Inline validation error shown above the footer.
    error: Option<String>,
    browse_button: ViewHandle<ActionButton>,
    manual_toggle_mouse_state: MouseStateHandle,
    cancel_button: ViewHandle<ActionButton>,
    submit_button: ViewHandle<ActionButton>,
    close_button_mouse_state: MouseStateHandle,
    /// The auto-selected default harness (config name) + its availability,
    /// computed once in [`Self::on_open`]. Cached because `check_harness`
    /// walks `PATH` (filesystem syscalls) — never recompute it per render.
    default_harness: String,
    agent_availability: Option<HarnessAvailability>,
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
    /// Selects the recent project at this index in `recent_projects`.
    SelectProject(usize),
    /// Opens the native folders-only directory picker.
    Browse,
    /// Carries a directory the user picked via the native picker or another
    /// out-of-render path (dispatched back to this view from a picker callback).
    PathPicked(String),
    /// Toggles between the visual picker and the manual-path escape hatch.
    ToggleManualPath,
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

        let browse_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Browse…", NakedTheme)
                .with_icon(Icon::Folder)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(StartMissionModalAction::Browse);
                })
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
            recent_projects: Vec::new(),
            project_row_mouse_states: Vec::new(),
            project_list_scroll_state: ClippedScrollStateHandle::new(),
            selected_dir: None,
            manual_path_mode: false,
            error: None,
            browse_button,
            manual_toggle_mouse_state: Default::default(),
            cancel_button,
            submit_button,
            close_button_mouse_state: Default::default(),
            default_harness: String::new(),
            agent_availability: None,
        }
    }

    /// Called by the workspace before making the modal visible. `initial_dir`
    /// (the active session's cwd) preselects a project when it matches a known
    /// one, or seeds the selection directly.
    pub fn on_open(&mut self, initial_dir: Option<String>, ctx: &mut ViewContext<Self>) {
        self.templates = load_mission_templates();
        self.selected_template_index = 0;
        self.error = None;
        self.manual_path_mode = false;

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

        self.reload_recent_projects(ctx);

        // Resolve the agent availability once here (it walks PATH); render
        // reads the cache so no filesystem syscalls run per frame.
        let default_harness = pick_default_harness();
        self.agent_availability = Some(check_harness(&default_harness));
        self.default_harness = default_harness;

        // Seed the selection from the active session cwd, if any.
        self.selected_dir = match &initial_dir {
            Some(dir) if !dir.trim().is_empty() => Some(dir.trim().to_string()),
            _ => None,
        };
        // Keep the manual-path editor in sync so power users see the value if
        // they flip to manual mode.
        let seed = self.selected_dir.clone().unwrap_or_default();
        self.dir_editor.update(ctx, |editor, ctx| {
            if seed.is_empty() {
                editor.clear_buffer_and_reset_undo_stack(ctx);
            } else {
                editor.system_reset_buffer_text(&seed, ctx);
            }
            ctx.notify();
        });
        self.briefing_editor.update(ctx, |editor, ctx| {
            editor.clear_buffer_and_reset_undo_stack(ctx);
        });

        ctx.focus_self();
        ctx.notify();
    }

    /// Called by the workspace when the modal is dismissed.
    pub fn on_close(&mut self, ctx: &mut ViewContext<Self>) {
        self.error = None;
        ctx.notify();
    }

    /// Loads recent projects from the shared [`ProjectManagementModel`] (most
    /// recently used first) and resizes the per-row mouse-state handle vector to
    /// match — the handles are long-lived, never recreated during render.
    fn reload_recent_projects(&mut self, ctx: &mut ViewContext<Self>) {
        let mut projects: Vec<(String, chrono::NaiveDateTime)> =
            ProjectManagementModel::handle(ctx)
                .as_ref(ctx)
                .all_projects()
                .filter(|project| Path::new(&project.path).is_dir())
                .map(|project| (project.path.clone(), project.last_used_at()))
                .collect();
        // Most-recently-used first.
        projects.sort_by(|a, b| b.1.cmp(&a.1));

        self.recent_projects = projects
            .into_iter()
            .map(|(path, _)| {
                let name = Path::new(&path)
                    .file_name()
                    .map(|component| component.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                RecentProject {
                    name,
                    full_path: abbreviate_home(&path),
                    path,
                }
            })
            .collect();

        // Resize the long-lived mouse-state vector to one handle per row.
        self.project_row_mouse_states
            .resize_with(self.recent_projects.len(), MouseStateHandle::default);
    }

    fn handle_dir_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        match event {
            EditorEvent::Navigate(NavigationKey::Tab) => ctx.focus(&self.briefing_editor),
            EditorEvent::Navigate(NavigationKey::ShiftTab) => ctx.focus(&self.briefing_editor),
            EditorEvent::Enter | EditorEvent::CmdEnter => self.try_submit(ctx),
            EditorEvent::Escape => ctx.emit(StartMissionModalEvent::Closed),
            EditorEvent::Edited(_) => {
                // In manual mode the typed path is the selection.
                let text = self.dir_editor.as_ref(ctx).buffer_text(ctx);
                let trimmed = text.trim();
                self.selected_dir = (!trimmed.is_empty()).then(|| trimmed.to_string());
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

    /// Opens the native folders-only directory picker (same API as
    /// `welcome_view.rs` / `project_buttons.rs`). On pick, the chosen dir is
    /// dispatched back to this view as [`StartMissionModalAction::PathPicked`].
    fn open_directory_picker(&mut self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        let view_id = ctx.view_id();
        ctx.open_file_picker(
            move |result, ctx| {
                if let Ok(paths) = result {
                    if let Some(path) = paths.into_iter().next() {
                        ctx.dispatch_typed_action_for_view(
                            window_id,
                            view_id,
                            &StartMissionModalAction::PathPicked(path),
                        );
                    }
                }
            },
            FilePickerConfiguration::new().folders_only(),
        );
    }

    /// Records a newly selected/picked directory and remembers it as a recent
    /// project so it appears in the list next time.
    fn select_dir(&mut self, dir: String, ctx: &mut ViewContext<Self>) {
        let dir = dir.trim().to_string();
        if dir.is_empty() {
            return;
        }
        // Persist to the recent-projects model (mirrors `welcome_view.rs`).
        ProjectManagementModel::handle(ctx).update(ctx, |projects, ctx| {
            projects.upsert_project(PathBuf::from(&dir), ctx);
        });
        self.selected_dir = Some(dir);
        self.error = None;
        self.reload_recent_projects(ctx);
        ctx.notify();
    }

    fn try_submit(&mut self, ctx: &mut ViewContext<Self>) {
        // In manual mode the source of truth is the editor; otherwise it's the
        // selected project row.
        let dir_text = if self.manual_path_mode {
            self.dir_editor
                .as_ref(ctx)
                .buffer_text(ctx)
                .trim()
                .to_string()
        } else {
            self.selected_dir.clone().unwrap_or_default()
        };
        if dir_text.is_empty() {
            self.error = Some("Pick a project to work in.".to_string());
            ctx.notify();
            return;
        }
        let expanded = shellexpand::tilde(&dir_text).into_owned();
        if !Path::new(&expanded).is_dir() {
            self.error = Some(format!("That folder doesn't exist: {expanded}"));
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

    /// Renders one selectable project row (bold name + dimmed path), highlighted
    /// when it is the current selection.
    fn render_project_row(&self, index: usize, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let project = &self.recent_projects[index];
        let is_selected = self.selected_dir.as_deref() == Some(project.path.as_str());
        let mouse_state = self.project_row_mouse_states[index].clone();
        let action = StartMissionModalAction::SelectProject(index);

        let name = project.name.clone();
        let full_path = project.full_path.clone();
        let ui_font_family = appearance.ui_font_family();
        let ui_font_size = appearance.ui_font_size();
        let active_text = theme.active_ui_text_color();
        let sub_text = theme.sub_text_color(theme.background());
        let accent = theme.accent();
        let selected_fill = theme.surface_overlay_2();
        let hover_fill = theme.surface_overlay_1();
        let background_fill = theme.background();

        Hoverable::new(mouse_state, move |state| {
            let name_el = Text::new_inline(name.clone(), ui_font_family, ui_font_size)
                .with_color(active_text.into())
                .with_style(Properties::default().weight(Weight::Bold))
                .finish();
            let path_el = Text::new_inline(full_path.clone(), ui_font_family, ui_font_size - 1.)
                .with_color(sub_text.into())
                .finish();

            let text_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(name_el)
                .with_child(Container::new(path_el).with_margin_top(2.).finish());

            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Shrinkable::new(1., text_column.finish()).finish());

            if is_selected {
                row.add_child(
                    ConstrainedBox::new(Icon::Check.to_warpui_icon(accent).finish())
                        .with_width(16.)
                        .with_height(16.)
                        .finish(),
                );
            }

            let background = if is_selected {
                selected_fill
            } else if state.is_hovered() {
                hover_fill
            } else {
                background_fill
            };

            Container::new(row.finish())
                .with_padding(
                    Padding::uniform(0.)
                        .with_top(PROJECT_ROW_VERTICAL_PADDING)
                        .with_bottom(PROJECT_ROW_VERTICAL_PADDING)
                        .with_left(10.)
                        .with_right(10.),
                )
                .with_background(background.into_solid())
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
                    PROJECT_ROW_CORNER_RADIUS,
                )))
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
    }

    /// The project picker section: a scrollable list of recent projects (or a
    /// friendly empty state) and a "Browse…" button, with a manual-path toggle.
    fn render_project_picker(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let sub_text = theme.sub_text_color(theme.background());

        let list_or_empty: Box<dyn Element> = if self.recent_projects.is_empty() {
            Container::new(
                Text::new_inline(
                    "No recent projects yet. Use Browse… to pick a folder.".to_string(),
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(sub_text.into())
                .finish(),
            )
            .with_padding(Padding::uniform(12.))
            .finish()
        } else {
            let mut list = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for index in 0..self.recent_projects.len() {
                list.add_child(self.render_project_row(index, appearance));
            }
            ConstrainedBox::new(
                ClippedScrollable::vertical(
                    self.project_list_scroll_state.clone(),
                    list.finish(),
                    ScrollbarWidth::Auto,
                    theme.nonactive_ui_detail().into(),
                    theme.active_ui_detail().into(),
                    warpui::elements::Fill::None,
                )
                .with_overlayed_scrollbar()
                .finish(),
            )
            .with_max_height(PROJECT_LIST_MAX_HEIGHT)
            .finish()
        };

        let list_container = Container::new(list_or_empty)
            .with_border(theme.outline().into_solid())
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .finish();

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(list_container);
        column.add_child(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(ChildView::new(&self.browse_button).finish())
                    .with_child(self.render_manual_toggle("Enter a path manually", appearance))
                    .finish(),
            )
            .with_margin_top(8.)
            .finish(),
        );
        column.finish()
    }

    /// The manual-path escape hatch: the single-line editor plus a toggle back
    /// to the visual picker.
    fn render_manual_path(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(
            appearance
                .ui_builder()
                .text_input(self.dir_editor.clone())
                .build()
                .finish(),
        );
        column.add_child(
            Container::new(self.render_manual_toggle("Choose from recent projects", appearance))
                .with_margin_top(8.)
                .finish(),
        );
        column.finish()
    }

    /// A small clickable text link that toggles manual-path mode.
    fn render_manual_toggle(&self, label: &str, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let accent = theme.accent();
        let label = label.to_string();
        let ui_font_family = appearance.ui_font_family();
        let ui_font_size = appearance.ui_font_size();
        Hoverable::new(self.manual_toggle_mouse_state.clone(), move |_state| {
            Text::new_inline(label.clone(), ui_font_family, ui_font_size)
                .with_color(accent.into())
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(StartMissionModalAction::ToggleManualPath)
        })
        .finish()
    }

    /// Inline "Agent: <name> ✓/✗" line for the auto-selected default harness, so
    /// the user sees up front whether the mission can run (the launch is still
    /// gated in `Workspace::start_mission`). Install detection only — no login
    /// probing.
    fn render_agent_availability(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let display_name = Harness::from_config_name(&self.default_harness)
            .map(|h| h.display_name())
            .unwrap_or("Claude Code");

        let error_fill: warp_core::ui::theme::Fill = theme.ui_error_color().into();
        let (icon, icon_color, suffix, suffix_color) = match &self.agent_availability {
            Some(HarnessAvailability::Installed) | None => (
                Icon::Check,
                theme.accent(),
                "ready".to_string(),
                theme.sub_text_color(theme.background()),
            ),
            Some(HarnessAvailability::NotInstalled { install_hint, .. }) => (
                Icon::X,
                error_fill,
                format!("not installed — {install_hint}"),
                error_fill,
            ),
            Some(HarnessAvailability::Unsupported { message, .. }) => (
                Icon::X,
                error_fill,
                format!("unsupported — {message}"),
                error_fill,
            ),
        };

        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_child(
                ConstrainedBox::new(icon.to_warpui_icon(icon_color).finish())
                    .with_width(14.)
                    .with_height(14.)
                    .finish(),
            )
            .with_child(
                Text::new_inline(
                    format!("Agent: {display_name} — {suffix}"),
                    appearance.ui_font_family(),
                    appearance.ui_font_size() - 1.,
                )
                .with_color(suffix_color.into())
                .finish(),
            )
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
        // When focus arrives directly at this view and the manual-path editor is
        // showing, send focus there so the user can keep typing. Otherwise keep
        // focus on the modal body (the picker rows handle clicks directly).
        if focus_ctx.is_self_focused() && self.manual_path_mode {
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

        form.add_child(Self::render_section_label("Project", 0., appearance));
        if self.manual_path_mode {
            form.add_child(self.render_manual_path(appearance));
        } else {
            form.add_child(self.render_project_picker(appearance));
        }

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

        // Inline agent availability (Cockpit "Gap C" — early signal).
        form.add_child(
            Container::new(self.render_agent_availability(appearance))
                .with_margin_top(SECTION_GAP)
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
            StartMissionModalAction::SelectProject(index) => {
                if let Some(project) = self.recent_projects.get(*index) {
                    self.selected_dir = Some(project.path.clone());
                    self.error = None;
                    ctx.notify();
                }
            }
            StartMissionModalAction::Browse => self.open_directory_picker(ctx),
            StartMissionModalAction::PathPicked(path) => {
                self.select_dir(path.clone(), ctx);
            }
            StartMissionModalAction::ToggleManualPath => {
                self.manual_path_mode = !self.manual_path_mode;
                self.error = None;
                if self.manual_path_mode {
                    ctx.focus(&self.dir_editor);
                }
                ctx.notify();
            }
        }
    }
}

/// Abbreviates a `$HOME`-prefixed path with `~/` for display (mirrors
/// `util::path::display_path_with_host`'s home abbreviation).
fn abbreviate_home(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = Path::new(path).strip_prefix(&home) {
            return format!("~/{}", relative.display());
        }
    }
    path.to_string()
}
