//! The controller: the app behind the settings window. It owns the hotkeys (registered at
//! run time, so they can be changed while the app runs), steers every display's overlay,
//! answers the menu bar item, and checks for and installs updates.

use std::sync::Arc;
use std::time::Duration;

use rune::{AppHandle, Image, ImageCache, OverlayHandle, OverlayInput, Shortcut, ShortcutError, ShortcutId, Tx, Waker};
use rune_kit::ShortcutRecorderState;

use crate::actions::{self, Host};
use crate::bindings::{self, ACTIONS};
use crate::input;
use crate::login;
use crate::menubar::{self, Pick};
use crate::settings::{Settings, Shared, Toast, Tool};
use crate::stage::Stage;
use crate::update::{self, Release, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Pointer,
    Shortcuts,
    Updates,
    About,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::General, Page::Pointer, Page::Shortcuts, Page::Updates, Page::About];
    pub fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Pointer => "Pointer",
            Page::Shortcuts => "Shortcuts",
            Page::Updates => "Updates",
            Page::About => "About",
        }
    }
}

/// Pictures the settings window shows.
pub struct Images {
    pub icon: Image,
    pub thumbs: [Image; 5],
}

pub struct Ctl {
    pub shared: Arc<Shared>,
    /// The settings as the window last saw them.
    pub st: Settings,
    pub overlays: Vec<OverlayHandle<Stage>>,
    pub app: Option<AppHandle<Ctl>>,
    escape: Option<ShortcutId>,
    capturing: bool,
    /// Each action's registration, in `ACTIONS` order (`None`: unbound or refused).
    registered: Vec<Option<ShortcutId>>,
    /// The recorder showing each action's chord.
    pub recorders: Vec<ShortcutRecorderState>,
    pub page: Page,
    pub update: Status,
    /// The newest version a toast already announced.
    announced: Option<String>,
    /// Open at login: `None` where it can't be (a development build).
    pub login: Option<bool>,
    pub login_error: Option<String>,
    pub images: Option<Images>,
    /// Running as the installed app (updates install), not a development build.
    pub installed: bool,
}

const ICON: &[u8] = include_bytes!("../assets/app-icon-256.png");
const THUMBS: [&[u8]; 5] = [
    include_bytes!("../assets/thumbs/glove.png"),
    include_bytes!("../assets/thumbs/saber.png"),
    include_bytes!("../assets/thumbs/wand.png"),
    include_bytes!("../assets/thumbs/mallet.png"),
    include_bytes!("../assets/thumbs/chicken.png"),
];

impl Ctl {
    pub fn new(shared: Arc<Shared>, overlays: Vec<OverlayHandle<Stage>>, app: Option<AppHandle<Ctl>>) -> Ctl {
        let st = shared.get();
        let recorders = ACTIONS.iter().map(|a| ShortcutRecorderState::new(bindings::chord(&st.bindings, a.id))).collect();
        Ctl {
            shared,
            st,
            overlays,
            app,
            escape: None,
            capturing: false,
            registered: vec![None; ACTIONS.len()],
            recorders,
            page: Page::General,
            update: Status::Unchecked,
            announced: None,
            login: login::status(),
            login_error: None,
            images: None,
            installed: update::bundled(),
        }
    }

    /// Loads the window's pictures (with the setup waker, so they draw as they decode).
    pub fn setup(&mut self, waker: Waker) {
        self.shared.wakers.lock().unwrap_or_else(|e| e.into_inner()).push(waker.clone());
        let cache = ImageCache::new(waker);
        self.images = Some(Images { icon: cache.load(ICON), thumbs: THUMBS.map(|b| cache.load(b)) });
    }

    /// Picks up changes made elsewhere (a hotkey, the menu). True when the view must re-run.
    pub fn refresh(&mut self) -> bool {
        if self.shared.rev() == self.st.rev { return false; }
        self.st = self.shared.get();
        true
    }

    // ------------------------------------------------------------------ hotkeys

    fn handler(id: &str) -> actions::Handler<Ctl> {
        actions::handler::<Ctl>(id).expect("every action has a handler")
    }

    /// Registers every action's chord (once at launch, and after a reset).
    pub fn register_all(&mut self) {
        let Some(app) = self.app.clone() else { return };
        for (i, a) in ACTIONS.iter().enumerate() {
            if let Some(id) = self.registered[i].take() { let _ = app.unregister_shortcut(id); }
            let Some(chord) = bindings::chord(&self.st.bindings, a.id) else { continue };
            match app.register_shortcut(chord.clone(), Self::handler(a.id)) {
                Ok(id) => self.registered[i] = Some(id),
                Err(e) => {
                    eprintln!("pointer: {} for “{}” isn't available ({e}); change it in Settings", chord.keycaps().concat(), a.label);
                    self.recorders[i].mark_unregistered();
                }
            }
        }
    }

    /// Moves action `i` to `chord` (or unbinds it). A refused chord keeps the old one.
    pub fn rebind(&mut self, i: usize, chord: Option<&Shortcut>) -> Result<(), ShortcutError> {
        let a = &ACTIONS[i];
        if let Some(app) = self.app.clone() {
            if let Some(id) = self.registered[i].take() { let _ = app.unregister_shortcut(id); }
            if let Some(chord) = chord {
                match app.register_shortcut(chord.clone(), Self::handler(a.id)) {
                    Ok(id) => self.registered[i] = Some(id),
                    Err(e) => {
                        if let Some(old) = bindings::chord(&self.st.bindings, a.id)
                            && let Ok(id) = app.register_shortcut(old, Self::handler(a.id))
                        {
                            self.registered[i] = Some(id);
                        }
                        return Err(e);
                    }
                }
            }
        }
        let text = chord.map(|c| c.to_string()).unwrap_or_default();
        let default = a.default.parse::<Shortcut>().map(|s| s.to_string()).ok();
        let id = a.id.to_string();
        self.shared.update(|s| {
            if Some(&text) == default.as_ref() { s.bindings.remove(&id); } else { s.bindings.insert(id, text); }
        });
        self.st = self.shared.get();
        Ok(())
    }

    /// Every hotkey back to its default.
    pub fn reset_shortcuts(&mut self) {
        self.shared.update(|s| s.bindings.clear());
        self.st = self.shared.get();
        self.register_all();
        self.recorders = ACTIONS.iter().map(|a| ShortcutRecorderState::new(bindings::chord(&self.st.bindings, a.id))).collect();
    }

    // ------------------------------------------------------------------ the window

    pub fn show_page(&mut self, page: Page) {
        self.page = page;
        self.open_settings();
    }

    pub fn settings_closed(&mut self) {
        if let Some(app) = &self.app { app.set_dock_icon(false); }
    }

    pub fn set_login(&mut self, on: bool) {
        match login::set(on) {
            Ok(()) => self.login_error = None,
            Err(e) => self.login_error = Some(e),
        }
        self.login = login::status();
    }

    pub fn menu_pick(&mut self, pick: Pick, tx: &mut Tx) {
        let id = match pick {
            Pick::Toggle => "toggle",
            Pick::NextStyle => "next-style",
            Pick::ToolPointer => "tool-pointer",
            Pick::ToolSpotlight => "tool-spotlight",
            Pick::ToolLaser => "tool-laser",
            Pick::ToolPen => "tool-pen",
            Pick::ToolFocus => "tool-focus",
            Pick::Settings => "settings",
            Pick::Quit => "quit",
            Pick::Updates => {
                self.show_page(Page::Updates);
                if !matches!(self.update, Status::Available(_)) { self.check_updates(true); }
                return;
            }
        };
        (Self::handler(id))(self, tx);
    }

    // ------------------------------------------------------------------ updates

    /// Asks GitHub for the latest release. `asked`: the user wants an answer either way.
    pub fn check_updates(&mut self, asked: bool) {
        let Some(app) = self.app.clone() else { return };
        if matches!(self.update, Status::Checking | Status::Installing(..)) { return; }
        if asked { self.update = Status::Checking; }
        std::thread::spawn(move || {
            let result = update::check();
            app.update(move |c, _| c.checked(result, asked));
        });
    }

    fn checked(&mut self, result: Result<Option<Release>, String>, asked: bool) {
        match result {
            Ok(Some(release)) => {
                menubar::set_update(Some(&release.version));
                if !asked && self.announced.as_deref() != Some(release.version.as_str()) {
                    self.announced = Some(release.version.clone());
                    let keys = bindings::keys(&self.st.bindings, "settings");
                    let title = format!("Pointer {} is available", release.version);
                    self.shared.update(|s| {
                        s.toast = Toast { title, detail: format!("Install it from Settings ({keys}) or the menu bar"), keys, accent: Some([0.48, 0.38, 1.0]) };
                        s.toast_rev += 1;
                    });
                }
                self.update = Status::Available(release);
            }
            Ok(None) => self.update = Status::Current,
            Err(e) if asked => self.update = Status::Failed(e),
            Err(e) => eprintln!("pointer: update check failed: {e}"),
        }
    }

    /// Looks for updates now, then every six hours, while the setting is on.
    pub fn start_update_checks(&self) {
        let (Some(app), shared) = (self.app.clone(), self.shared.clone()) else { return };
        std::thread::Builder::new().name("pointer-updates".into()).spawn(move || {
            std::thread::sleep(Duration::from_secs(8));
            loop {
                if shared.get().auto_update { app.update(|c, _| c.check_updates(false)); }
                std::thread::sleep(Duration::from_secs(6 * 60 * 60));
            }
        }).expect("update thread");
    }

    /// Downloads, checks and installs the available update, then relaunches into it.
    pub fn install_update(&mut self) {
        let Status::Available(release) = self.update.clone() else { return };
        let Some(app) = self.app.clone() else { return };
        self.update = Status::Installing(release.clone(), "Starting");
        std::thread::spawn(move || {
            let progress = app.clone();
            let shown = release.clone();
            let result = update::install(&release, move |step| {
                let shown = shown.clone();
                progress.update(move |c, _| c.update = Status::Installing(shown, step));
            });
            app.update(move |c, tx| match result {
                Ok(bundle) => {
                    update::relaunch(&bundle);
                    input::set_cursor_hidden(false);
                    tx.quit();
                }
                Err(e) => c.update = Status::Failed(e),
            });
        });
    }
}

impl Host for Ctl {
    fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    fn capture(&mut self, on: bool) {
        if on == self.capturing { return; }
        self.capturing = on;
        for o in &self.overlays { o.set_input(if on { OverlayInput::All } else { OverlayInput::None }); }
        let Some(app) = &self.app else { return };
        if on {
            match "escape".parse() {
                Ok(chord) => match app.register_shortcut(chord, |c: &mut Ctl, tx| actions::escape(c, tx)) {
                    Ok(id) => self.escape = Some(id),
                    Err(e) => eprintln!("pointer: couldn't take Esc while drawing ({e})"),
                },
                Err(e) => eprintln!("pointer: {e}"),
            }
        } else if let Some(id) = self.escape.take() {
            let _ = app.unregister_shortcut(id);
        }
    }

    fn open_settings(&mut self) {
        self.st = self.shared.get();
        self.login = login::status();
        if let Some(app) = &self.app {
            app.set_dock_icon(true);
            app.open();
        }
    }
}

/// Picks a tool from the settings window (as its hotkey would).
pub fn pick_tool(c: &mut Ctl, tool: Tool) {
    actions::set_tool(c, tool);
    c.st = c.shared.get();
}
