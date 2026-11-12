//! Okno 1000×700 z WebView2 (bez paska tytułu Windows; pasek rysuje interfejs).
//! Interfejs (ui/dist/index.html, jeden plik) jest wkompilowany w program.
use super::app::{Action, App};
use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowBuilderExtWindows;
use tao::window::{Icon, WindowBuilder};
use wry::WebViewBuilder;

pub enum UserEvent {
    /// JSON do interfejsu (odpowiedź albo zdarzenie).
    Emit(String),
    /// Wiadomość z interfejsu: obsługiwana w pętli okna, poza wywołaniem zwrotnym WebView2.
    Call(String),
}

static UI: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.html"));

fn apps_dark() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .is_ok_and(|v| v == 0)
}

fn icon() -> Option<Icon> {
    // Ikona okna z pliku .ico wkompilowanego w program (zasób 1, data/shrimp-install.rc).
    use tao::platform::windows::IconExtWindows;
    Icon::from_resource(1, None).ok()
}

pub type Proxy = Arc<Mutex<EventLoopProxy<UserEvent>>>;

/// Tworzy pętlę okna; `ready` dostaje pośrednika zdarzeń, zanim okno zacznie działać.
pub fn run(make_app: impl FnOnce(Proxy) -> Arc<App>, opts_json: String, tr: impl Fn(usize) -> String) -> i32 {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy: Proxy = Arc::new(Mutex::new(event_loop.create_proxy()));
    let app = make_app(proxy.clone());
    let dark = app.opts.theme.as_deref().map_or_else(apps_dark, |t| t == "dark");

    let window = match WindowBuilder::new()
        .with_title("Shrimp Install")
        .with_inner_size(LogicalSize::new(1000.0, 700.0))
        .with_min_inner_size(LogicalSize::new(800.0, 560.0))
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_window_icon(icon())
        .with_theme(Some(if dark { tao::window::Theme::Dark } else { tao::window::Theme::Light }))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => {
            crate::log!("okno: {e}");
            super::sys::message(&tr(1));
            return 1;
        }
    };
    if UI.is_empty() {
        super::sys::message(&tr(3));
        return 1;
    }

    let ipc = proxy.clone();
    let navigations = std::sync::atomic::AtomicUsize::new(0);
    let devtools = std::env::var_os("SHRIMP_DEVTOOLS").is_some();
    let init = format!(
        "window.SHRIMP_OPTS={opts_json};document.addEventListener('contextmenu',function(e){{if(!{devtools})e.preventDefault()}});"
    );
    let webview = WebViewBuilder::new()
        .with_html(UI)
        .with_initialization_script(&init)
        .with_background_color(if dark { (0x20, 0x20, 0x20, 255) } else { (0xF3, 0xF3, 0xF3, 255) })
        .with_devtools(devtools)
        .with_hotkeys_zoom(false)
        .with_ipc_handler(move |req| {
            let _ = ipc.lock().unwrap().send_event(UserEvent::Call(req.body().clone()));
        })
        // Tylko pierwsza nawigacja (nasz interfejs); żadnych przejść do innych stron ani nowych okien.
        .with_navigation_handler(move |_| {
            navigations.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0
        })
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .build(&window);
    let webview = match webview {
        Ok(w) => w,
        Err(e) => {
            crate::log!("WebView2: {e}");
            super::sys::message(&tr(0));
            return 1;
        }
    };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Emit(json)) => {
                let _ = webview.evaluate_script(&format!("window.__shrimpRecv&&window.__shrimpRecv({json})"));
            }
            Event::UserEvent(UserEvent::Call(msg)) => match app.dispatch(&msg) {
                Action::None => {}
                Action::Close => close(&app, flow),
                Action::Minimize => window.set_minimized(true),
                Action::Drag => {
                    let _ = window.drag_window();
                }
                Action::Pick { id, folder } => {
                    let d = rfd::FileDialog::new().set_parent(&window);
                    let picked = if folder { d.set_title("Dodaj folder").pick_folders() } else { d.set_title("Dodaj plik").pick_files() };
                    app.picked(id, folder, picked.unwrap_or_default());
                }
            },
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => close(&app, flow),
            _ => {}
        }
    });
}

/// Zamknięcie w trakcie przygotowania: interfejs pyta, czy anulować.
fn close(app: &App, flow: &mut ControlFlow) {
    if app.busy() {
        app.ctx.emit(r#"{"event":"closeRequested"}"#.into());
    } else {
        *flow = ControlFlow::Exit;
    }
}
