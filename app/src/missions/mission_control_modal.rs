//! Body view for the "Mission Control" modal: an overview of every active
//! mission with per-mission Resume / Next Stage / Abandon controls.
//!
//! The workspace wraps this in a [`crate::modal::Modal`] and calls
//! [`MissionControlModal::on_open`] / [`MissionControlModal::on_close`] around
//! the modal's visibility (same hosting pattern as
//! [`crate::missions::start_mission_modal::StartMissionModal`]).

use std::path::{Path, PathBuf};

use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warpui::elements::{
    Border, ChildView, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Flex,
    Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::keymap::macros::*;
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
use crate::missions::registry::{ActiveMission, MissionRegistry};
use crate::missions::spec::{self, CriteriaProgress};
use crate::modal::ModalAction;
use crate::view_components::action_button::{ActionButton, NakedTheme, PrimaryTheme};

/// Registers fixed keybindings for the Mission Control modal.
pub fn init(app: &mut AppContext) {
    app.register_fixed_bindings(vec![FixedBinding::new(
        "escape",
        MissionControlModalAction::Escape,
        id!("MissionControlModal"),
    )]);
}

const CONTENT_HORIZONTAL_PADDING: f32 = 24.;
const HEADER_TITLE_FONT_SIZE: f32 = 16.;
const ROW_VERTICAL_PADDING: f32 = 12.;
const ROW_LINE_GAP: f32 = 4.;
const ROW_BUTTON_HEIGHT: f32 = 28.;

/// Per-row hover states for the Resume / Next Stage / Abandon buttons.
#[derive(Clone, Default)]
struct RowMouseStates {
    resume: MouseStateHandle,
    next_stage: MouseStateHandle,
    abandon: MouseStateHandle,
}

pub struct MissionControlModal {
    /// One entry per active mission row, sized in [`Self::on_open`].
    row_mouse_states: Vec<RowMouseStates>,
    /// Per-row spec acceptance-criteria progress, snapshotted in
    /// [`Self::on_open`] (parallel to `row_mouse_states`) so `render` reads no
    /// files. Empty progress for missions without a spec checklist.
    row_criteria: Vec<CriteriaProgress>,
    new_mission_button: ViewHandle<ActionButton>,
    close_button: ViewHandle<ActionButton>,
    close_button_mouse_state: MouseStateHandle,
}

pub enum MissionControlModalEvent {
    /// Resume the current stage of the mission at `mission_index`.
    Resume {
        mission_index: usize,
    },
    /// Advance the mission at `mission_index` past its current stage.
    NextStage {
        mission_index: usize,
    },
    /// Abandon the mission at `mission_index`.
    Abandon {
        mission_index: usize,
    },
    /// Open the Start Mission modal.
    NewMission,
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MissionControlModalAction {
    Resume(usize),
    NextStage(usize),
    Abandon(usize),
    NewMission,
    Close,
    Escape,
}

impl MissionControlModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let close_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Close", NakedTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(MissionControlModalAction::Close);
            })
        });
        let new_mission_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("New Mission", PrimaryTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(MissionControlModalAction::NewMission);
            })
        });

        Self {
            row_mouse_states: Vec::new(),
            row_criteria: Vec::new(),
            new_mission_button,
            close_button,
            close_button_mouse_state: Default::default(),
        }
    }

    /// Called by the workspace before making the modal visible.
    pub fn on_open(&mut self, ctx: &mut ViewContext<Self>) {
        // Snapshot each mission's spec criteria progress once, on open: reading
        // spec.md here (rather than in `render`) keeps the per-row render pure.
        let mission_dirs: Vec<PathBuf> = MissionRegistry::as_ref(ctx)
            .missions()
            .iter()
            .map(|mission| mission.mission_dir.clone())
            .collect();
        self.row_mouse_states
            .resize_with(mission_dirs.len(), Default::default);
        self.row_criteria = mission_dirs
            .iter()
            .map(|mission_dir| spec::read_progress(mission_dir))
            .collect();
        ctx.focus_self();
        ctx.notify();
    }

    /// Called by the workspace when the modal is dismissed.
    pub fn on_close(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.notify();
    }

    /// The last two components of `dir`, e.g. `projetos/warp`.
    fn short_dir(dir: &Path) -> String {
        let components: Vec<String> = dir
            .iter()
            .rev()
            .take(2)
            .map(|component| component.to_string_lossy().into_owned())
            .collect();
        components.into_iter().rev().collect::<Vec<_>>().join("/")
    }

    /// Compact per-stage status strip: done "✓", current "▶", pending "·".
    fn stage_strip(mission: &ActiveMission) -> String {
        mission
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| {
                let marker = match index.cmp(&mission.current_stage) {
                    std::cmp::Ordering::Less => "✓",
                    std::cmp::Ordering::Equal => "▶",
                    std::cmp::Ordering::Greater => "·",
                };
                format!("{marker} {}", stage.name)
            })
            .collect::<Vec<_>>()
            .join("   ")
    }

    fn render_row_button(
        &self,
        label: &str,
        variant: ButtonVariant,
        mouse_state: MouseStateHandle,
        action: MissionControlModalAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let button_style = UiComponentStyles {
            font_size: Some(12.),
            height: Some(ROW_BUTTON_HEIGHT),
            padding: Some(Coords {
                left: 10.,
                right: 10.,
                top: 0.,
                bottom: 0.,
            }),
            ..Default::default()
        };
        appearance
            .ui_builder()
            .button(variant, mouse_state)
            .with_centered_text_label(label.into())
            .with_style(button_style)
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
            .finish()
    }

    fn render_mission_row(
        &self,
        index: usize,
        mission: &ActiveMission,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let sub_text = theme.sub_text_color(theme.background());
        let mouse_states = self
            .row_mouse_states
            .get(index)
            .cloned()
            .unwrap_or_default();

        let mut row = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(ROW_LINE_GAP);

        // Template name + slug.
        row.add_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.)
                .with_child(
                    Text::new_inline(
                        mission.template_name.clone(),
                        appearance.ui_font_family(),
                        appearance.ui_font_size(),
                    )
                    .with_color(theme.active_ui_text_color().into())
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        Text::new_inline(
                            mission.slug.clone(),
                            appearance.ui_font_family(),
                            appearance.ui_font_size() - 2.,
                        )
                        .with_color(sub_text.into())
                        .finish(),
                    )
                    .finish(),
                )
                .finish(),
        );

        // Project directory (last two path components).
        row.add_child(
            Text::new_inline(
                Self::short_dir(&mission.project_dir),
                appearance.ui_font_family(),
                appearance.ui_font_size() - 2.,
            )
            .with_color(sub_text.into())
            .finish(),
        );

        // Stage progress, with the spec's acceptance-criteria tally appended
        // when the mission has a checklist (snapshotted in `on_open`).
        let stage_name = mission
            .stages
            .get(mission.current_stage)
            .map(|stage| stage.name.as_str())
            .unwrap_or("?");
        let criteria = self.row_criteria.get(index).copied().unwrap_or_default();
        let criteria_suffix = if criteria.is_empty() {
            String::new()
        } else {
            format!("   ·   {}/{} criteria", criteria.met, criteria.total)
        };
        row.add_child(
            Text::new_inline(
                format!(
                    "Stage {}/{}: {stage_name}{criteria_suffix}",
                    mission.current_stage + 1,
                    mission.stages.len()
                ),
                appearance.ui_font_family(),
                appearance.ui_font_size() - 1.,
            )
            .with_color(theme.active_ui_text_color().into())
            .finish(),
        );

        // Per-stage status strip.
        row.add_child(
            Text::new_inline(
                Self::stage_strip(mission),
                appearance.ui_font_family(),
                appearance.ui_font_size() - 2.,
            )
            .with_color(sub_text.into())
            .finish(),
        );

        // Buttons.
        row.add_child(
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(self.render_row_button(
                        "Resume",
                        ButtonVariant::Accent,
                        mouse_states.resume,
                        MissionControlModalAction::Resume(index),
                        appearance,
                    ))
                    .with_child(self.render_row_button(
                        "Next Stage",
                        ButtonVariant::Basic,
                        mouse_states.next_stage,
                        MissionControlModalAction::NextStage(index),
                        appearance,
                    ))
                    .with_child(self.render_row_button(
                        "Abandon",
                        ButtonVariant::Basic,
                        mouse_states.abandon,
                        MissionControlModalAction::Abandon(index),
                        appearance,
                    ))
                    .finish(),
            )
            .with_margin_top(ROW_LINE_GAP)
            .finish(),
        );

        let mut container =
            Container::new(row.finish()).with_vertical_padding(ROW_VERTICAL_PADDING);
        if index > 0 {
            // Divider between mission rows.
            container = container.with_border(Border::top(1.).with_border_fill(theme.outline()));
        }
        container.finish()
    }
}

impl Entity for MissionControlModal {
    type Event = MissionControlModalEvent;
}

impl View for MissionControlModal {
    fn ui_name() -> &'static str {
        "MissionControlModal"
    }

    fn on_focus(&mut self, focus_ctx: &warpui::FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            ctx.focus_self();
        }
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let sub_text = theme.sub_text_color(theme.background());

        // ── Header ───────────────────────────────────────────────────────
        let header = {
            let title = Text::new_inline(
                "Mission Control".to_string(),
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

        // ── Mission rows ─────────────────────────────────────────────────
        let missions = MissionRegistry::as_ref(app).missions();
        let mut rows = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if missions.is_empty() {
            rows.add_child(
                Text::new_inline(
                    "No active missions.".to_string(),
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(sub_text.into())
                .finish(),
            );
        } else {
            for (index, mission) in missions.iter().enumerate() {
                rows.add_child(self.render_mission_row(index, mission, appearance));
            }
        }

        let body_container = Container::new(rows.finish())
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
                .with_child(ChildView::new(&self.close_button).finish())
                .with_child(ChildView::new(&self.new_mission_button).finish())
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
            .with_child(Shrinkable::new(1., body_container).finish())
            .with_child(footer)
            .finish()
    }
}

impl TypedActionView for MissionControlModal {
    type Action = MissionControlModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            MissionControlModalAction::Resume(mission_index) => {
                ctx.emit(MissionControlModalEvent::Resume {
                    mission_index: *mission_index,
                });
            }
            MissionControlModalAction::NextStage(mission_index) => {
                ctx.emit(MissionControlModalEvent::NextStage {
                    mission_index: *mission_index,
                });
            }
            MissionControlModalAction::Abandon(mission_index) => {
                ctx.emit(MissionControlModalEvent::Abandon {
                    mission_index: *mission_index,
                });
            }
            MissionControlModalAction::NewMission => {
                ctx.emit(MissionControlModalEvent::NewMission);
            }
            MissionControlModalAction::Close | MissionControlModalAction::Escape => {
                ctx.emit(MissionControlModalEvent::Closed);
            }
        }
    }
}
