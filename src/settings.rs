//! What the user has chosen, shared between the hotkey handlers and every display's overlay.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rune::Waker;
use rune::prefs::Prefs;

use crate::style::{BLADES, Style};

const PREFS_ID: &str = "Pointer";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Taps ripple; rapid taps or a long press crack the glass.
    Tap,
    /// Dims the screen except around the fingertip.
    Spotlight,
    /// The fingertip leaves a fading laser trail.
    Laser,
    /// Draw ink (takes the mouse until you switch tools or press Esc).
    Pen,
    /// Drag a box to dim everything else (takes the mouse).
    Focus,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Tap, Tool::Spotlight, Tool::Laser, Tool::Pen, Tool::Focus];

    /// Whether the overlay takes the mouse while this tool is out, so drawing doesn't click
    /// the app underneath.
    pub fn captures(self) -> bool {
        matches!(self, Tool::Pen | Tool::Focus)
    }
    pub fn name(self) -> &'static str {
        match self {
            Tool::Tap => "Pointer",
            Tool::Spotlight => "Spotlight",
            Tool::Laser => "Laser",
            Tool::Pen => "Pen",
            Tool::Focus => "Focus",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Tap => "Click to ripple · tap fast or hold to crack",
            Tool::Spotlight => "Everything but the fingertip dims",
            Tool::Laser => "Move to draw a fading trail",
            Tool::Pen => "Drag to draw · Esc to finish",
            Tool::Focus => "Drag a box to focus it · Esc to finish",
        }
    }
    /// The hotkey action that picks this tool.
    pub fn action(self) -> &'static str {
        match self {
            Tool::Tap => "tool-pointer",
            Tool::Spotlight => "tool-spotlight",
            Tool::Laser => "tool-laser",
            Tool::Pen => "tool-pen",
            Tool::Focus => "tool-focus",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Tool::Tap => "tap",
            Tool::Spotlight => "spotlight",
            Tool::Laser => "laser",
            Tool::Pen => "pen",
            Tool::Focus => "focus",
        }
    }
    fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// A short message near the bottom of the screen the cursor is on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toast {
    pub title: String,
    pub detail: String,
    pub keys: String,
    /// The dot's colour (sRGB); the tool's when `None`.
    pub accent: Option<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    /// Whether the pointer is out.
    pub shown: bool,
    pub tool: Tool,
    /// Which pointer is out.
    pub style: Style,
    /// The energy sword's blade colour (an index into `style::BLADES`).
    pub blade: usize,
    /// The glove's size, as a multiple of the default for its display.
    pub scale: f32,
    /// Hide the system cursor while the glove is out (the fingertip is the cursor).
    pub hide_cursor: bool,
    /// Sound effects.
    pub sound: bool,
    /// The frame-rate readout.
    pub perf: bool,
    pub help: bool,
    /// Hotkeys the user has changed: an action id to its chord, or "" for none.
    pub bindings: BTreeMap<String, String>,
    /// Look for new releases on GitHub at launch and every few hours.
    pub auto_update: bool,
    /// Bumped to crack the glass at the fingertip.
    pub smash: u64,
    /// Bumped to clear ink, cracks and the focus box.
    pub clear: u64,
    /// Bumped with each new toast.
    pub toast_rev: u64,
    pub toast: Toast,
    /// Bumped on every change, so overlays can tell they need to look.
    pub rev: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shown: true,
            tool: Tool::Tap,
            style: Style::Glove,
            blade: 0,
            scale: 1.0,
            hide_cursor: true,
            sound: true,
            perf: false,
            help: false,
            bindings: BTreeMap::new(),
            auto_update: true,
            smash: 0,
            clear: 0,
            toast_rev: 0,
            toast: Toast::default(),
            rev: 0,
        }
    }
}

pub struct Shared {
    settings: Mutex<Settings>,
    prefs: Mutex<Prefs>,
    /// Each overlay's waker, so a change here reaches overlays that are resting.
    pub wakers: Mutex<Vec<Waker>>,
}

impl Shared {
    /// Settings from last time (size, tool, cursor and readout choices).
    pub fn load(persist: bool) -> Arc<Shared> {
        let prefs = if persist { Prefs::open(PREFS_ID) } else { Prefs::memory() };
        let mut s = Settings::default();
        if let Some(v) = prefs.get::<f32>("scale") { s.scale = v.clamp(0.4, 3.0); }
        if let Some(v) = prefs.get::<bool>("hide_cursor") { s.hide_cursor = v; }
        if let Some(v) = prefs.get::<bool>("perf") { s.perf = v; }
        if let Some(v) = prefs.get::<bool>("sound") { s.sound = v; }
        if let Some(v) = prefs.get::<String>("style").and_then(|id| Style::from_id(&id)) { s.style = v; }
        if let Some(v) = prefs.get::<usize>("blade") { s.blade = v % BLADES.len(); }
        if let Some(v) = prefs.get::<BTreeMap<String, String>>("bindings") { s.bindings = v; }
        if let Some(v) = prefs.get::<bool>("auto_update") { s.auto_update = v; }
        if let Some(v) = prefs.get::<String>("tool").and_then(|t| Tool::from_id(&t)) {
            // Tools that take the mouse never come back on their own at launch.
            if !v.captures() { s.tool = v; }
        }
        Arc::new(Shared {
            settings: Mutex::new(s),
            prefs: Mutex::new(prefs),
            wakers: Mutex::new(Vec::new()),
        })
    }

    pub fn get(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The current revision, without copying the settings.
    pub fn rev(&self) -> u64 {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).rev
    }

    /// Changes the settings, saves the ones worth keeping, and wakes every overlay.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) {
        let snapshot = {
            let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut s);
            s.rev += 1;
            s.clone()
        };
        let mut prefs = self.prefs.lock().unwrap_or_else(|e| e.into_inner());
        let _ = prefs.set("scale", &snapshot.scale);
        let _ = prefs.set("hide_cursor", &snapshot.hide_cursor);
        let _ = prefs.set("perf", &snapshot.perf);
        let _ = prefs.set("sound", &snapshot.sound);
        let _ = prefs.set("style", snapshot.style.id());
        let _ = prefs.set("blade", &snapshot.blade);
        let _ = prefs.set("tool", snapshot.tool.id());
        let _ = prefs.set("bindings", &snapshot.bindings);
        let _ = prefs.set("auto_update", &snapshot.auto_update);
        drop(prefs);
        self.wake_all();
    }

    /// True the first time it's asked (ever, for these preferences), then false.
    pub fn first_run(&self) -> bool {
        let mut prefs = self.prefs.lock().unwrap_or_else(|e| e.into_inner());
        if prefs.get::<bool>("welcomed").unwrap_or(false) { return false; }
        let _ = prefs.set("welcomed", &true);
        true
    }

    pub fn wake_all(&self) {
        for w in self.wakers.lock().unwrap_or_else(|e| e.into_inner()).iter() { w(); }
    }
}
