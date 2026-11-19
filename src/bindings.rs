//! Every hotkey: what it does, what it's called, and the chord it starts on. Settings hold
//! the user's changes (an action id to a chord, or "" for none); everything that shows or
//! registers a chord asks here.

use std::collections::BTreeMap;

use rune::Shortcut;

pub struct Action {
    /// Stable, for preferences.
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
}

pub const ACTIONS: &[Action] = &[
    Action { id: "toggle", label: "Show or hide the pointer", default: "ctrl+alt+p" },
    Action { id: "next-style", label: "Next pointer", default: "ctrl+alt+s" },
    Action { id: "tool-pointer", label: "Pointer tool: ripples, crack the glass", default: "ctrl+alt+1" },
    Action { id: "tool-spotlight", label: "Spotlight", default: "ctrl+alt+2" },
    Action { id: "tool-laser", label: "Laser trail", default: "ctrl+alt+3" },
    Action { id: "tool-pen", label: "Pen (Esc to finish)", default: "ctrl+alt+4" },
    Action { id: "tool-focus", label: "Focus box (Esc to finish)", default: "ctrl+alt+5" },
    Action { id: "smash", label: "Smash the glass here", default: "ctrl+alt+x" },
    Action { id: "clear", label: "Clear ink, cracks and focus", default: "ctrl+alt+z" },
    Action { id: "bigger", label: "Bigger pointer", default: "ctrl+alt+bracketright" },
    Action { id: "smaller", label: "Smaller pointer", default: "ctrl+alt+bracketleft" },
    Action { id: "cursor", label: "Show or hide the system cursor", default: "ctrl+alt+c" },
    Action { id: "sound", label: "Sound on or off", default: "ctrl+alt+m" },
    Action { id: "perf", label: "Frame-rate readout", default: "ctrl+alt+0" },
    Action { id: "help", label: "Hotkey cheat sheet", default: "ctrl+alt+slash" },
    Action { id: "settings", label: "Settings", default: "ctrl+alt+comma" },
    Action { id: "quit", label: "Quit Pointer", default: "ctrl+alt+q" },
];

/// The chord an action is bound to: the user's, else its default. `None` when unbound.
pub fn chord(overrides: &BTreeMap<String, String>, id: &str) -> Option<Shortcut> {
    let text = match overrides.get(id) {
        Some(s) => s.as_str(),
        None => ACTIONS.iter().find(|a| a.id == id)?.default,
    };
    if text.is_empty() { None } else { text.parse().ok() }
}

/// The chord as keycaps ("⌃⌥P"), or an em dash when unbound.
pub fn keys(overrides: &BTreeMap<String, String>, id: &str) -> String {
    chord(overrides, id).map_or_else(|| "—".to_string(), |s| s.keycaps().concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_parses_and_none_repeat() {
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            let s: Shortcut = a.default.parse().unwrap_or_else(|e| panic!("{}: {e}", a.id));
            assert!(seen.insert(s.to_string()), "{} repeats a chord", a.id);
        }
    }

    #[test]
    fn overrides_win_and_empty_unbinds() {
        let mut o = BTreeMap::new();
        assert!(chord(&o, "toggle").is_some());
        o.insert("toggle".into(), "ctrl+alt+k".into());
        assert_eq!(chord(&o, "toggle").unwrap().to_string(), "ctrl+alt+k");
        o.insert("toggle".into(), String::new());
        assert!(chord(&o, "toggle").is_none());
        assert_eq!(keys(&o, "toggle"), "—");
    }
}
