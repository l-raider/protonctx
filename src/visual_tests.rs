//! Headless windows on Slint's software renderer: tests that the UI routes
//! clicks and keys to the right callbacks, and renders of interactive states
//! (hover, menus, dialogs) for comparing the port with the gpui reference by
//! eye.
//!
//! The renders are ignored by default; run them with
//! `PROTONCTX_SNAPSHOT_DIR=/some/dir cargo test visual_states -- --ignored`.

use std::cell::RefCell;
use std::rc::Rc;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};

use crate::ui::{
    AboutDialog, DeleteCacheDialog, ErrorDialog, GameRowData, MainWindow, SettingsDialog,
};

/// A platform whose windows are software-rendered buffers.
struct Headless {
    windows: RefCell<Vec<Rc<MinimalSoftwareWindow>>>,
}

impl slint::platform::Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        self.windows.borrow_mut().push(window.clone());
        Ok(window)
    }
}

thread_local! {
    static WINDOWS: RefCell<Option<Rc<Headless>>> = const { RefCell::new(None) };
}

struct Shared(Rc<Headless>);
impl slint::platform::Platform for Shared {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        self.0.create_window_adapter()
    }
}

fn platform() -> Rc<Headless> {
    WINDOWS.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| {
                let headless = Rc::new(Headless {
                    windows: RefCell::new(Vec::new()),
                });
                slint::platform::set_platform(Box::new(Shared(headless.clone())))
                    .expect("the headless platform installs once");
                headless
            })
            .clone()
    })
}

/// Render the most recently created window into `name`.png.
fn save(window: &MinimalSoftwareWindow, width: u32, height: u32, name: &str) {
    let Some(dir) = std::env::var_os("PROTONCTX_SNAPSHOT_DIR") else {
        return;
    };
    window.set_size(slint::PhysicalSize::new(width, height));
    window.request_redraw();
    let mut buffer = vec![PremultipliedRgbaColor::default(); (width * height) as usize];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, width as usize);
    });
    let mut bytes = Vec::with_capacity(buffer.len() * 4);
    for pixel in &buffer {
        bytes.extend_from_slice(&[pixel.red, pixel.green, pixel.blue, 255]);
    }
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("snapshot file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes))
        .expect("snapshot encodes");
}

fn pointer(window: &slint::Window, x: f32, y: f32) {
    window.dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(x, y),
    });
}

fn click(window: &slint::Window, x: f32, y: f32, button: PointerEventButton) {
    pointer(window, x, y);
    let position = LogicalPosition::new(x, y);
    window.dispatch_event(WindowEvent::PointerPressed { position, button });
    window.dispatch_event(WindowEvent::PointerReleased { position, button });
}

fn rows() -> Vec<GameRowData> {
    [
        ("Broforce", "274190", "GE-Proton10-34"),
        ("Castle Crashers", "204360", "proton_hotfix"),
        ("Enter the Gungeon", "311690", "(default)"),
        (
            "Raptor: Call of The Shadows - 2015 Edition",
            "336060",
            "proton_experimental",
        ),
        (
            "The Disney Afternoon Collection",
            "525040",
            "Proton - Experimental (default)",
        ),
        ("Worms Crazy Golf", "70620", "(default)"),
    ]
    .iter()
    .map(|(name, id, tool)| GameRowData {
        name: (*name).into(),
        app_id: (*id).into(),
        tool: (*tool).into(),
    })
    .collect()
}

#[test]
#[ignore]
fn visual_states() {
    let platform = platform();
    let tokens = crate::theme::build(&crate::system_theme::load(None));

    let main = MainWindow::new().unwrap();
    crate::ui::apply_theme(&main, &tokens);
    let adapter = platform.windows.borrow().last().unwrap().clone();
    main.set_rows(slint::ModelRc::new(slint::VecModel::from(rows())));
    main.set_selected(1);
    main.set_status_text("6 games loaded".into());
    main.set_log_text("[02:57:17.809] Refreshing games...\n".into());
    main.set_log_lines(1);
    main.show().unwrap();
    let window = main.window();

    save(&adapter, 760, 520, "slint-main-idle");
    pointer(window, 200.0, 33.0 + 28.0 + 28.0 * 2.5);
    save(&adapter, 760, 520, "slint-main-hover-row");
    pointer(window, 80.0, 16.0);
    save(&adapter, 760, 520, "slint-main-hover-refresh");

    // The hamburger menu, hovered on its second item.
    click(window, 16.0, 16.0, PointerEventButton::Left);
    pointer(window, 40.0, 72.0);
    save(&adapter, 760, 520, "slint-main-menu");
    click(window, 700.0, 300.0, PointerEventButton::Left);

    // The row context menu over the third row.
    click(
        window,
        200.0,
        33.0 + 28.0 + 28.0 * 2.5,
        PointerEventButton::Right,
    );
    pointer(window, 230.0, 33.0 + 28.0 + 28.0 * 2.5 + 70.0);
    save(&adapter, 760, 520, "slint-main-context-menu");
    click(window, 700.0, 450.0, PointerEventButton::Left);

    // Enough games and log text for both scroll bar lanes in each pane; the
    // pointer rests on the table's vertical handle.
    let many: Vec<GameRowData> = std::iter::repeat_with(rows).take(5).flatten().collect();
    main.set_rows(slint::ModelRc::new(slint::VecModel::from(many)));
    main.set_selected(-1);
    let log: String = (0..12)
        .map(|i| {
            format!(
                "[02:57:17.{i:03}] wine: {}\n",
                "call to unimplemented function ".repeat(4)
            )
        })
        .collect();
    main.set_log_text(log.into());
    main.set_log_lines(12);
    main.invoke_log_appended();
    save(&adapter, 760, 520, "slint-main-scrollbars");
    pointer(window, 740.0, 80.0);
    save(&adapter, 760, 520, "slint-main-scrollbar-hover");
    drop(main);

    let dialog = AboutDialog::new().unwrap();
    crate::ui::apply_theme(&dialog, &tokens);
    dialog.set_version("protonctx v1.4.0".into());
    dialog.show().unwrap();
    let adapter = platform.windows.borrow().last().unwrap().clone();
    save(&adapter, 400, 200, "slint-dialog-about");

    let dialog = SettingsDialog::new().unwrap();
    crate::ui::apply_theme(&dialog, &tokens);
    dialog.show().unwrap();
    let adapter = platform.windows.borrow().last().unwrap().clone();
    save(&adapter, 300, 100, "slint-dialog-settings");

    let dialog = ErrorDialog::new().unwrap();
    crate::ui::apply_theme(&dialog, &tokens);
    dialog.set_message(
        "Failed to launch winecfg: no Proton directory found for this game (has it been run yet?)"
            .into(),
    );
    dialog.show().unwrap();
    let adapter = platform.windows.borrow().last().unwrap().clone();
    save(&adapter, 500, 130, "slint-dialog-launch-error");

    let dialog = DeleteCacheDialog::new().unwrap();
    crate::ui::apply_theme(&dialog, &tokens);
    dialog.set_message(
        "This will permanently delete the shader cache directory:\n\n\
         /home/lraider/.local/share/Steam/steamapps/shadercache/311690\n\n\
         Shader caches are rebuilt automatically the next time the game runs, \
         but for best results close the game before deleting."
            .into(),
    );
    dialog.show().unwrap();
    let adapter = platform.windows.borrow().last().unwrap().clone();
    save(&adapter, 520, 230, "slint-dialog-delete-cache");
}

/// Records which callbacks fired, in order.
#[derive(Default)]
struct Calls(RefCell<Vec<String>>);

impl Calls {
    fn push(&self, call: impl Into<String>) {
        self.0.borrow_mut().push(call.into());
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.0.borrow_mut())
    }
}

fn key(window: &slint::Window, text: impl Into<slint::SharedString>) {
    let text = text.into();
    window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window.dispatch_event(WindowEvent::KeyReleased { text });
}

/// The main window's mouse and keyboard wiring, on the deterministic preset
/// (13.33 px font: a 33 px toolbar, the table frame at x 3).
#[test]
fn main_window_routes_input_to_its_callbacks() {
    let _platform = platform();
    let tokens = crate::theme::build(&crate::system_theme::preset(&Default::default(), true));
    let main = MainWindow::new().unwrap();
    crate::ui::apply_theme(&main, &tokens);
    main.set_rows(slint::ModelRc::new(slint::VecModel::from(rows())));
    main.show().unwrap();
    let window = main.window();
    window.set_size(slint::LogicalSize::new(760.0, 520.0));

    let calls = Rc::new(Calls::default());
    let c = calls.clone();
    main.on_refresh(move || c.push("refresh"));
    let c = calls.clone();
    main.on_open_settings(move || c.push("settings"));
    let c = calls.clone();
    main.on_clear_logs(move || c.push("clear"));
    let c = calls.clone();
    main.on_header_clicked(move |column| c.push(format!("header {column}")));
    let c = calls.clone();
    main.on_row_pressed(move |row| c.push(format!("row {row}")));
    let c = calls.clone();
    main.on_launch_tool(move |tool| c.push(format!("launch {tool}")));

    let row_y = |row: f32| 33.0 + 1.0 + 28.0 + 28.0 * row + 14.0;

    // Keyboard shortcuts.
    key(window, slint::platform::Key::F5);
    assert_eq!(calls.take(), ["refresh"]);

    // Header clicks pick the column under the pointer.
    click(window, 319.0, 48.0, PointerEventButton::Left);
    assert_eq!(calls.take(), ["header 1"]);

    // Left and right presses select the row; the right one opens the
    // context menu, whose fourth entry is Registry Editor.
    click(window, 200.0, row_y(2.0), PointerEventButton::Left);
    assert_eq!(calls.take(), ["row 2"]);
    click(window, 200.0, row_y(3.0), PointerEventButton::Right);
    assert_eq!(calls.take(), ["row 3"]);
    let menu_top = row_y(3.0);
    click(
        window,
        230.0,
        menu_top + 4.0 + 28.0 + 9.0 + 28.0 + 14.0,
        PointerEventButton::Left,
    );
    assert_eq!(calls.take(), ["launch regedit"]);

    // The Menu key and Shift+F10 open the same menu under the selected row
    // (row 3 ends at y 174; the menu opens at x 36).
    main.set_selected(3);
    let registry_editor = 174.0 + 4.0 + 28.0 + 9.0 + 28.0 + 14.0;
    key(window, slint::platform::Key::Menu);
    click(window, 60.0, registry_editor, PointerEventButton::Left);
    assert_eq!(calls.take(), ["launch regedit"]);
    window.dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Shift.into(),
    });
    key(window, slint::platform::Key::F10);
    window.dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Shift.into(),
    });
    click(window, 60.0, registry_editor, PointerEventButton::Left);
    assert_eq!(calls.take(), ["launch regedit"]);

    // The hamburger menu: Settings, then Clear logs after reopening.
    click(window, 16.0, 16.0, PointerEventButton::Left);
    click(window, 40.0, 35.0 + 4.0 + 14.0, PointerEventButton::Left);
    assert_eq!(calls.take(), ["settings"]);
    click(window, 16.0, 16.0, PointerEventButton::Left);
    click(
        window,
        40.0,
        35.0 + 4.0 + 28.0 + 14.0,
        PointerEventButton::Left,
    );
    assert_eq!(calls.take(), ["clear"]);

    // While a dialog is open the main window ignores input.
    main.set_dialog_open(true);
    key(window, slint::platform::Key::F5);
    click(window, 200.0, row_y(1.0), PointerEventButton::Left);
    click(window, 16.0, 16.0, PointerEventButton::Left);
    assert!(calls.take().is_empty());
}

/// Esc dismisses every dialog; Enter picks the default button, which is No
/// for the destructive one.
#[test]
fn dialog_keys_dismiss_or_pick_the_default() {
    let _platform = platform();
    let calls = Rc::new(Calls::default());

    let about = AboutDialog::new().unwrap();
    about.show().unwrap();
    let c = calls.clone();
    about.on_ok(move || c.push("ok"));
    key(about.window(), slint::platform::Key::Escape);
    key(about.window(), slint::platform::Key::Return);
    assert_eq!(calls.take(), ["ok", "ok"]);

    let delete = DeleteCacheDialog::new().unwrap();
    delete.show().unwrap();
    let c = calls.clone();
    delete.on_yes(move || c.push("yes"));
    let c = calls.clone();
    delete.on_no(move || c.push("no"));
    key(delete.window(), slint::platform::Key::Return);
    key(delete.window(), slint::platform::Key::Escape);
    assert_eq!(calls.take(), ["no", "no"]);
}
