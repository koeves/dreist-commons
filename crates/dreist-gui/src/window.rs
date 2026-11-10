//! Editor window: baseview for the native view and events, softbuffer to
//! present the tiny-skia frame.

use std::num::NonZeroU32;
use std::ptr::NonNull;

use baseview::{Event, EventStatus, MouseEvent, Size, Window, WindowEvent, WindowHandler, WindowOpenOptions, WindowScalePolicy};
use raw_window_handle as rwh;
use raw_window_handle_05 as rwh05;
use rwh05::HasRawWindowHandle;

use crate::app::{App, MouseButton, UiEvent};

pub use baseview::WindowHandle;
pub use rwh05::HasRawWindowHandle as RawHandle;

/// Called with each UI event and once per frame (`None`) so the owner can
/// sync engine state into the control tree.
pub type Hook = Box<dyn FnMut(&mut App, Option<&UiEvent>) + Send>;

/// What the window drives: an [`App`] plus event/frame callbacks.
pub trait WindowApp: 'static {
    fn app(&mut self) -> &mut App;
    fn event(&mut self, ev: &UiEvent);
    /// Once per frame before drawing.
    fn frame(&mut self);
    /// Keys the app does not use are still consumed (a standalone app:
    /// passing them on makes AppKit beep). Plugins pass them to the host.
    fn captures_keys(&self) -> bool {
        false
    }
    fn render(&mut self, scale: f32) -> &tiny_skia::Pixmap {
        self.app().render_frame_with(scale, None)
    }
}

/// An [`App`] with a [`Hook`].
pub struct Hooked(pub App, pub Hook);

impl WindowApp for Hooked {
    fn app(&mut self) -> &mut App {
        &mut self.0
    }
    fn event(&mut self, ev: &UiEvent) {
        (self.1)(&mut self.0, Some(ev));
    }
    fn frame(&mut self) {
        (self.1)(&mut self.0, None);
    }
}

/// Adapts a baseview window's rwh 0.5 handle to the rwh 0.6 traits
/// softbuffer needs.
struct Handles {
    window: rwh::RawWindowHandle,
    display: rwh::RawDisplayHandle,
}

impl rwh::HasWindowHandle for Handles {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        Ok(unsafe { rwh::WindowHandle::borrow_raw(self.window) })
    }
}

impl rwh::HasDisplayHandle for Handles {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(self.display) })
    }
}

fn handles(window: &Window) -> Option<Handles> {
    match window.raw_window_handle() {
        rwh05::RawWindowHandle::AppKit(h) => {
            let view = NonNull::new(h.ns_view)?;
            Some(Handles { window: rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(view)), display: rwh::RawDisplayHandle::AppKit(rwh::AppKitDisplayHandle::new()) })
        }
        rwh05::RawWindowHandle::Win32(h) => {
            let hwnd = std::num::NonZeroIsize::new(h.hwnd as isize)?;
            Some(Handles { window: rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(hwnd)), display: rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()) })
        }
        rwh05::RawWindowHandle::Xlib(h) => {
            let mut d = rwh::XlibDisplayHandle::new(None, 0);
            d.display = None;
            Some(Handles { window: rwh::RawWindowHandle::Xlib(rwh::XlibWindowHandle::new(h.window)), display: rwh::RawDisplayHandle::Xlib(d) })
        }
        _ => None,
    }
}

struct Handler<T: WindowApp> {
    app: T,
    surface: Option<softbuffer::Surface<Handles, Handles>>,
    scale: f64,
    physical: (u32, u32),
    /// The cursor last applied.
    cursor: crate::cursor::Cursor,
}

impl<T: WindowApp> Handler<T> {
    fn dispatch(&mut self, events: Vec<UiEvent>) {
        for e in &events {
            self.app.event(e);
        }
    }

    /// Ask for the tooltip of the control under the pointer.
    fn tooltip(&mut self) {
        let path = self.app.app().tooltip_target();
        self.app.event(&UiEvent::ToolTip { path });
    }
}

impl<T: WindowApp> WindowHandler for Handler<T> {
    fn on_frame(&mut self, window: &mut Window) {
        self.app.frame();
        // Live UI scale changes ([re:2fbdeb] + host resize).
        if std::mem::take(&mut self.app.app().resize_request) {
            let (w, h) = self.app.app().size();
            let ui = self.app.app().ui_scale.max(0.25) as f64;
            window.resize(Size::new(w as f64 * ui, h as f64 * ui));
            self.app.app().dirty = true;
        }
        if !self.app.app().needs_redraw() {
            return;
        }
        let scale = self.scale as f32;
        if self.surface.is_none() {
            return;
        }
        let ui = self.app.app().ui_scale.max(0.25);
        let pm = self.app.render(scale * ui);
        let Some(surface) = self.surface.as_mut() else { return };
        // Debug aid: DREIST_GUI_DUMP=path.png saves each presented frame.
        if let Ok(p) = std::env::var("DREIST_GUI_DUMP") {
            let _ = pm.save_png(p);
        }
        let (w, h) = (pm.width(), pm.height());
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else { return };
        if surface.resize(nw, nh).is_err() {
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        for (d, s) in buf.iter_mut().zip(pm.data().as_chunks::<4>().0) {
            // Opaque backdrop, so premultiplied == straight: RGBA bytes to
            // 0RGB (a form the compiler vectorizes).
            let v = u32::from_le_bytes(*s);
            *d = (v & 0xff) << 16 | (v & 0xff00) | ((v >> 16) & 0xff);
        }
        let _ = buf.present();
        self.physical = (w, h);
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        let set_mods = |app: &mut App, m: keyboard_types::Modifiers| {
            app.mods = crate::app::Mods {
                shift: m.contains(keyboard_types::Modifiers::SHIFT),
                cmd: m.contains(keyboard_types::Modifiers::META),
                alt: m.contains(keyboard_types::Modifiers::ALT),
                ctrl: m.contains(keyboard_types::Modifiers::CONTROL),
            };
        };
        match &event {
            Event::Mouse(
                MouseEvent::CursorMoved { modifiers, .. } | MouseEvent::ButtonPressed { modifiers, .. } | MouseEvent::ButtonReleased { modifiers, .. } | MouseEvent::WheelScrolled { modifiers, .. },
            ) => set_mods(self.app.app(), *modifiers),
            Event::Keyboard(k) => set_mods(self.app.app(), k.modifiers),
            _ => {}
        }
        match event {
            Event::Mouse(MouseEvent::CursorMoved { position, .. }) => {
                let ui = self.app.app().ui_scale.max(0.25);
                let ev = self.app.app().mouse_move(position.x as f32 / ui, position.y as f32 / ui);
                self.dispatch(ev);
                self.tooltip();
                // A drag left the window with files to give: the OS drag.
                if let Some(files) = self.app.app().drag_out.take() {
                    let view = match _window.raw_window_handle() {
                        rwh05::RawWindowHandle::AppKit(h) => h.ns_view,
                        _ => std::ptr::null_mut(),
                    };
                    if crate::dragout::begin(view, &files) {
                        self.app.app().end_press();
                    }
                }
            }
            Event::Mouse(MouseEvent::ButtonPressed { button, .. }) => {
                let b = if button == baseview::MouseButton::Right { MouseButton::Right } else { MouseButton::Left };
                let ev = self.app.app().mouse_down(b);
                self.dispatch(ev);
                self.tooltip();
            }
            Event::Mouse(MouseEvent::ButtonReleased { button, .. }) => {
                let b = if button == baseview::MouseButton::Right { MouseButton::Right } else { MouseButton::Left };
                let ev = self.app.app().mouse_up(b);
                self.dispatch(ev);
            }
            Event::Mouse(MouseEvent::CursorLeft) => {
                self.app.app().mouse_left();
            }
            Event::Mouse(MouseEvent::WheelScrolled { delta, .. }) => {
                // Both axes: scroll containers take a sideways trackpad
                // swipe as such; other controls get the dominant axis
                // (the mixer's channel scroller has no scroll bar).
                let (dx, dy, lines) = match delta {
                    baseview::ScrollDelta::Lines { x, y } => (x, y, true),
                    baseview::ScrollDelta::Pixels { x, y } => (x, y, false),
                };
                let ev = self.app.app().mouse_scroll(dx, dy, lines);
                self.dispatch(ev);
                self.tooltip();
            }
            Event::Keyboard(k) => {
                if k.state != keyboard_types::KeyState::Down {
                    return if self.app.app().is_editing() { EventStatus::Captured } else { EventStatus::Ignored };
                }
                match &k.key {
                    keyboard_types::Key::Character(s) => {
                        let cmd = self.app.app().mods.cmd;
                        let capture = self.app.captures_keys();
                        if cmd || !self.app.app().is_editing() {
                            let ev = self.app.app().shortcut(&s.to_lowercase());
                            if ev.is_empty() {
                                return if capture { EventStatus::Captured } else { EventStatus::Ignored };
                            }
                            self.dispatch(ev);
                            return if cmd || capture { EventStatus::Captured } else { EventStatus::Ignored };
                        }
                        if !self.app.app().key_text(s) {
                            return EventStatus::Ignored;
                        }
                    }
                    other if !self.app.app().is_editing() => {
                        let key = match other {
                            keyboard_types::Key::Backspace | keyboard_types::Key::Delete => "delete",
                            keyboard_types::Key::ArrowLeft => "left",
                            keyboard_types::Key::ArrowRight => "right",
                            keyboard_types::Key::ArrowUp => "up",
                            keyboard_types::Key::ArrowDown => "down",
                            keyboard_types::Key::Enter => "enter",
                            _ => return if self.app.captures_keys() { EventStatus::Captured } else { EventStatus::Ignored },
                        };
                        let ev = self.app.app().shortcut(key);
                        self.dispatch(ev);
                    }
                    other => {
                        let name = match other {
                            keyboard_types::Key::Enter => "Enter",
                            keyboard_types::Key::Escape => "Escape",
                            keyboard_types::Key::Backspace => "Backspace",
                            keyboard_types::Key::Tab => "Tab",
                            _ => "",
                        };
                        let (ev, used) = self.app.app().key_named(name);
                        self.dispatch(ev);
                        if !used {
                            return EventStatus::Ignored;
                        }
                    }
                }
            }
            Event::Mouse(MouseEvent::DragEntered { position, data, .. } | MouseEvent::DragMoved { position, data, .. }) => {
                let baseview::DropData::Files(files) = data else { return EventStatus::Ignored };
                let ui = self.app.app().ui_scale.max(0.25);
                let ev = self.app.app().files(position.x as f32 / ui, position.y as f32 / ui, files, false);
                self.dispatch(ev);
                self.apply_cursor();
                return EventStatus::AcceptDrop(baseview::DropEffect::Copy);
            }
            Event::Mouse(MouseEvent::DragDropped { position, data, .. }) => {
                let baseview::DropData::Files(files) = data else { return EventStatus::Ignored };
                let ui = self.app.app().ui_scale.max(0.25);
                let ev = self.app.app().files(position.x as f32 / ui, position.y as f32 / ui, files, true);
                self.dispatch(ev);
                self.app.app().cursor = crate::cursor::Cursor::Arrow;
                self.apply_cursor();
                return EventStatus::AcceptDrop(baseview::DropEffect::Copy);
            }
            Event::Window(WindowEvent::Resized(info)) => {
                self.scale = info.scale();
                let l = info.logical_size();
                let app = self.app.app();
                let ui = app.ui_scale.max(0.25) as f64;
                app.resize((l.width / ui).round() as i32, (l.height / ui).round() as i32);
                app.dirty = true;
            }
            _ => return EventStatus::Ignored,
        }
        self.apply_cursor();
        EventStatus::Captured
    }
}

impl<T: WindowApp> Handler<T> {
    fn apply_cursor(&mut self) {
        let want = self.app.app().cursor.clone();
        if want != self.cursor {
            crate::cursor::apply(&want, &dreist_formats::paths::ui_dir().join("Cursors"));
            self.cursor = want;
        }
    }
}

fn options<T: WindowApp>(app: &mut T, title: &str) -> WindowOpenOptions {
    let (w, h) = app.app().size();
    let ui = app.app().ui_scale.max(0.25) as f64;
    WindowOpenOptions { title: title.into(), size: Size::new(w as f64 * ui, h as f64 * ui), scale: WindowScalePolicy::SystemScaleFactor, gl_config: None }
}

fn build<T: WindowApp>(app: T) -> impl FnOnce(&mut Window) -> Handler<T> + Send + 'static {
    // The app is created on the caller's thread and only used on the
    // window thread afterwards.
    struct SendApp<T>(T);
    unsafe impl<T> Send for SendApp<T> {}
    let pack = SendApp(app);
    move |window: &mut Window| {
        let pack = pack;
        let SendApp(app) = pack;
        let surface = handles(window).and_then(|h| {
            let h2 = Handles { window: h.window, display: h.display };
            let ctx = softbuffer::Context::new(h).ok()?;
            softbuffer::Surface::new(&ctx, h2).ok()
        });
        Handler { app, surface, scale: 1.0, physical: (0, 0), cursor: crate::cursor::Cursor::Arrow }
    }
}

/// Open the editor inside a host-provided parent view.
pub fn open_parented<P: HasRawWindowHandle, T: WindowApp>(parent: &P, mut app: T) -> baseview::WindowHandle {
    let opts = options(&mut app, dreist_formats::profile::get().product);
    Window::open_parented(parent, opts, build(app))
}

/// Open a top-level window and run until it is closed.
pub fn open_blocking<T: WindowApp>(mut app: T, title: &str) {
    let opts = options(&mut app, title);
    Window::open_blocking(opts, build(app));
}
