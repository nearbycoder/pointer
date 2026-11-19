//! The overlay's view: two effect slots the compositor paints (the glass beneath, the pointer
//! above) around a small HUD drawn with Rune: a toast when a tool changes, the hotkey card,
//! and a frame-rate readout. In headless runs it also draws a page behind everything, so a
//! still shows the pointer in context.

use rune::{Color, Cx, El, TextStyle, Transition, col, effect, el, row, text};

use crate::math::v2;
use crate::bindings::{self, ACTIONS};
use crate::settings::Tool;
use crate::stage::{Backdrop, MARK_OVER, MARK_UNDER, Source, Stage};

/// Rune's colours are linear light; these are written the way designers write them, in sRGB.
pub(crate) fn srgb(r: f32, g: f32, b: f32, a: f32) -> Color {
    use rune::color::srgb_decode;
    Color { r: srgb_decode(r), g: srgb_decode(g), b: srgb_decode(b), a }
}

pub(crate) fn card() -> Color {
    srgb(0.075, 0.075, 0.09, 0.93)
}

pub(crate) fn white(a: f32) -> Color {
    Color { r: 1.0, g: 1.0, b: 1.0, a }
}

pub(crate) fn tool_color(tool: Tool) -> Color {
    match tool {
        Tool::Tap => srgb(0.62, 0.85, 0.3, 1.0),
        Tool::Spotlight => srgb(1.0, 0.82, 0.3, 1.0),
        Tool::Laser => srgb(1.0, 0.25, 0.3, 1.0),
        Tool::Pen => srgb(1.0, 0.3, 0.55, 1.0),
        Tool::Focus => srgb(0.45, 0.7, 1.0, 1.0),
    }
}

pub(crate) fn keycap(keys: &str, k: f32) -> El {
    el().px(8.0 * k).py(3.0 * k).radius(7.0 * k).fill(white(0.09)).border(1.0, white(0.16))
        .child(text(keys).font(TextStyle::new(13.0 * k, 600.0)).color(white(0.9)))
}

/// A column covering the whole display.
pub(crate) fn full() -> El {
    col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
}

pub fn view(s: &Stage, _: &mut Cx<Stage>) -> El {
    let k = s.hud_scale();
    let mut root = el().w_full().h_full();
    match s.backdrop {
        Backdrop::None => {}
        Backdrop::Icon(at) => root = root.child(icon_field(at)),
        Backdrop::Card(at) => root = root.child(icon_field(at)).child(card_words()),
        other => root = root.child(backdrop(other)),
    }
    let promo = match s.source { Source::Promo(shot) => Some(shot), _ => None };
    if let Some(shot) = promo && let Some(back) = crate::promo::backdrop(shot, s.time() as f32) { root = root.child(back); }
    // The effect slots are keyed: the toast, the hotkey card and the readout come and go
    // between them, and an unkeyed slot whose place among its siblings changes reads as a new
    // element that fades in. A fading slot is drawn inside an offscreen layer, and the glove
    // would blink out while it does (or stay out, if the overlay came to rest meanwhile).
    root = root.child(effect(MARK_UNDER).key("fx-under").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)));
    if let Some(shot) = promo { root = root.child(crate::promo::overlay(shot, s.time() as f32)); }

    if let Some((toast, _)) = s.toast.as_ref().filter(|_| promo.is_none()) {
        let accent = match toast.accent {
            Some([r, g, b]) => srgb(r, g, b, 1.0),
            None if toast.title == "Hidden" => white(0.5),
            None => tool_color(s.st.tool),
        };
        let pill = row().key("toast").items_center().gap(14.0 * k).px(20.0 * k).py(13.0 * k).radius(30.0 * k)
            .fill(card()).border(1.0, white(0.1)).shadow(10.0 * k, 34.0 * k, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 })
            .child(el().size(10.0 * k, 10.0 * k).radius(5.0 * k).fill(accent))
            .child(col().gap(2.0 * k)
                .child(text(toast.title.clone()).font(TextStyle::new(17.0 * k, 650.0)).color(white(0.97)))
                .child(text(toast.detail.clone()).font(TextStyle::new(13.0 * k, 450.0)).color(white(0.6))))
            .child(keycap(&toast.keys, k))
            .transition(Transition::offset(0.0, 18.0 * k).combined(Transition::opacity()));
        root = root.child(full().key("toast-wrap").justify_end().items_center().pb(72.0 * k).child(pill));
    }

    if s.help {
        let mut list = col().gap(9.0 * k);
        for a in ACTIONS {
            list = list.child(row().items_center().justify_between().gap(24.0 * k)
                .child(text(a.label).font(TextStyle::new(14.5 * k, 450.0)).color(white(0.82)))
                .child(keycap(&bindings::keys(&s.st.bindings, a.id), k)));
        }
        let card = col().key("help").w(520.0 * k).padding(28.0 * k).gap(18.0 * k).radius(24.0 * k)
            .fill(card()).border(1.0, white(0.1)).shadow(16.0 * k, 50.0 * k, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.4 })
            .child(col().gap(4.0 * k)
                .child(text("Pointer").font(TextStyle::new(22.0 * k, 700.0)).color(white(0.98)))
                .child(text(format!("These keys work from any app. Change them in Settings ({}).", bindings::keys(&s.st.bindings, "settings"))).font(TextStyle::new(13.5 * k, 450.0)).color(white(0.55))))
            .child(list)
            .transition(Transition::scale(0.96).combined(Transition::opacity()));
        root = root.child(full().key("help-wrap").center().child(card));
    }

    if s.perf_on {
        let readout = row().key("perf").px(12.0 * k).py(7.0 * k).radius(12.0 * k).fill(card()).border(1.0, white(0.1))
            .child(text("000 fps · 00.00 ms").live_text(&s.perf_text).font(TextStyle::new(13.0 * k, 600.0).features("'tnum' 1")).color(white(0.9)))
            .transition(Transition::opacity());
        root = root.child(full().key("perf-wrap").items_end().pt(18.0 * k).pr(18.0 * k).child(readout));
    }

    root = root.child(effect(MARK_OVER).key("fx-over").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)));
    root.on_pointer_move(|s: &mut Stage, p| s.pointer_moved(v2(p.x, p.y)))
        .on_drag(|s: &mut Stage, d, _tx| {
            let down = !matches!(d.phase, rune::Phase::Up) && !d.cancelled();
            s.pointer_button(v2(d.x, d.y), down);
        })
}

/// The app icon's field: a violet gradient, a glow behind the fingertip and two ripples. The
/// art script cuts the canvas to the macOS icon shape afterwards.
fn icon_field(at: crate::math::V2) -> El {
    let ring = |r: f32, a: f32| el().size(r * 2.0, r * 2.0).radius(r).border(10.0, white(a)).pin(Some(at.x - r), Some(at.y - r), None, None);
    full().key("icon-field")
        .linear_gradient(2.4, vec![(0.0, srgb(0.56, 0.44, 1.0, 1.0)), (0.55, srgb(0.36, 0.22, 0.93, 1.0)), (1.0, srgb(0.16, 0.08, 0.5, 1.0))])
        .child(el().size(760.0, 760.0).pin(Some(at.x - 380.0), Some(at.y - 380.0), None, None)
            .radial_gradient([0.5, 0.5], 380.0, vec![(0.0, white(0.42)), (0.45, white(0.12)), (1.0, white(0.0))]))
        .child(ring(118.0, 0.30))
        .child(ring(196.0, 0.16))
}

/// The social card's words, left of the pointer.
fn card_words() -> El {
    col().key("card-words").pin(Some(80.0), Some(0.0), None, Some(0.0)).justify_center().items_start().gap(10.0).pb(20.0)
        .child(text("Pointer").font(TextStyle::new(118.0, 800.0)).color(white(1.0)))
        .child(text("A giant pointer for your screen.").font(TextStyle::new(38.0, 600.0)).color(white(0.94)))
        .child(text("Free for macOS · Apple silicon and Intel").font(TextStyle::new(23.0, 500.0)).color(white(0.72)).pt(14.0))
}

/// A plain slide-like page for headless stills and videos.
fn backdrop(kind: Backdrop) -> El {
    let dark = kind == Backdrop::Dark;
    let (bg, ink, soft, card) = if dark {
        (srgb(0.09, 0.1, 0.12, 1.0), white(0.92), white(0.5), srgb(0.15, 0.16, 0.19, 1.0))
    } else {
        (srgb(0.975, 0.975, 0.98, 1.0), srgb(0.07, 0.07, 0.09, 1.0), srgb(0.07, 0.07, 0.09, 0.5), Color::WHITE)
    };
    let chip = |label: &'static str| el().px(14.0).py(8.0).radius(10.0).fill(card).border(1.0, soft.alpha(0.18))
        .child(text(label).font(TextStyle::new(15.0, 500.0)).color(ink));
    let tile = |c: Color| el().size(250.0, 150.0).radius(16.0).fill(c);
    full().key("backdrop").fill(bg).items_center().pt(170.0).gap(28.0)
        .child(text("Quarterly product update").font(TextStyle::new(54.0, 700.0)).color(ink))
        .child(text("Three things shipped, one thing slipped, and what we do next.").font(TextStyle::new(22.0, 450.0)).color(soft))
        .child(el().w(760.0).h(110.0).radius(20.0).fill(card).border(1.0, soft.alpha(0.15)).shadow(6.0, 24.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.08 })
            .padding(22.0).child(text("Ask anything about the launch…").font(TextStyle::new(18.0, 450.0)).color(soft)))
        .child(row().gap(12.0).child(chip("Revenue")).child(chip("Retention")).child(chip("Roadmap")).child(chip("Hiring")))
        .child(row().gap(18.0).pt(30.0)
            .child(tile(srgb(0.28, 0.55, 0.95, 1.0)))
            .child(tile(srgb(0.98, 0.62, 0.25, 1.0)))
            .child(tile(srgb(0.36, 0.78, 0.48, 1.0)))
            .child(tile(srgb(0.62, 0.42, 0.9, 1.0))))
}
