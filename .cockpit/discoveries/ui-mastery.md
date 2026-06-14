Perfect. Now I have enough information. Let me compile a comprehensive reference document based on my research:

Based on my thorough exploration of the Warp UI framework codebase, here is the definitive reference document:

---

# Warp UI Framework - Developer Reference

## 1. RENDERING MODEL

### Retained vs Immediate
Warp uses a **retained** rendering model where views hold persistent state and are rebuilt/re-rendered when models change. This is NOT immediate-mode UI—views are long-lived entities that subscribe to model changes.

### Core Traits: View, Entity, Element

**Entity trait** (`/crates/warpui_core/src/core/entity.rs:39`):
```rust
pub trait Entity: 'static {
    type Event;  // Events this entity emits
}
```
Base trait for anything that can produce events (views or models).

**View trait** (`/crates/warpui_core/src/core/view/mod.rs:58`):
```rust
pub trait View: Entity {
    fn ui_name() -> &'static str;
    fn render(&self, app: &AppContext) -> Box<dyn Element>;
    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {}
    fn on_blur(&mut self, blur_ctx: &BlurContext, ctx: &mut ViewContext<Self>) {}
    fn keymap_context(&self, _: &AppContext) -> keymap::Context {}
    fn accessibility_contents(&self, _ctx: &AppContext) -> Option<AccessibilityContent> {}
    fn active_cursor_position(&self, _ctx: &ViewContext<Self>) -> Option<CursorInfo> {}
}
```

**Element trait** (`/crates/warpui_core/src/elements/mod.rs:106`):
```rust
pub trait Element {
    fn layout(&mut self, constraint: SizeConstraint, ctx: &mut LayoutContext, app: &AppContext) -> Vector2F;
    fn after_layout(&mut self, _: &mut AfterLayoutContext, _: &AppContext);
    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext);
    fn size(&self) -> Option<Vector2F>;
    fn origin(&self) -> Option<Point>;
    fn dispatch_event(&mut self, event: &DispatchedEvent, ctx: &mut EventContext, app: &AppContext) -> bool;
}
```

### Layout→After_Layout→Paint Pipeline

The rendering pipeline occurs in three phases:

1. **Layout Phase** (`LayoutContext`):
   - Traverse element tree bottom-up
   - Compute sizes using `SizeConstraint { min, max }`
   - Set `size: Option<Vector2F>` on each element
   - SizeConstraint passed: `/crates/warpui_core/src/presenter.rs:275`

2. **After Layout Phase** (`AfterLayoutContext`):
   - Post-layout computations (animations, text layout cache updates)
   - Happens after all sizes are known

3. **Paint Phase** (`PaintContext`):
   - Traverse tree top-down with origin positions
   - Record primitives to `Scene` (rects, images, glyphs, icons)
   - Access position cache for element locations (`ctx.position_cache.get_position(id)`)

### Context Types

**ViewContext<T>** (`/crates/warpui_core/src/core/view/context.rs:30`):
- Holds `&mut AppContext`, `WindowId`, `EntityId`
- Used in view event handlers and lifecycle methods
- Key methods:
  - `ctx.notify()` - Trigger re-render of this view
  - `ctx.subscribe_to_model(handle, callback)` - Subscribe view to model events
  - `ctx.subscribe_to_view(handle, callback)` - Subscribe view to another view's events
  - `ctx.add_view(builder)` - Create child view
  - `ctx.dispatch_typed_action(action)` - Send typed action

**AppContext** (`/crates/warpui_core/src/core/app.rs`):
- Global app state: all views, models, window managers
- Temporary access during render/event dispatch
- Used to get singleton models: `Appearance::as_ref(ctx)`

**ModelContext<T>** (`/crates/warpui_core/src/core/model/context.rs`):
- Used in model initialization and event handling
- `ctx.notify()` - Notify all subscribers (triggers view re-renders)
- `ctx.emit(event)` - Send specific event to subscribers

### Observation & Re-rendering Pattern

Views re-render when:
1. They subscribe to a model via `ctx.subscribe_to_model(handle, |view, model_handle, event, ctx| { ... })`
2. The subscribed model calls `ctx.notify()` or `ctx.emit(event)`
3. The framework calls `view.render(&app_context)` again

Example pattern (from `/app/src/missions/start_mission_modal.rs:124`):
```rust
ctx.subscribe_to_view(&dir_editor, |me, _, event, ctx| {
    me.handle_dir_editor_event(event, ctx);
});
```

### Scene & GPU Backend

**Scene** (`/crates/warpui_core/src/scene.rs:1`):
- Records all visual primitives for a frame
- Contains layers with z-indices
- Stores: rects, images, glyphs, icons
- Has clipping bounds and hit-test maps (RTree)

**Rendering** (`/crates/warpui/src/rendering/`):
- **wgpu backend** (`/crates/warpui/src/rendering/wgpu/`): GPU-accelerated rendering
- **Font atlas** (`/crates/warpui/src/rendering/atlas/`): Glyph caching

---

## 2. COMPONENT CATALOG

All Warp components follow the **Component pattern** (`/crates/ui_components/src/lib.rs:108`):

```rust
pub trait Component: Default {
    type Params<'a>: Params;
    fn render<'a>(&self, appearance: &Appearance, params: Self::Params<'a>) -> Box<dyn Element>;
}
```

Components are **stored as view fields** (not created inline during render) to preserve internal state like `MouseStateHandle`.

### Layout Components

**Flex** (`/crates/warpui_core/src/elements/flex/mod.rs:18`)
- Flexbox-like row/column layout
- Constructor: `Flex::row()` or `Flex::column()`
- Methods:
  - `.with_main_axis_alignment(MainAxisAlignment)` - Start, Center, End, SpaceBetween, SpaceEvenly
  - `.with_cross_axis_alignment(CrossAxisAlignment)` - Start, Center, End, Stretch
  - `.with_spacing(f32)` - Gap between children
  - `.add_child(element)` / `.with_child(element)`

**Container** (`/crates/warpui_core/src/elements/container.rs:16`)
- Adds padding, margin, background, border, corner radius, drop shadow
- Methods: `.with_horizontal_padding(f32)`, `.with_background(fill)`, `.with_border(border)`, `.with_corner_radius(radius)`, `.with_drop_shadow(shadow)`

**ConstrainedBox** (`/crates/warpui_core/src/elements/constrained_box.rs`)
- Fixes width/height constraints
- Methods: `.with_width(f32)`, `.with_height(f32)`, `.with_min_width()`, `.with_max_height()`

**Align** (`/crates/warpui_core/src/elements/align.rs`)
- Aligns child element within available space
- Constructor: `Align::new(alignment, child)`

**Stack** (`/crates/warpui_core/src/elements/stack/mod.rs:79`)
- Layers elements (z-ordering)
- Methods:
  - `.add_child(element)` - Child contributes to stack size
  - `.add_positioned_child(element, positioning)` - Absolute positioned, doesn't affect size
  - `.add_overlay_child(element)` - Float above normal UI (for dropdowns/tooltips)

**Shrinkable** - Makes element shrink to fit content
- Constructor: `Shrinkable::new(flex_factor, child)`

### Primitives

**Text** (`/crates/warpui_core/src/elements/text.rs:121`)
- Constructor: `Text::new(text, family_id, font_size)` or `Text::new_inline(...)` (single line)
- Methods:
  - `.with_color(color)`
  - `.with_style(Properties::default().weight(Weight::Semibold))`
  - `.with_selectable(false)`

**Icon** (`/crates/warpui_core/src/elements/icon.rs`)
- Renders SVG icon from `/app/assets/bundled/svg/`
- Constructor: `Icon::new(icon_enum)` or use `warp_core::ui::Icon` enum
- Example: `Icon::File`, `Icon::Plus`, `Icon::Settings` (see `/crates/warp_core/src/ui/icons.rs:11`)

**Rect** (`/crates/warpui_core/src/elements/rect.rs`)
- Rectangle shape with fill/border
- Constructor: `Rect::new()`
- Methods: `.with_background(fill)`, `.with_border(border)`, `.with_drop_shadow(shadow)`

**Image** (`/crates/warpui_core/src/elements/image.rs`)
- Renders cached image from asset system

### Interactive Components

**Button/ActionButton** (`/crates/ui_components/src/button.rs`)
- Component (must be stored in view struct)
- Constructor: Stored as field, rendered via `.render(appearance, params)`
- Params:
  ```rust
  button::Params {
      content: button::Content::Label("Click".into()),  // or ::Icon or ::IconAndLabel
      theme: &button::themes::Primary,                  // or Naked, etc.
      options: button::Options { disabled: false, .. }
  }
  ```
- Usage example: `/app/src/missions/start_mission_modal.rs:156`

**Hoverable** (`/crates/warpui_core/src/elements/hoverable.rs:30`)
- Wraps element and tracks mouse state
- Constructor: `Hoverable::new(mouse_state_handle, |state| { ... render content ... })`
- MouseState methods: `.is_hovered()`, `.is_clicked()`, `.is_mouse_over_element()`
- Critical: **Store MouseStateHandle in your view struct, not inline!** (WARP.md:68)

**EventHandler** (`/crates/warpui_core/src/elements/event_handler.rs`)
- Attaches keyboard/mouse handlers to elements
- Methods: `.on_key_down()`, `.on_key_up()`, `.on_left_click()`, `.on_scroll()`

**MouseStateHandle** (created via `MouseStateHandle::default()`)
- Shared state for mouse interactions
- Must be created once and cloned, NOT re-created on each render

**Dropdown** (app-level, `/app/src/view_components/dropdown.rs`)
- Dropdown menu with selection
- Methods: `.set_items()`, `.set_selected_by_index()`, `.get_selected()`

**Switch** (`/crates/ui_components/src/switch.rs`)
- Toggle switch component
- Params with enabled/disabled state

**Tooltip** (`/crates/ui_components/src/tooltip.rs`)
- Shows on hover
- Methods: `.render(appearance, params)`

### Containers & Scrolling

**Clipped** (`/crates/warpui_core/src/elements/clipped.rs`)
- Clips child to bounds
- Constructor: `Clipped::new(child, bounds)`

**ClippedScrollable** (`/crates/warpui_core/src/elements/clipped_scrollable.rs:187`)
- Full-tree scrolling with clipping (slower but works on any element)
- Constructor: `ClippedScrollable::new(axis, child)`
- State: `ClippedScrollStateHandle::new()`
- Methods: `.scroll_to(pixels)`, `.scroll_by(delta)`, `.scroll_to_position(target)`

**Scrollable** (legacy, `/crates/warpui_core/src/elements/scrollable.rs`)
- Use `ClippedScrollable` instead

**List** (`/crates/warpui_core/src/elements/list.rs`)
- Virtualized list (only renders visible items)
- Use `UniformList` for uniform-height items

**Table** (`/crates/warpui_core/src/elements/table/mod.rs`)
- Tabular data display with columns
- Example: `/app/src/`

**Dialog** (`/crates/ui_components/src/dialog.rs:30`)
- Modal dialog component
- Params:
  ```rust
  dialog::Params {
      title: "Title",
      content: Box::new(...),
      options: dialog::Options { width: Some(400.), on_dismiss: Some(callback), .. }
  }
  ```

### Text Input

**EditorView** (in `crates/editor` + app integration)
- Full-featured text editor with syntax highlighting, undo/redo
- Single-line: `EditorView::single_line(options, ctx)`
- Multi-line: `EditorView::new(options, ctx)`
- Options: `EditorOptions { soft_wrap: true, enter_settings: ..., .. }`
- Methods: `.set_placeholder_text()`, `.clear_buffer_and_reset_undo_stack()`
- Example: `/app/src/missions/start_mission_modal.rs:113`

---

## 3. THEMING

### WarpTheme & Appearance

**Appearance** (`/crates/warp_core/src/ui/appearance.rs:18`):
- Singleton model storing visual settings
- Constructor: `Appearance::new(theme, mono_font_family, mono_font_size, ...)`
- Mock for testing: `Appearance::mock()`
- Access: `Appearance::as_ref(ctx)` or `Appearance::handle(ctx)`

**WarpTheme** (`/crates/warp_core/src/ui/theme.rs`):
- Holds all color definitions
- Methods:
  - `.surface_1()` - Main background
  - `.surface_2()` - Secondary background
  - `.main_text_color(background)` - Text color with contrast
  - `.accent()` - Primary accent color
  - `.outline()` - Border color

### Colors, Fonts, Fills

**Fill** (`/crates/warpui_core/src/elements/mod.rs` or scene):
```rust
pub enum Fill {
    Solid(ColorU),
    Gradient { start: ColorU, end: ColorU },
    None,
}
```

**ColorU** (from pathfinder):
- RGBA: `ColorU::new(r, g, b, a)` or `ColorU::from_u32(0xRRGGBBAA)`

**Fonts**:
- `appearance.ui_font_family()` - Main UI font
- `appearance.monospace_font_family()` - Code font
- `appearance.ui_font_size()` - Default 12.0px
- `appearance.monospace_font_size()` - Code font size
- Font weight: `Properties::default().weight(Weight::Semibold)`

**Corners & Borders**:
- `CornerRadius::with_all(Radius::Pixels(4.0))`
- `Border::all(1.0).with_border_color(color)`
- Dashed borders: `Border { dash: Some(Dash { dash_length: 4., gap_length: 2., .. }), .. }`

**DropShadow** (`/crates/warpui_core/src/scene.rs:128`):
```rust
DropShadow {
    color: ColorU,
    offset: Vector2F,
    blur_radius: f32,
    spread_radius: f32,
}
// Or use default: DropShadow::default()
// Or: DropShadow::new_with_standard_offset_and_spread(color)
```

---

## 4. ANIMATION & MOTION

### Limitations & Approach

Warp does **NOT** have a declarative animation system. Instead:

1. **Frame-based updates**: Use `ctx.on_next_frame_drawn(callback)` to schedule work
2. **Hover transitions**: `Hoverable` tracks state changes; visual transition via styling
3. **Drop shadows & blur**: Baked into rendering (via DropShadow, no animation primitives)
4. **Animated images**: `Image::with_animation_enabled()` for GIF/WebP (see `/crates/warpui_core/src/elements/image.rs:161`)

### Motion Pattern

For hover state transitions without animation:
- Use `Hoverable` with `MouseState::is_hovered()`
- Change colors/opacity based on state in the render closure
- No easing/timing—instant or manual polling

---

## 5. MODALS & OVERLAYS

### The Pattern: ModalViewState

Modal views use the **ModalViewState** wrapper (`/app/src/modal.rs:55`):

```rust
pub struct ModalViewState<T: View> {
    pub view: ViewHandle<T>,
    state: ModalState,  // Open or Closed
}

impl<T: View> ModalViewState<T> {
    pub fn is_open(&self) -> bool { /* ... */ }
    pub fn open(&mut self) { }
    pub fn close(&mut self) { }
    pub fn render(&self) -> Box<dyn Element> {
        ChildView::new(&self.view).finish()
    }
}
```

### Creating a Modal View (Example: StartMissionModal)

**Step 1: Define the view** (`/app/src/missions/start_mission_modal.rs:76`):
```rust
pub struct StartMissionModal {
    dir_editor: ViewHandle<EditorView>,
    briefing_editor: ViewHandle<EditorView>,
    template_dropdown: ViewHandle<Dropdown<StartMissionModalAction>>,
    error: Option<String>,
    // ... fields for child views
}

pub enum StartMissionModalEvent {
    Confirmed { project_dir: String, template: MissionTemplate, briefing: String },
    Closed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StartMissionModalAction {
    Cancel, Submit, Escape, SelectTemplate(usize),
}
```

**Step 2: Register keybindings** (lines 42-64):
```rust
pub fn init(app: &mut AppContext) {
    app.register_fixed_bindings(vec![
        FixedBinding::new("escape", StartMissionModalAction::Escape, id!("StartMissionModal")),
        FixedBinding::new("enter", StartMissionModalAction::Submit, id!("StartMissionModal") & !id!("EditorView")),
    ]);
}
```
- The `id!()` macro scopes actions to specific views

**Step 3: Implement View & TypedActionView** (lines 109-183):
- `View::ui_name()` returns `"StartMissionModal"`
- `View::render()` builds the UI
- `View::keymap_context()` returns context with `id!("StartMissionModal")`
- `TypedActionView::handle_action()` processes actions

**Step 4: Lifecycle methods** (lines 187-234):
```rust
pub fn on_open(&mut self, initial_dir: Option<String>, ctx: &mut ViewContext<Self>) {
    // Load templates, reset editors, focus first field
    ctx.notify();
}

pub fn on_close(&mut self, ctx: &mut ViewContext<Self>) {
    // Clear transient state
    ctx.notify();
}
```

**Step 5: In parent (e.g., Workspace), manage state**:
```rust
let modal_state: ModalViewState<StartMissionModal> = ...;
if should_show {
    modal_state.open();
    modal_state.view.update(ctx, |view, view_ctx| {
        view.on_open(initial_dir, view_ctx);
    });
}
```

### Rendering Modals in Stack

Modal is rendered in a **Stack** with backdrop:
```rust
let mut stack = Stack::new();
stack.add_overlay_child(backdrop);  // Dark overlay behind modal
stack.add_positioned_overlay_child(modal_content, positioning);
```

**Stack positioning** (`/crates/warpui_core/src/elements/stack/offset_positioning.rs`):
- `.add_positioned_overlay_child(element, OffsetPositioning::...)` places element absolutely
- Center on screen: Use `ParentOffsetBounds::WindowByPosition` + anchors

### Dialog Component (Easier Alternative)

For simple dialogs, use `Dialog` component (`/crates/ui_components/src/dialog.rs:30`):
```rust
Dialog.render(appearance, dialog::Params {
    title: "Confirm".into(),
    content: Box::new(|app| Text::new("Are you sure?", ...).finish()),
    options: dialog::Options {
        on_dismiss: Some(Arc::new(|ctx, app| { /* handle dismiss */ })),
        ..Default::default()
    },
})
```

---

## 6. KEYBINDINGS & ACTIONS

### TypedActionView Pattern

All modern views use **TypedActionView** (`/crates/warpui_core/src/core/view/mod.rs:143`):

```rust
pub trait TypedActionView {
    type Action: Action;
    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {}
}
```

**Action trait** (`/crates/warpui_core/src/core/action.rs:10`):
```rust
pub trait Action: Any + Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}
// Automatically implemented for all types meeting the trait bounds
```

### Registering Keybindings

**FixedBinding** (static, from `warpui_core::keymap::FixedBinding`):
```rust
app.register_fixed_bindings(vec![
    FixedBinding::new("cmdorctrl-s", MyAction::Save, id!("MyView")),
    FixedBinding::new("escape", MyAction::Close, id!("Modal")),
]);
```

**Context Predicates** (using `id!()` macro):
- `id!("ViewName")` - Matches when view is focused
- `id!("ViewName") & !id!("EditorView")` - AND / NOT combinations
- `id!("MyView") | id!("OtherView")` - OR combinations

### Dispatching Actions

From a view:
```rust
ctx.dispatch_typed_action(MyAction::Clicked);
```

From an event handler (in Hoverable, etc.):
```rust
EventHandler::new(child)
    .on_left_click(|ctx, app| {
        ctx.dispatch_typed_action(MyAction::Clicked);
    })
```

### EditableBinding (Dynamic)

For user-configurable keybindings:
- Stored in settings
- Loaded at startup via keymap configuration
- Not used in most UI components (fixed bindings preferred)

---

## 7. ICONS & ASSETS

### Icon System

**Icon enum** (`/crates/warp_core/src/ui/icons.rs:11`):
- 200+ variants: `Icon::File`, `Icon::Plus`, `Icon::Settings`, `Icon::AiAssistant`, etc.
- Located at `/crates/warp_core/src/ui/icons.rs`

**Icon constructor**:
```rust
Icon::File.to_warpui_icon(color).finish()
```

**Rendering in Button**:
```rust
button::Params {
    content: button::Content::Icon(Icon::X),  // or IconAndLabel
    ..
}
```

### SVG Storage

- **Location**: `/app/assets/bundled/svg/*.svg`
- **Mapping**: Icon enum variant name → SVG filename (auto-mapped)
- **Size**: Default `ICON_DIMENSIONS = 24.0` px
- **Color**: Applied at render time via `to_warpui_icon(color)`

### Adding New Icons

1. Add `.svg` file to `/app/assets/bundled/svg/`
2. Add variant to `Icon` enum (`/crates/warp_core/src/ui/icons.rs`)
3. The framework auto-resolves name→file via `icon_name.to_lowercase().replace('_', '-')`

---

## 8. GOTCHAS & CRITICAL PATTERNS

### MouseStateHandle Must Be Long-Lived

**CRITICAL (WARP.md:68)**: Never create `MouseStateHandle::default()` inline during render:

```rust
// WRONG - will break hover/click:
let mut hoverable = Hoverable::new(MouseStateHandle::default(), |state| { ... });

// RIGHT - store in view struct:
pub struct MyView {
    mouse_state: MouseStateHandle,  // Created once in new()
}

// Then reuse:
let mut hoverable = Hoverable::new(self.mouse_state.clone(), |state| { ... });
```

### Terminal Model Locking Deadlock

**CRITICAL (WARP.md:113-117)**: Be extremely careful with `TerminalModel.lock()`:
- Acquiring multiple locks in the same call stack → **UI FREEZE** (beach ball on macOS)
- Before adding `model.lock()`, verify no caller already holds it
- Prefer passing locked references down the stack
- Keep lock scope minimal

### ViewContext Lifetimes

`ViewContext<'a, T>` borrows `AppContext` mutably:
- Valid only during that event/render callback
- Cannot be stored in view state
- Use `WeakViewHandle` / `ViewHandle` for cross-view references instead

### id!() Scoping & Keymap Context

The `id!()` macro creates unique scopes for keybinding predicates:
```rust
id!("StartMissionModal")  // View must implement View::ui_name() -> "StartMissionModal"
id!("EditorView") & !id!("TerminalView")  // Compound predicates
```

**View must set this in `keymap_context()`**:
```rust
fn keymap_context(&self, _: &AppContext) -> keymap::Context {
    let mut ctx = keymap::Context::default();
    ctx.set.insert("StartMissionModal");  // Or use Self::default_keymap_context()
    ctx
}
```

### Scene & Rendering Phases Are Separate

- **Layout/Paint**: Produce `Scene` with primitives
- **GPU Rendering**: wgpu backend renders Scene to frame
- Cannot access rendered pixels during same frame
- Use `position_cache.get_position(id)` to query element bounds from previous frame

### No Event Handling in Paint Phase

- Events dispatched in separate phase (via `dispatch_event()`)
- Cannot respond to events in paint callbacks
- Use `after_layout` for post-position calculations

### WASM cfg-gating

Some code is WASM-only or WASM-incompatible:
```rust
#[cfg(not(target_arch = "wasm32"))]
fn native_only() { }

#[cfg(target_arch = "wasm32")]
fn wasm_only() { }
```

- Soft keyboard: wasm-specific (`/crates/warpui/src/platform/wasm/soft_keyboard.rs`)
- File picking: platform-specific implementations

### Dismiss & Click-Through Behavior

`Dismiss` element blocks interaction with siblings:
```rust
Dismiss::new(modal_content)
    .prevent_interaction_with_other_elements()  // Prevents interaction outside
    .on_dismiss(move |ctx, app| { /* close modal */ })
```

---

## 9. USAGE REFERENCE SNIPPETS

### Building a Simple Button

```rust
pub struct MyView {
    my_button: button::Button,
}

impl View for MyView {
    fn ui_name() -> &'static str { "MyView" }
    
    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        self.my_button.render(appearance, button::Params {
            content: button::Content::Label("Click Me".into()),
            theme: &button::themes::Primary,
            options: button::Options {
                ..button::Options::default(appearance)
            },
        })
    }
}
```

### Creating a Hover-Sensitive Element

```rust
pub struct MyView {
    mouse_state: MouseStateHandle,
}

impl View for MyView {
    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        Hoverable::new(self.mouse_state.clone(), |state| {
            let bg = if state.is_hovered() {
                ColorU::new(100, 100, 100, 255)
            } else {
                ColorU::new(50, 50, 50, 255)
            };
            
            Rect::new()
                .with_background(bg)
                .finish()
        })
        .on_left_click(|ctx, app, _pos| {
            // Handle click
        })
        .finish()
    }
}
```

### Listening to Model Changes

```rust
impl View for MyView {
    fn new(ctx: &mut ViewContext<Self>) -> Self {
        let appearance_handle = Appearance::handle(ctx);
        ctx.subscribe_to_model(&appearance_handle, |me, _handle, event, ctx| {
            me.handle_appearance_changed(event, ctx);
        });
        // ...
    }
}

impl Entity for MyView {
    type Event = ();
}

impl TypedActionView for MyView {
    type Action = MyAction;
    
    fn handle_action(&mut self, action: &MyAction, ctx: &mut ViewContext<Self>) {
        match action {
            MyAction::Save => { /* ... */ }
            _ => {}
        }
    }
}
```

### Modal Lifecycle

```rust
// In workspace or parent:
let mut start_mission_modal: ModalViewState<StartMissionModal> = 
    ModalViewState::new(ctx.add_view(|ctx| StartMissionModal::new(ctx)));

// When opening:
start_mission_modal.open();
start_mission_modal.view.update(ctx, |view, view_ctx| {
    view.on_open(Some("/home/user/project".into()), view_ctx);
});

// When rendering:
if start_mission_modal.is_open() {
    stack.add_overlay_child(start_mission_modal.render());
}

// When closing (from modal action):
start_mission_modal.close();
start_mission_modal.view.update(ctx, |view, view_ctx| {
    view.on_close(view_ctx);
});
```

---

This reference covers the essential patterns, APIs, and gotchas needed to build polished, modern animated UI in Warp. The framework prioritizes retained state, strong type safety, and reactive updates via the subscription model.
