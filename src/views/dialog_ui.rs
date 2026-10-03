//! Shared native-dialog template and message chrome.
//!
//! Every dialog window (About, Launch Error, Settings, Delete Shader Cache) is
//! one `DialogWindow<C>` view opened by [`open_dialog`]. The template owns the
//! window chrome: titlebar options, modality, clamped placement, Esc and WM
//! close, focus, accessibility metadata, a scrollable body, and the action
//! footer. Contents only supply text, widgets, and buttons.
//!
//! Body copy wraps inside [`dialog_alert_body`]'s `flex_1().min_w_0()` column;
//! without it the flex item's auto minimum size keeps the text at its intrinsic
//! (unwrapped) width and the scroll container clips it.

use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    notification::Notification,
    v_flex,
};
use gpui_kit::*;

/// Default dialog dimensions (a short message box). Contents override
/// [`DialogContent::size`] when they need different dimensions.
pub const DEFAULT_DIALOG_SIZE: Size<Pixels> = size(px(420.), px(240.));

/// Visual variant of a dialog action button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogActionKind {
    Primary,
    Outline,
    Danger,
}

/// Click handler for one dialog action button.
type DialogClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One button in the dialog footer.
pub struct DialogAction {
    id: &'static str,
    label: SharedString,
    kind: DialogActionKind,
    icon: Option<IconName>,
    enabled: bool,
    on_click: DialogClickHandler,
}

impl DialogAction {
    /// A new action whose default click closes the dialog window.
    pub fn new(id: &'static str, label: impl Into<SharedString>, kind: DialogActionKind) -> Self {
        Self {
            id,
            label: label.into(),
            kind,
            icon: None,
            enabled: true,
            on_click: Box::new(|_, window, _| window.remove_window()),
        }
    }

    /// An outline action that closes the dialog window (Cancel/Close).
    pub fn close(id: &'static str, label: impl Into<SharedString>) -> Self {
        Self::new(id, label, DialogActionKind::Outline)
    }

    /// Prefix the label with an icon.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Box::new(f);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn into_button(self) -> Button {
        let Self {
            id,
            label,
            kind,
            icon,
            enabled,
            on_click,
        } = self;
        let mut button = match kind {
            DialogActionKind::Primary => Button::new(id).primary(),
            DialogActionKind::Outline => Button::new(id).outline(),
            DialogActionKind::Danger => Button::new(id).danger(),
        };
        if let Some(icon) = icon {
            button = button.icon(icon);
        }
        // Fixed size (not rem-scaled) to match the Qt/Breeze button geometry.
        button
            .label(label)
            .h(px(32.))
            .min_w(px(80.))
            .disabled(!enabled)
            .on_click(move |event, window, cx| on_click(event, window, cx))
    }
}

/// The content of one dialog window.
pub trait DialogContent: 'static {
    /// Stable identity: one window per id, deduped by [`open_dialog`].
    fn id(&self) -> &'static str;
    fn title(&self) -> SharedString;
    /// Window size; defaults to [`DEFAULT_DIALOG_SIZE`]. Override in the
    /// content type when the dialog needs different dimensions.
    fn size(&self) -> Size<Pixels> {
        DEFAULT_DIALOG_SIZE
    }
    fn body(&mut self, window: &mut Window, cx: &mut App) -> AnyElement;
    fn actions(&self, cx: &App) -> Vec<DialogAction>;
    /// Esc and the WM close button are ignored while this returns false.
    fn dismissible(&self, cx: &App) -> bool {
        let _ = cx;
        true
    }
}

/// The shared chrome around a [`DialogContent`]: titlebar, modality, clamped
/// placement, Esc, focus, accessibility, scrollable body, and footer.
pub struct DialogWindow<C: DialogContent> {
    content: C,
    focus_handle: FocusHandle,
}

impl<C: DialogContent> Render for DialogWindow<C> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.content.id();
        let title = self.content.title();
        let actions = self.content.actions(cx);
        let body = self.content.body(window, cx);
        let focus_handle = self.focus_handle.clone();

        v_flex()
            .id("dialog-root")
            .test_support()
            .debug_selector(move || format!("{id}-dialog"))
            .role(Role::Dialog)
            .aria_label(title)
            .accessibility_id(format!("{id}.dialog"))
            .track_focus(&focus_handle)
            .size_full()
            .p_4()
            .gap_2()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.content.dismissible(cx) {
                    window.remove_window();
                }
            }))
            .child(
                div()
                    .id("dialog-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .children(actions.into_iter().map(DialogAction::into_button)),
            )
    }
}

/// A handle to an open dialog window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogHandle {
    handle: AnyWindowHandle,
}

impl DialogHandle {
    #[cfg(test)]
    pub fn window_id(&self) -> WindowId {
        self.handle.window_id()
    }
}

/// Failure to create a dialog window; the message is already user-facing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogOpenError(pub String);

impl From<DialogHandle> for AnyWindowHandle {
    fn from(handle: DialogHandle) -> Self {
        handle.handle
    }
}

impl std::fmt::Display for DialogOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DialogOpenError {}

/// Type-erased content replacement kept by the registry (the registry must not
/// know the concrete `C`).
type ReplaceDialogContent = Rc<dyn Fn(&mut App, Box<dyn Any>)>;

/// Type-erased content mutation kept by the registry.
type UpdateDialogContent = Rc<dyn Fn(&mut App, &mut dyn FnMut(&mut dyn Any))>;

struct DialogEntry {
    /// Window that owns the dialog; closing it cascades to this dialog.
    parent: WindowId,
    handle: AnyWindowHandle,
    replace: ReplaceDialogContent,
    update: UpdateDialogContent,
}

#[derive(Default)]
struct DialogRegistry {
    open: HashMap<&'static str, DialogEntry>,
    /// Keeps the `on_window_closed` observer alive while the registry exists.
    _closed: Option<Subscription>,
}

impl Global for DialogRegistry {}

/// Lazily install the registry and its window-closed observer.
fn ensure_registry(cx: &mut App) {
    if cx.has_global::<DialogRegistry>() {
        return;
    }

    let closed = cx.on_window_closed(|cx, window_id| {
        if !cx.has_global::<DialogRegistry>() {
            return;
        }
        let registry = cx.global_mut::<DialogRegistry>();
        let mut cascade = Vec::new();
        registry.open.retain(|_, entry| {
            if entry.handle.window_id() == window_id {
                return false;
            }
            if entry.parent == window_id {
                cascade.push(entry.handle);
                return false;
            }
            true
        });
        if cascade.is_empty() {
            return;
        }
        // Deferred: the observer list is retained while it is dispatching, so
        // closing child windows from here would re-enter the dispatch.
        cx.defer(move |cx| {
            for handle in cascade {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
    });

    cx.set_global(DialogRegistry {
        open: HashMap::new(),
        _closed: Some(closed),
    });
}

/// The live window for a dialog id, if one is registered.
pub fn window_for(id: &str, cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<DialogRegistry>()?
        .open
        .get(id)
        .map(|entry| entry.handle)
}

/// Whether a dialog with this id is currently open.
#[cfg(test)]
pub fn is_open(id: &str, cx: &App) -> bool {
    window_for(id, cx).is_some()
}

/// Mutate the content of an open dialog and repaint it. Returns false when the
/// dialog is not open or its content type no longer matches.
///
/// Dialog contents hold a snapshot of the state they render, because their
/// first frame is drawn while the owning entity is still being updated (and
/// must not read it back). Callers push later changes through this function.
pub fn update_dialog_content<C: DialogContent>(
    id: &str,
    cx: &mut App,
    update: impl FnOnce(&mut C),
) -> bool {
    let updater = cx
        .try_global::<DialogRegistry>()
        .and_then(|registry| registry.open.get(id))
        .map(|entry| entry.update.clone());
    let Some(updater) = updater else {
        return false;
    };
    let mut update = Some(update);
    let mut applied = false;
    updater(cx, &mut |content: &mut dyn Any| {
        if let Some(content) = content.downcast_mut::<C>()
            && let Some(update) = update.take()
        {
            update(content);
            applied = true;
        }
    });
    applied
}

/// Close an open dialog window by id; a no-op when it is not open.
pub fn close_dialog(id: &str, cx: &mut App) {
    if let Some(handle) = window_for(id, cx) {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
    }
}

/// Run an owning view's method with the owning window's `&mut Window`.
///
/// Dialog actions dispatch with the dialog's window, but the owning view lives
/// in the parent window (`append_log`, `spawn_in`, ...). Returns `None` when
/// the window or the entity is already gone (for example, quit mid-flow).
pub fn with_window_and_entity<T: 'static, R>(
    window: AnyWindowHandle,
    entity: &WeakEntity<T>,
    cx: &mut App,
    f: impl FnOnce(&mut T, &mut Window, &mut Context<T>) -> R,
) -> Option<R> {
    let entity = entity.upgrade()?;
    window
        .update(cx, |_, window, cx| {
            entity.update(cx, |entity, cx| f(entity, window, cx))
        })
        .ok()
}

/// Center a dialog on the parent, then clamp it into the display's visible
/// bounds. A dialog larger than the visible area is pinned to the visible
/// origin on that axis.
pub(crate) fn dialog_bounds(
    parent: Bounds<Pixels>,
    desired: Size<Pixels>,
    visible: Bounds<Pixels>,
) -> Bounds<Pixels> {
    let centered = Bounds::centered_at(parent.center(), desired);
    let max_x = visible.origin.x + visible.size.width - desired.width;
    let max_y = visible.origin.y + visible.size.height - desired.height;
    let x = centered
        .origin
        .x
        .max(visible.origin.x)
        .min(max_x.max(visible.origin.x));
    let y = centered
        .origin
        .y
        .max(visible.origin.y)
        .min(max_y.max(visible.origin.y));
    Bounds::new(point(x, y), desired)
}

/// Open (or re-focus and update) a native dialog window.
///
/// A second call with a live id activates the existing window and replaces its
/// content instead of stacking a duplicate. Failures are reported on `parent`
/// as a notification and returned to the caller so destructive flows can abort.
pub fn open_dialog<C: DialogContent>(
    parent: &mut Window,
    cx: &mut App,
    content: C,
) -> Result<DialogHandle, DialogOpenError> {
    ensure_registry(cx);

    let id = content.id();
    let existing = cx
        .try_global::<DialogRegistry>()
        .and_then(|registry| registry.open.get(id))
        .map(|entry| (entry.handle, entry.replace.clone(), entry.update.clone()));
    if let Some((handle, replace, update)) = existing {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        replace(cx, Box::new(content));
        update(cx, &mut |_| {});
        return Ok(DialogHandle { handle });
    }

    let desired = content.size();
    let visible = parent
        .display(cx)
        .map(|display| display.visible_bounds())
        .unwrap_or_else(|| parent.bounds());
    let bounds = dialog_bounds(parent.bounds(), desired, visible);
    let title = content.title();

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(desired),
        titlebar: Some(TitlebarOptions {
            title: Some(title),
            ..Default::default()
        }),
        kind: WindowKind::Dialog,
        window_decorations: Some(WindowDecorations::Server),
        // Same identity as the main window: it must match the installed
        // desktop file's basename so dialogs inherit the app icon.
        app_id: Some(crate::flatpak::window_app_id()),
        ..Default::default()
    };

    match gpui_kit::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| DialogWindow {
            content,
            focus_handle: cx.focus_handle(),
        });
        let focus_handle = view.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);

        let weak = view.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            weak.upgrade()
                .is_none_or(|view| view.read(cx).content.dismissible(cx))
        });
        view
    }) {
        Ok((handle, view)) => {
            let weak_view = view.downgrade();
            let replace: ReplaceDialogContent = {
                let weak_view = weak_view.clone();
                Rc::new(move |cx: &mut App, boxed: Box<dyn Any>| {
                    let Ok(content) = boxed.downcast::<C>() else {
                        return;
                    };
                    let _ = weak_view.update(cx, |view, cx| {
                        view.content = *content;
                        cx.notify();
                    });
                })
            };
            let update: UpdateDialogContent = {
                let weak_view = weak_view.clone();
                Rc::new(move |cx: &mut App, apply: &mut dyn FnMut(&mut dyn Any)| {
                    if let Some(view) = weak_view.upgrade() {
                        view.update(cx, |view, cx| {
                            apply(&mut view.content);
                            cx.notify();
                        });
                    }
                    let _ = handle.update(cx, |_, window, _| window.refresh());
                })
            };
            cx.global_mut::<DialogRegistry>().open.insert(
                id,
                DialogEntry {
                    parent: parent.window_handle().window_id(),
                    handle,
                    replace,
                    update,
                },
            );
            Ok(DialogHandle { handle })
        }
        Err(err) => {
            let message = format!("protonctx: failed to open {id} dialog: {err}");
            eprintln!("{message}");
            parent.push_notification(Notification::error(message.clone()), cx);
            Err(DialogOpenError(message))
        }
    }
}

/// Dialog body copy at Qt's 10 pt scale: base text size with a 1.25 line
/// height, matching the rem the root plugin installs on KDE (10 pt @ 96 DPI).
pub fn dialog_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_base()
        .line_height(relative(1.25))
        .child(text.into())
}

/// The shared alert body: warning icon plus message text that wraps inside the
/// dialog instead of relying on the clip.
pub fn dialog_alert_body(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .child(
            Icon::new(IconName::CircleAlert)
                .flex_shrink_0()
                .text_color(cx.theme().danger),
        )
        .child(v_flex().flex_1().min_w_0().child(dialog_text(text)))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context,
        InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
        Styled as _, TestAppContext, TestSupportExt as _, Window, div, point, px, size,
    };

    use super::{
        DEFAULT_DIALOG_SIZE, DialogAction, DialogActionKind, DialogContent, DialogHandle,
        DialogOpenError, dialog_bounds, open_dialog, window_for,
    };

    struct ParentView;

    impl Render for ParentView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    struct TestContent {
        dialog_id: &'static str,
        message: String,
        seen: Rc<RefCell<String>>,
        dismissible: Rc<Cell<bool>>,
    }

    impl TestContent {
        fn new(dialog_id: &'static str, message: &str) -> Self {
            Self {
                dialog_id,
                message: message.to_string(),
                seen: Rc::new(RefCell::new(String::new())),
                dismissible: Rc::new(Cell::new(true)),
            }
        }
    }

    impl DialogContent for TestContent {
        fn id(&self) -> &'static str {
            self.dialog_id
        }

        fn title(&self) -> SharedString {
            "Test Dialog".into()
        }

        fn body(&mut self, _window: &mut Window, _cx: &mut App) -> AnyElement {
            *self.seen.borrow_mut() = self.message.clone();
            div()
                .id("test-dialog-body")
                .test_support()
                .debug_selector(|| "test-dialog-body".into())
                .child(self.message.clone())
                .into_any_element()
        }

        fn actions(&self, _cx: &App) -> Vec<DialogAction> {
            vec![DialogAction::new(
                "test-dialog-ok",
                "Ok",
                DialogActionKind::Primary,
            )]
        }

        fn dismissible(&self, _cx: &App) -> bool {
            self.dismissible.get()
        }
    }

    fn open_parent(cx: &mut TestAppContext) -> AnyWindowHandle {
        cx.update(gpui_kit::init);
        cx.open_window(size(px(760.), px(520.)), |_, _| ParentView)
            .into()
    }

    fn open_test_dialog(
        parent: AnyWindowHandle,
        content: TestContent,
        cx: &mut TestAppContext,
    ) -> DialogHandle {
        cx.update_window(parent, |_, window, cx| {
            open_dialog(window, cx, content).expect("test dialog opens")
        })
        .unwrap()
    }

    #[test]
    fn content_without_override_uses_the_default_dialog_size() {
        let content = TestContent::new("test", "default size");
        assert_eq!(content.size(), DEFAULT_DIALOG_SIZE);
        assert_eq!(DEFAULT_DIALOG_SIZE, size(px(420.), px(240.)));
    }

    #[test]
    fn dialog_open_error_formats_its_message() {
        let error = DialogOpenError("failed to open test dialog".to_string());
        assert_eq!(error.to_string(), "failed to open test dialog");
    }

    #[test]
    fn dialog_bounds_centers_on_parent() {
        let parent = Bounds::new(point(px(100.), px(100.)), size(px(400.), px(300.)));
        let desired = size(px(200.), px(100.));
        let visible = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        assert_eq!(
            dialog_bounds(parent, desired, visible),
            Bounds::new(point(px(200.), px(200.)), desired)
        );
    }

    #[test]
    fn dialog_bounds_clamps_to_every_visible_edge() {
        let desired = size(px(200.), px(100.));
        let visible = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));

        // Parent at the top-left: the centered origin would be negative.
        let parent = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.)));
        assert_eq!(
            dialog_bounds(parent, desired, visible),
            Bounds::new(point(px(0.), px(0.)), desired)
        );

        // Parent at the bottom-right: the centered origin would overhang.
        let parent = Bounds::new(point(px(1820.), px(1000.)), size(px(100.), px(80.)));
        assert_eq!(
            dialog_bounds(parent, desired, visible),
            Bounds::new(point(px(1720.), px(980.)), desired)
        );
    }

    #[test]
    fn dialog_bounds_pins_when_desired_exceeds_visible() {
        let parent = Bounds::new(point(px(500.), px(500.)), size(px(100.), px(100.)));
        let visible = Bounds::new(point(px(10.), px(20.)), size(px(300.), px(200.)));
        let desired = size(px(420.), px(240.));
        assert_eq!(
            dialog_bounds(parent, desired, visible),
            Bounds::new(point(px(10.), px(20.)), desired)
        );
    }

    #[test]
    fn dialog_bounds_uses_second_monitor_visible_origin() {
        let parent = Bounds::new(point(px(2000.), px(100.)), size(px(400.), px(300.)));
        let visible = Bounds::new(point(px(1920.), px(0.)), size(px(1920.), px(1080.)));
        let desired = size(px(200.), px(100.));
        assert_eq!(
            dialog_bounds(parent, desired, visible),
            Bounds::new(point(px(2100.), px(200.)), desired)
        );
    }

    #[gpui_kit::test]
    fn dialog_reopen_replaces_content_and_keeps_one_window(cx: &mut TestAppContext) {
        let parent = open_parent(cx);

        let first = {
            let content = TestContent::new("test", "first");
            open_test_dialog(parent, content, cx)
        };
        cx.update_window(first.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();

        let content = TestContent::new("test", "second");
        let seen = content.seen.clone();
        let second = open_test_dialog(parent, content, cx);

        assert_eq!(second.window_id(), first.window_id());
        assert_eq!(cx.windows().len(), 2, "reopen must not stack windows");
        cx.update_window(second.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        assert_eq!(*seen.borrow(), "second", "content was not replaced");
        assert_eq!(
            cx.update(|cx| window_for("test", cx))
                .expect("registry keeps the dialog")
                .window_id(),
            first.window_id()
        );
    }

    #[gpui_kit::test]
    fn dialog_action_click_closes_and_registry_prunes(cx: &mut TestAppContext) {
        let parent = open_parent(cx);
        let dialog = open_test_dialog(parent, TestContent::new("test", "close me"), cx);

        cx.update_window(dialog.into(), |_, window, cx| {
            window.click("test-dialog-ok", cx)
        })
        .unwrap();

        assert_eq!(cx.windows().len(), 1, "Ok closes the dialog window");
        assert!(cx.update(|cx| window_for("test", cx)).is_none());
    }

    #[gpui_kit::test]
    fn dialog_escape_dismisses(cx: &mut TestAppContext) {
        let parent = open_parent(cx);
        let dialog = open_test_dialog(parent, TestContent::new("test", "escape me"), cx);

        cx.update_window(dialog.into(), |_, window, cx| window.press("escape", cx))
            .unwrap();

        assert_eq!(cx.windows().len(), 1, "Esc closes the dialog window");
        assert!(cx.update(|cx| window_for("test", cx)).is_none());
    }

    #[gpui_kit::test]
    fn dialog_dismissible_false_blocks_escape(cx: &mut TestAppContext) {
        let parent = open_parent(cx);
        let content = TestContent::new("test", "locked");
        content.dismissible.set(false);
        let dialog = open_test_dialog(parent, content, cx);

        cx.update_window(dialog.into(), |_, window, cx| window.press("escape", cx))
            .unwrap();

        assert_eq!(cx.windows().len(), 2, "Esc must not close a locked dialog");
        assert!(cx.update(|cx| window_for("test", cx)).is_some());
    }

    #[gpui_kit::test]
    fn dialog_cascade_closes_children_with_the_parent(cx: &mut TestAppContext) {
        let parent = open_parent(cx);
        let _dialog = open_test_dialog(parent, TestContent::new("test", "cascade"), cx);

        cx.update_window(parent, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();

        assert!(cx.windows().is_empty(), "child dialog outlived its parent");
        assert!(cx.update(|cx| window_for("test", cx)).is_none());
    }
}
