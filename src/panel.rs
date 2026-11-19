//! The settings window: a sidebar of pages (General, Pointer, Shortcuts, Updates, About)
//! drawn with Rune's theme tokens in the manner of macOS System Settings. Shortcuts use
//! rune-kit's recorder; everything else is plain Rune elements.

use rune::tok::{self, motion};
use rune::{Align, Color, Cx, Drag, El, Icon, Phase, ScrollSpec, Shortcut, ShortcutError, TextStyle, Tx, col, el, icon, row, text};
use rune_kit::{Kit, ShortcutRecorder, ShortcutRecorderState};

use crate::actions;
use crate::bindings::{self, ACTIONS};
use crate::ctl::{self, Ctl, Page};
use crate::input;
use crate::settings::{Settings, Tool};
use crate::style::{BLADES, Style};
use crate::update::{self, Status};

pub const SITE: &str = "https://nearbycoder.github.io/pointer/";
pub const REPO_URL: &str = "https://github.com/nearbycoder/pointer";
pub const ISSUES: &str = "https://github.com/nearbycoder/pointer/issues";

#[derive(Clone, Copy)]
struct Paint {
    t: tok::Theme,
    accent: Color,
    hair: f32,
    dark: bool,
}

fn srgb(r: f32, g: f32, b: f32, a: f32) -> Color {
    use rune::color::srgb_decode;
    Color { r: srgb_decode(r), g: srgb_decode(g), b: srgb_decode(b), a }
}

fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

static GENERAL: Icon = Icon { name: "pointer-general", view: 24.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/></g>"# };
static POINTER: Icon = Icon { name: "pointer-pointer", view: 24.0, body: r#"<path d="M6 3.5l12.5 6.2-5.6 1.9-1.9 5.6z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/><path d="M13.2 12.6l5.3 5.3" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>"# };
static KEYS: Icon = Icon { name: "pointer-keys", view: 24.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><rect x="3" y="6" width="18" height="12" rx="2.5"/><path d="M7 10h.01M10.5 10h.01M14 10h.01M17.5 10h.01M8 14h8"/></g>"# };
static UPDATES: Icon = Icon { name: "pointer-updates", view: 24.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M12 7.5v8M8.5 12l3.5 3.5 3.5-3.5"/></g>"# };
static ABOUT: Icon = Icon { name: "pointer-about", view: 24.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.6v.01"/></g>"# };

fn page_icon(page: Page) -> &'static Icon {
    match page {
        Page::General => &GENERAL,
        Page::Pointer => &POINTER,
        Page::Shortcuts => &KEYS,
        Page::Updates => &UPDATES,
        Page::About => &ABOUT,
    }
}

pub fn view(c: &Ctl, cx: &mut Cx<Ctl>) -> El {
    let dark = cx.system_dark();
    let p = Paint {
        t: tok::theme(dark),
        accent: if dark { srgb(0.60, 0.52, 1.0, 1.0) } else { srgb(0.43, 0.33, 0.96, 1.0) },
        hair: 1.0 / cx.env().scale,
        dark,
    };
    let kit = Kit::neutral(dark);
    row().key("settings").w_full().h_full().fill(p.t.bg)
        .child(sidebar(c, p))
        .child(content(c, p, &kit))
}

fn sidebar(c: &Ctl, p: Paint) -> El {
    let bg = if p.dark { srgb(0.105, 0.105, 0.125, 1.0) } else { srgb(0.948, 0.948, 0.962, 1.0) };
    let mut nav = col().key("nav").gap(2.0);
    for page in Page::ALL {
        let on = c.page == page;
        let ink = if on { p.accent } else { p.t.text };
        nav = nav.child(
            row().key(page.name()).role("tab").label(page.name()).checked(on).focusable(true)
                .h(32.0).px(10.0).gap(10.0).center_y().radius(8.0)
                .fill(if on { p.accent.alpha(if p.dark { 0.22 } else { 0.13 }) } else { Color::TRANSPARENT })
                .hover(if on { Color::TRANSPARENT } else { p.t.hover })
                .press(p.t.press)
                .motion(motion::SNAPPY)
                .on_press(move |c: &mut Ctl| c.page = page)
                .child(icon(page_icon(page), 16.0).color(if on { p.accent } else { p.t.text_secondary }))
                .child(text(page.name()).font(font(13.5, if on { 600.0 } else { 500.0 })).color(ink).a11y_hidden()),
        );
    }
    let mut brand = row().key("brand").gap(10.0).center_y().px(6.0);
    if let Some(images) = &c.images {
        brand = brand.child(el().key("brand-icon").size(30.0, 30.0).pixels(images.icon.pixels()));
    }
    brand = brand.child(col().gap(1.0)
        .child(text("Pointer").font(font(15.0, 700.0)).color(p.t.text))
        .child(text(format!("Version {}", update::VERSION)).font(font(11.5, 450.0)).color(p.t.text_secondary)));
    col().key("sidebar").w(214.0).h_full().fill(bg)
        .child(el().key("sidebar-drag").w_full().h(48.0).window_drag())
        .child(col().key("sidebar-body").w_full().grow(1.0).px(12.0).gap(18.0).pb(16.0)
            .child(brand)
            .child(nav)
            .child(el().grow(1.0))
            .child(text("Made with Rune").font(font(11.5, 450.0)).color(p.t.text_secondary).px(8.0)))
}

fn content(c: &Ctl, p: Paint, kit: &Kit) -> El {
    let body = match c.page {
        Page::General => general(c, p),
        Page::Pointer => pointer_page(c, p),
        Page::Shortcuts => shortcuts(c, p, kit),
        Page::Updates => updates(c, p),
        Page::About => about(c, p),
    };
    col().key("content").grow(1.0).h_full().min_w(0.0)
        .child(row().key("header").w_full().h(56.0).px(28.0).pt(12.0).center_y().window_drag()
            .child(text(c.page.name()).font(font(21.0, 700.0)).color(p.t.text)))
        .child(col().key("scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false })
            .child(col().key(format!("page-{}", c.page.name())).w_full().px(28.0).pt(4.0).pb(28.0).gap(22.0).child(body)))
}

// ---------------------------------------------------------------------------------- pieces

fn section(p: Paint, key: &str, title: &str, rows: Vec<El>) -> El {
    let n = rows.len();
    let mut card = col().key("card").padding(4.0).fill(p.t.card).radius(12.0)
        .border(p.hair, p.t.card_border).shadow(1.0, 2.0, p.t.shadow).shadow(8.0, 24.0, p.t.shadow_soft);
    for (i, r) in rows.into_iter().enumerate() {
        card = card.child(r);
        if i + 1 < n {
            card = card.child(el().key(format!("sep{i}")).h(p.hair).ml(12.0).mr(12.0).fill(p.t.separator).a11y_hidden());
        }
    }
    col().key(key).gap(8.0)
        .child(text(title).key("title").font(font(12.5, 600.0)).color(p.t.text_secondary).ml(12.0))
        .child(card)
}

fn setting(p: Paint, key: &str, label: &str, detail: Option<&str>, control: El) -> El {
    let mut words = col().key("words").gap(2.0).shrink().min_w(0.0)
        .child(text(label).key("label").font(font(13.5, 500.0)).color(p.t.text).wrap().w_full());
    if let Some(d) = detail {
        words = words.child(text(d).key("detail").font(font(12.0, 450.0)).color(p.t.text_secondary).wrap().w_full());
    }
    row().key(key).min_h(48.0).px(12.0).py(8.0).gap(16.0).center_y().justify_between().child(words).child(control)
}

fn toggle(p: Paint, on: bool) -> El {
    let (w, h, k) = (38.0, 22.0, 18.0);
    let inset = (h - k) / 2.0;
    el().key("switch").size(w, h).radius(h / 2.0)
        .fill(if on { p.accent } else { p.t.control_off })
        .motion(motion::SNAPPY)
        .child(el().key("knob").size(k, k).absolute(if on { w - k - inset } else { inset }, inset).radius(k / 2.0)
            .fill(p.t.knob).shadow(1.0, 3.0, p.t.knob_shadow).shadow(0.0, 0.5, p.t.edge))
}

fn toggle_row(p: Paint, key: &str, label: &str, detail: &str, on: bool, flip: fn(&mut Ctl)) -> El {
    setting(p, key, label, Some(detail), toggle(p, on).a11y_hidden())
        .role("switch").label(label).checked(on).focusable(true)
        .radius(8.0).hover(p.t.hover).press(p.t.press).on_press(flip)
}

fn button(p: Paint, key: &str, label: &str, primary: bool, enabled: bool) -> El {
    let (fill, ink) = if primary { (p.accent, Color::WHITE) } else { (p.t.control_track, p.t.text) };
    let b = row().key(key).role("button").label(label).focusable(enabled).h(30.0).px(14.0).center().radius(8.0)
        .fill(fill).border(p.hair, if primary { Color::TRANSPARENT } else { p.t.card_border })
        .child(text(label).font(font(13.0, 600.0)).color(ink).a11y_hidden());
    if enabled { b.hover(Color::WHITE.alpha(0.08)).press(Color::BLACK.alpha(0.08)) } else { b.opacity(0.45) }
}

/// Changes a setting from the window and keeps the window's copy current.
fn change(c: &mut Ctl, f: impl FnOnce(&mut Settings)) {
    c.shared.update(f);
    c.st = c.shared.get();
}

// ---------------------------------------------------------------------------------- pages

fn general(c: &Ctl, p: Paint) -> El {
    let startup = match c.login {
        Some(on) => toggle_row(p, "login", "Open at login", "Start Pointer when you log in", on, |c| {
            let on = !c.login.unwrap_or(false);
            c.set_login(on);
        }),
        None => setting(p, "login", "Open at login", Some("Available in the installed app"), toggle(p, false).opacity(0.4)),
    };
    let mut startup_rows = vec![startup];
    if let Some(e) = &c.login_error {
        startup_rows.push(setting(p, "login-error", "Couldn't change it", Some(e), el()));
    }
    let feedback = vec![
        toggle_row(p, "sound", "Sound effects", "Pops, cracks, hums and squawks", c.st.sound, |c| change(c, |s| s.sound = !s.sound)),
        toggle_row(p, "cursor", "Hide the system cursor", "The pointer's tip becomes your cursor", c.st.hide_cursor, |c| {
            change(c, |s| s.hide_cursor = !s.hide_cursor);
            if !c.st.hide_cursor { input::set_cursor_hidden(false); }
        }),
        toggle_row(p, "perf", "Frame-rate readout", "Frames per second, in the corner of the screen", c.st.perf, |c| change(c, |s| s.perf = !s.perf)),
    ];
    let tip = format!("Press {} in any app to see every hotkey, and {} to bring back this window.",
        bindings::keys(&c.st.bindings, "help"), bindings::keys(&c.st.bindings, "settings"));
    col().key("general").gap(22.0)
        .child(section(p, "startup", "Startup", startup_rows))
        .child(section(p, "feedback", "Feedback", feedback))
        .child(text(tip).key("tip").font(font(12.5, 450.0)).color(p.t.text_secondary).px(12.0).wrap().w_full())
}

fn pointer_page(c: &Ctl, p: Paint) -> El {
    let mut tiles = row().key("styles").gap(10.0).flex_wrap();
    for (i, style) in Style::ALL.into_iter().enumerate() {
        let on = c.st.style == style;
        let mut tile = col().key(style.id()).role("radio").label(style.name()).checked(on).focusable(true)
            .w(100.0).h(124.0).radius(12.0).padding(8.0).gap(4.0).items_center()
            .fill(if on { p.accent.alpha(if p.dark { 0.2 } else { 0.1 }) } else { p.t.card })
            .border(if on { 2.0 } else { p.hair }, if on { p.accent } else { p.t.card_border })
            .hover(p.t.hover).press(p.t.press).motion(motion::SNAPPY)
            .on_press(move |c: &mut Ctl| {
                actions::set_style_on(&c.shared, style);
                c.st = c.shared.get();
            });
        if let Some(images) = &c.images {
            tile = tile.child(el().key("thumb").size(84.0, 84.0).pixels(images.thumbs[i].pixels()));
        }
        let short = match style {
            Style::Glove => "Glove",
            Style::Saber => "Laser sword",
            Style::Wand => "Magic wand",
            Style::Mallet => "Mallet",
            Style::Chicken => "Chicken",
        };
        tiles = tiles.child(tile.child(text(short).font(font(12.0, 600.0)).color(if on { p.accent } else { p.t.text }).a11y_hidden()));
    }
    let look = vec![
        setting(p, "size", "Size", Some("Or use the bigger and smaller hotkeys"), size_slider(p, c.st.scale)),
        setting(p, "blade", "Laser sword colour", Some("The sword also takes the next colour each time you pick it"), swatches(p, c.st.blade)),
    ];
    let tool = vec![setting(p, "tool", "Tool", Some("What clicks and drags do"), tools(p, c.st.tool))];
    col().key("pointer").gap(22.0)
        .child(col().gap(8.0).child(text("Pointer").font(font(12.5, 600.0)).color(p.t.text_secondary).ml(12.0)).child(tiles))
        .child(section(p, "look", "Look", look))
        .child(section(p, "tools", "Tool", tool))
}

/// The pointer's size, 40% to 300% on a log scale.
fn size_slider(p: Paint, scale: f32) -> El {
    let (w, h, k, bar) = (190.0, 22.0, 18.0, 4.0);
    let travel = w - k;
    let (lo, hi) = (0.4f32, 3.0f32);
    let v = ((scale.clamp(lo, hi) / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0);
    let to_scale = move |x: f32| lo * (hi / lo).powf(((x - k / 2.0) / travel).clamp(0.0, 1.0));
    let set = |c: &mut Ctl, scale: f32| change(c, |s| s.scale = (scale * 100.0).round() / 100.0);
    row().key("size-control").gap(12.0).center_y()
        .child(el().key("slider").role("slider").label("Size")
            .range_value(f64::from(scale), f64::from(lo), f64::from(hi))
            .on_step(move |c: &mut Ctl, steps, _| { let s = c.st.scale * 1.1f32.powi(steps); set(c, s.clamp(lo, hi)); })
            .size(w, h).motion(motion::SNAPPY)
            .on_drag(move |c: &mut Ctl, d: &mut Drag, tx| match d.phase {
                Phase::Down => { d.data = c.st.scale; set(c, to_scale(d.x)); }
                Phase::Move => { set(c, to_scale(d.x)); tx.instant(); }
                Phase::Up => { if d.cancelled() { let back = d.data; set(c, back); } }
            })
            .child(el().key("track").size(w, bar).absolute(0.0, (h - bar) / 2.0).radius(bar / 2.0).fill(p.t.control_off).a11y_hidden())
            .child(el().key("fill").size(k / 2.0 + v * travel, bar).absolute(0.0, (h - bar) / 2.0).radius(bar / 2.0).fill(p.accent).a11y_hidden())
            .child(el().key("knob").size(k, k).absolute(v * travel, (h - k) / 2.0).radius(k / 2.0)
                .fill(p.t.knob).shadow(1.0, 3.0, p.t.knob_shadow).shadow(0.0, 0.5, p.t.edge).a11y_hidden()))
        .child(text(format!("{:.0}%", scale * 100.0)).key("value").w(44.0).font(font(12.5, 600.0).features("'tnum' 1")).color(p.t.text_secondary).align(Align::End))
}

fn swatches(p: Paint, current: usize) -> El {
    let d = 20.0;
    let mut r = row().key("swatches").role("radiogroup").label("Laser sword colour").gap(10.0);
    for (i, [cr, cg, cb]) in BLADES.into_iter().enumerate() {
        let on = i == current % BLADES.len();
        r = r.child(el().key(format!("blade{i}")).role("radio").label(["Blue", "Green", "Red", "Purple", "Gold"][i]).checked(on).focusable(true)
            .size(d, d).radius(d / 2.0).fill(srgb(cr, cg, cb, 1.0))
            .border(if on { 2.5 } else { p.hair }, if on { p.t.text } else { Color::BLACK.alpha(0.12) })
            .on_press(move |c: &mut Ctl| change(c, |s| s.blade = i)));
    }
    r
}

fn tools(p: Paint, current: Tool) -> El {
    let names = ["Pointer", "Spotlight", "Laser", "Pen", "Focus"];
    let (seg, h, pad) = (74.0, 28.0, 2.0);
    let idx = Tool::ALL.iter().position(|t| *t == current).unwrap_or(0);
    let mut segs = row().key("segments");
    for (i, tool) in Tool::ALL.into_iter().enumerate() {
        let on = i == idx;
        segs = segs.child(el().key(names[i]).role("segment").label(names[i]).checked(on).focusable(true)
            .size(seg, h - 2.0 * pad).center().radius(6.0)
            .hover(if on { Color::TRANSPARENT } else { p.t.hover })
            .on_press(move |c: &mut Ctl| ctl::pick_tool(c, tool))
            .child(text(names[i]).font(font(12.5, if on { 600.0 } else { 500.0 })).color(if on { p.t.text } else { p.t.text_secondary }).a11y_hidden()));
    }
    el().key("segmented").role("segmented").size(seg * 5.0 + 2.0 * pad, h).padding(pad).radius(8.0).fill(p.t.control_track).motion(motion::SNAPPY)
        .child(el().key("indicator").size(seg, h - 2.0 * pad).absolute(pad + idx as f32 * seg, pad).radius(6.0)
            .fill(p.t.indicator).shadow(1.0, 3.0, p.t.shadow).shadow(0.0, 0.5, p.t.edge).lead(motion::LEAD, motion::DEFAULT).a11y_hidden())
        .child(segs)
}

/// Each recorder's state in the controller, for rune-kit.
fn recorder_state(i: usize) -> impl for<'a> Fn(&'a mut Ctl) -> &'a mut ShortcutRecorderState + 'static {
    move |c: &mut Ctl| &mut c.recorders[i]
}

fn shortcuts(c: &Ctl, p: Paint, kit: &Kit) -> El {
    let mut rows = Vec::with_capacity(ACTIONS.len());
    for (i, a) in ACTIONS.iter().enumerate() {
        let recorder = ShortcutRecorder::new(format!("rec-{}", a.id), &c.recorders[i], recorder_state(i))
            .label(a.label)
            .width(150.0)
            .on_change(move |c: &mut Ctl, chord: Option<&Shortcut>, _: &mut Tx| -> Result<(), ShortcutError> { c.rebind(i, chord) })
            .build(kit);
        rows.push(setting(p, a.id, a.label, None, recorder));
    }
    col().key("shortcuts").gap(16.0)
        .child(text("Click a shortcut, then press the keys you want. Backspace clears it and Esc cancels. Hotkeys work in every app, so pick ones other apps don't use.")
            .key("how").font(font(12.5, 450.0)).color(p.t.text_secondary).px(12.0).wrap().w_full())
        .child(section(p, "keys", "Hotkeys", rows))
        .child(row().key("reset-row").justify_end().child(button(p, "reset", "Restore defaults", false, true).on_press(|c: &mut Ctl| c.reset_shortcuts())))
}

fn updates(c: &Ctl, p: Paint) -> El {
    let bundled = c.installed;
    let (status, action): (String, Option<El>) = match &c.update {
        Status::Unchecked => ("Not checked yet.".into(), Some(button(p, "check", "Check now", false, true).on_press(|c: &mut Ctl| c.check_updates(true)))),
        Status::Checking => ("Checking GitHub…".into(), Some(button(p, "check", "Checking…", false, false))),
        Status::Current => ("You're up to date.".into(), Some(button(p, "check", "Check again", false, true).on_press(|c: &mut Ctl| c.check_updates(true)))),
        Status::Available(r) => (
            format!("Pointer {} is available.", r.version),
            Some(if bundled {
                button(p, "install", "Install and relaunch", true, true).on_press(|c: &mut Ctl| c.install_update())
            } else {
                let page = r.page.clone();
                button(p, "download", "Download", true, true).on_press_tx(move |_: &mut Ctl, tx| { let _ = tx.open_url(&page); })
            }),
        ),
        Status::Installing(r, step) => (format!("{step} Pointer {}…", r.version), Some(button(p, "install", "Installing…", true, false))),
        Status::Failed(e) => (format!("Couldn't update: {e}"), Some(button(p, "check", "Try again", false, true).on_press(|c: &mut Ctl| c.check_updates(true)))),
    };
    let mut head = row().key("status").gap(14.0).center_y().px(12.0).py(12.0);
    if let Some(images) = &c.images {
        head = head.child(el().key("icon").size(44.0, 44.0).pixels(images.icon.pixels()));
    }
    head = head.child(col().gap(3.0).grow(1.0).min_w(0.0)
        .child(text(format!("Pointer {}", update::VERSION)).font(font(15.0, 650.0)).color(p.t.text))
        .child(text(status).font(font(12.5, 450.0)).color(p.t.text_secondary).wrap().w_full()));
    if let Some(a) = action { head = head.child(a); }
    let mut page = col().key("updates").gap(22.0)
        .child(section(p, "version", "This Mac", vec![head]))
        .child(section(p, "auto", "Automatic updates", vec![
            toggle_row(p, "auto-update", "Check for updates automatically", "At launch and every six hours, from GitHub", c.st.auto_update, |c| change(c, |s| s.auto_update = !s.auto_update)),
        ]));
    if let Status::Available(r) | Status::Installing(r, _) = &c.update && !r.notes.is_empty() {
        let page_url = r.page.clone();
        page = page.child(section(p, "notes", "What's new", vec![
            col().key("notes-body").px(12.0).py(12.0).gap(10.0)
                .child(text(r.notes.clone()).font(font(12.5, 450.0)).color(p.t.text).wrap().w_full().max_lines(16))
                .child(row().child(button(p, "notes-link", "Read on GitHub", false, true).on_press_tx(move |_: &mut Ctl, tx| { let _ = tx.open_url(&page_url); }))),
        ]));
    }
    if !bundled {
        page = page.child(text("This is a development build, so updates won't install here. The app from the website updates itself.")
            .font(font(12.0, 450.0)).color(p.t.text_secondary).px(12.0).wrap().w_full());
    }
    page.child(text("Updates come from the releases at github.com/nearbycoder/pointer. Pointer installs one only after checking it's signed by the same developer.")
        .font(font(12.0, 450.0)).color(p.t.text_secondary).px(12.0).wrap().w_full())
}

fn about(c: &Ctl, p: Paint) -> El {
    let link = |key: &'static str, label: &'static str, url: &'static str| button(p, key, label, false, true).on_press_tx(move |_: &mut Ctl, tx| { let _ = tx.open_url(url); });
    let mut page = col().key("about").w_full().items_center().gap(10.0).pt(18.0);
    if let Some(images) = &c.images {
        page = page.child(el().key("icon").size(112.0, 112.0).pixels(images.icon.pixels()));
    }
    page.child(text("Pointer").font(font(28.0, 750.0)).color(p.t.text))
        .child(text(format!("Version {}", update::VERSION)).font(font(13.0, 500.0)).color(p.t.text_secondary))
        .child(text("A giant pointer for your screen, with tools for showing people where to look.")
            .font(font(14.0, 450.0)).color(p.t.text).align(Align::Center).wrap().w_full().max_w(420.0).pt(6.0))
        .child(row().key("links").gap(10.0).pt(12.0)
            .child(link("site", "Website", SITE))
            .child(link("source", "Source code", REPO_URL))
            .child(link("issues", "Report a problem", ISSUES)))
        .child(text("Built with Rune. Every picture and sound is made live, as you point.")
            .font(font(12.0, 450.0)).color(p.t.text_secondary).align(Align::Center).wrap().w_full().max_w(420.0).pt(18.0))
        .child(text("© 2026 nearbycoder · MIT License").font(font(12.0, 450.0)).color(p.t.text_secondary))
}
