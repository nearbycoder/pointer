//! What each hotkey does. Handlers are generic over their host so the same table drives the
//! real app (the settings window's controller over every display's overlay) and headless
//! scripts (a single stage), where `{"type":"shortcut"}` events press them.

use std::sync::Arc;

use rune::Tx;

use crate::bindings::{self, ACTIONS};
use crate::input;
use crate::settings::{Settings, Shared, Toast, Tool};
use crate::style::{BLADES, Style};

pub trait Host: 'static {
    fn shared(&self) -> &Arc<Shared>;
    /// Whether the overlays take the mouse (while a drawing tool is out).
    fn capture(&mut self, on: bool);
    /// Brings up the settings window.
    fn open_settings(&mut self) {}
}

/// The keycaps an action is bound to right now, for a toast.
fn keys(s: &Settings, id: &str) -> String {
    bindings::keys(&s.bindings, id)
}

fn toast(s: &mut Settings, title: &str, detail: &str, keys: String) {
    s.toast = Toast { title: title.into(), detail: detail.into(), keys, accent: None };
    s.toast_rev += 1;
}

/// The toast for a pointer style, its dot in the style's colour (a sword's in its blade's).
fn style_toast(s: &mut Settings) {
    let accent = if s.style == Style::Saber { BLADES[s.blade % BLADES.len()] } else { s.style.accent() };
    let next = keys(s, "next-style");
    s.toast = Toast { title: s.style.name().into(), detail: s.style.hint().replace("⌃⌥S", &next), keys: next, accent: Some(accent) };
    s.toast_rev += 1;
}

pub fn set_style_on(shared: &Shared, style: Style) {
    shared.update(|s| {
        if style == Style::Saber && s.style != Style::Saber { s.blade = (s.blade + 1) % BLADES.len(); }
        s.style = style;
        s.shown = true;
        s.help = false;
        style_toast(s);
    });
}

fn next_style<H: Host>(h: &mut H, _: &mut Tx) {
    let next = h.shared().get().style.next();
    set_style_on(h.shared(), next);
}

pub fn set_tool_on(shared: &Shared, tool: Tool) {
    shared.update(|s| {
        s.tool = tool;
        s.shown = true;
        s.help = false;
        let k = keys(s, tool.action());
        toast(s, tool.name(), tool.hint(), k);
    });
}

pub fn set_tool<H: Host>(h: &mut H, tool: Tool) {
    set_tool_on(h.shared(), tool);
    h.capture(tool.captures());
}

fn toggle<H: Host>(h: &mut H, _: &mut Tx) {
    let shared = h.shared().clone();
    shared.update(|s| {
        s.shown = !s.shown;
        s.help = false;
        let k = keys(s, "toggle");
        if s.shown { toast(s, s.tool.name(), s.tool.hint(), k); } else { toast(s, "Hidden", "The pointer is tucked away", k); }
    });
    let st = shared.get();
    if !st.shown { input::set_cursor_hidden(false); }
    h.capture(st.shown && st.tool.captures());
}

fn tool<H: Host, const N: usize>(h: &mut H, _: &mut Tx) {
    set_tool(h, Tool::ALL[N]);
}

fn smash<H: Host>(h: &mut H, _: &mut Tx) {
    h.shared().update(|s| { s.shown = true; s.smash += 1; });
}

fn clear<H: Host>(h: &mut H, _: &mut Tx) {
    h.shared().update(|s| {
        let k = keys(s, "clear");
        s.clear += 1;
        toast(s, "Cleared", "Ink, cracks and focus fade away", k);
    });
}

pub fn set_scale_on(shared: &Shared, scale: f32) {
    shared.update(|s| {
        let bigger = scale > s.scale;
        s.scale = scale.clamp(0.4, 3.0);
        let detail = format!("{} bigger · {} smaller", keys(s, "bigger"), keys(s, "smaller"));
        let k = keys(s, if bigger { "bigger" } else { "smaller" });
        toast(s, &format!("Size {:.0}%", s.scale * 100.0), &detail, k);
    });
}

fn resize<H: Host, const BIGGER: bool>(h: &mut H, _: &mut Tx) {
    let s = h.shared().get().scale;
    set_scale_on(h.shared(), if BIGGER { s * 1.15 } else { s / 1.15 });
}

fn cursor<H: Host>(h: &mut H, _: &mut Tx) {
    h.shared().update(|s| {
        s.hide_cursor = !s.hide_cursor;
        let (title, detail) = if s.hide_cursor { ("Cursor hidden", "The fingertip is your cursor") } else { ("Cursor shown", "The arrow stays under the glove") };
        let k = keys(s, "cursor");
        toast(s, title, detail, k);
    });
    if !h.shared().get().hide_cursor { input::set_cursor_hidden(false); }
}

fn sound<H: Host>(h: &mut H, _: &mut Tx) {
    h.shared().update(|s| {
        s.sound = !s.sound;
        let (title, detail) = if s.sound { ("Sound on", "Taps, cracks and tools make noise") } else { ("Sound off", "Silent pointing") };
        let k = keys(s, "sound");
        toast(s, title, detail, k);
    });
}

fn perf<H: Host>(h: &mut H, _: &mut Tx) {
    h.shared().update(|s| s.perf = !s.perf);
}

fn help<H: Host>(h: &mut H, _: &mut Tx) {
    h.shared().update(|s| s.help = !s.help);
}

fn settings<H: Host>(h: &mut H, _: &mut Tx) {
    h.open_settings();
}

fn quit<H: Host>(_: &mut H, tx: &mut Tx) {
    input::set_cursor_hidden(false);
    tx.quit();
}

/// Escape, while a drawing tool holds the mouse: back to the plain pointer.
pub fn escape<H: Host>(h: &mut H, _: &mut Tx) {
    set_tool(h, Tool::Tap);
}

pub type Handler<H> = fn(&mut H, &mut Tx);

/// The handler for an action in `bindings::ACTIONS`.
pub fn handler<H: Host>(id: &str) -> Option<Handler<H>> {
    let f: Handler<H> = match id {
        "toggle" => toggle::<H>,
        "next-style" => next_style::<H>,
        "tool-pointer" => tool::<H, 0>,
        "tool-spotlight" => tool::<H, 1>,
        "tool-laser" => tool::<H, 2>,
        "tool-pen" => tool::<H, 3>,
        "tool-focus" => tool::<H, 4>,
        "smash" => smash::<H>,
        "clear" => clear::<H>,
        "bigger" => resize::<H, true>,
        "smaller" => resize::<H, false>,
        "cursor" => cursor::<H>,
        "sound" => sound::<H>,
        "perf" => perf::<H>,
        "help" => help::<H>,
        "settings" => settings::<H>,
        "quit" => quit::<H>,
        _ => return None,
    };
    Some(f)
}

/// Every action with its default chord and handler (headless runs register these).
pub fn defaults<H: Host>() -> Vec<(&'static str, Handler<H>)> {
    ACTIONS.iter().filter_map(|a| handler::<H>(a.id).map(|h| (a.default, h))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nobody(Arc<Shared>);
    impl Host for Nobody {
        fn shared(&self) -> &Arc<Shared> { &self.0 }
        fn capture(&mut self, _: bool) {}
    }

    #[test]
    fn every_action_has_a_handler() {
        for a in ACTIONS { assert!(handler::<Nobody>(a.id).is_some(), "{} has no handler", a.id); }
        let _ = Nobody(Shared::load(false));
    }
}
