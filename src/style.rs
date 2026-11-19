//! The pointers to choose from (⌃⌥S cycles them). Each has its own shape, click and sounds;
//! the tools (spotlight, laser, pen, focus) and the cracking glass work with all of them.

use crate::math::{V2, v2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// A cartoon glove on a telescoping pole.
    Glove,
    /// A glowing energy blade on a metal hilt: it ignites, hums, trails its swings and
    /// throws sparks when you click.
    Saber,
    /// A magician's wand that sheds sparkles and bursts into stars.
    Wand,
    /// A cartoon mallet: every click is a bonk.
    Mallet,
    /// A rubber chicken on the pole. It squawks.
    Chicken,
}

/// How a style hangs off the rig: drawn on the telescoping pole, or a rigid prop of its own.
pub enum Mount {
    /// On the pole, which runs into the model at `attach` (model units).
    Pole { attach: V2 },
    /// Rigid, `length` glove-sizes long from the tip down its axis.
    Prop { length: f32 },
}

/// Blade colours for the energy sword: each time it's picked it takes the next.
pub const BLADES: [[f32; 3]; 5] = [
    [0.25, 0.6, 1.0],
    [0.3, 1.0, 0.35],
    [1.0, 0.16, 0.12],
    [0.78, 0.35, 1.0],
    [1.0, 0.78, 0.2],
];

/// The saber's blade and hilt, in glove sizes.
pub const BLADE: f32 = 3.6;
pub const HILT: f32 = 1.05;
/// The wand's length.
pub const WAND: f32 = 2.5;
/// How long the mallet's handle hangs from its head.
pub const MALLET_HANDLE: f32 = 2.3;
/// The chicken is modelled at the glove's size, and drawn this much bigger.
pub const CHICKEN: f32 = 1.3;

impl Style {
    pub const ALL: [Style; 5] = [Style::Glove, Style::Saber, Style::Wand, Style::Mallet, Style::Chicken];

    pub fn name(self) -> &'static str {
        match self {
            Style::Glove => "Glove",
            Style::Saber => "Laser sword",
            Style::Wand => "Magic wand",
            Style::Mallet => "Mallet",
            Style::Chicken => "Rubber chicken",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Style::Glove => "The classic · ⌃⌥S for the next pointer",
            Style::Saber => "Swing it · click to clash · ⌃⌥S for the next",
            Style::Wand => "Sparkles as it moves · click to cast · ⌃⌥S for the next",
            Style::Mallet => "Every click is a bonk · ⌃⌥S for the next",
            Style::Chicken => "Click to squawk · ⌃⌥S for the next",
        }
    }
    pub fn accent(self) -> [f32; 3] {
        match self {
            Style::Glove => [0.95, 0.95, 0.95],
            Style::Saber => [0.35, 0.65, 1.0],
            Style::Wand => [1.0, 0.85, 0.35],
            Style::Mallet => [0.85, 0.6, 0.35],
            Style::Chicken => [1.0, 0.85, 0.2],
        }
    }
    pub fn next(self) -> Style {
        let i = Style::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Style::ALL[(i + 1) % Style::ALL.len()]
    }
    pub fn id(self) -> &'static str {
        match self {
            Style::Glove => "glove",
            Style::Saber => "saber",
            Style::Wand => "wand",
            Style::Mallet => "mallet",
            Style::Chicken => "chicken",
        }
    }
    pub fn from_id(id: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.id() == id)
    }
    pub fn mount(self) -> Mount {
        match self {
            Style::Glove => Mount::Pole { attach: v2(0.08, 1.0) },
            Style::Chicken => Mount::Pole { attach: v2(0.02 * CHICKEN, 1.08 * CHICKEN) },
            Style::Saber => Mount::Prop { length: BLADE + HILT },
            Style::Wand => Mount::Prop { length: WAND },
            Style::Mallet => Mount::Prop { length: MALLET_HANDLE + 0.3 },
        }
    }
    /// Where the rig's pole would meet this style (model units), for the pose.
    pub fn attach(self) -> V2 {
        match self.mount() {
            Mount::Pole { attach } => attach,
            Mount::Prop { length } => v2(0.0, length),
        }
    }
}
