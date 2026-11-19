//! Pointer: a giant glove on a telescoping pole that follows your cursor across every
//! display, with tools for showing people where to look. Built on Rune
//! (github.com/anomalyco/rune): one transparent, click-through overlay per display; a custom
//! compositor draws the pointer and effects at the display's refresh rate.
//!
//! `pointer` runs it. Headless (Rune's agent tooling) it draws one 1920×1080 stage over a
//! sample page instead: `pointer --scene demo --video demo.mp4`, `pointer --scene crack
//! --snap crack.png --at 0.6`, `pointer --list-scenes`.

mod actions;
mod audio;
mod bindings;
mod ctl;
mod demo;
mod draw;
mod fx;
mod gpu;
mod hud;
mod input;
mod login;
mod math;
mod menubar;
mod panel;
mod pointer;
mod promo;
mod settings;
mod sim;
mod stage;
mod style;
mod update;

use std::sync::Arc;

use rune::{App, AppSpec, Chrome, DisplayTarget, OverlayHandle, OverlayInput, OverlayLevel, OverlayOptions, OverlayPlacement, OverlaySpec, RunOptions, Tx, Waker, WindowSpec};

use crate::actions::Host;
use crate::ctl::Ctl;
use crate::math::v2;
use crate::settings::{Shared, Tool};
use crate::stage::{Backdrop, Source, Stage};

impl Host for Stage {
    fn shared(&self) -> &Arc<Shared> {
        Stage::shared(self)
    }
    fn capture(&mut self, _on: bool) {}
}

fn ctl_init() -> Ctl {
    Ctl::new(Shared::load(false), Vec::new(), None)
}

fn ctl_setup(c: &mut Ctl, waker: Waker) {
    c.setup(waker);
}

fn ctl_poll(c: &mut Ctl, _: f64, _: &mut Tx) -> bool {
    c.refresh()
}

/// The settings window's app: windowless until opened (⌃⌥, or the menu bar), with its
/// traffic lights over the sidebar.
fn settings_app(spec_init: fn() -> Ctl) -> App<Ctl> {
    App::new(AppSpec {
        setup: Some(ctl_setup),
        poll: Some(ctl_poll),
        min_size: Some((720.0, 520.0)),
        scenes: SETTINGS_SCENES,
        ..AppSpec::new("Pointer Settings", (860.0, 600.0), spec_init, panel::view)
    })
    .chrome(Chrome { full_size_content: true, traffic_lights: Some((18.0, 18.0)), drag_regions: true })
    .on_close(|c: &mut Ctl, _: &mut Tx| c.settings_closed())
}

type SettingsScene = (&'static str, fn(&mut Ctl));

const SETTINGS_SCENES: &[SettingsScene] = &[
    ("general", |c| c.page = ctl::Page::General),
    ("pointer", |c| c.page = ctl::Page::Pointer),
    ("shortcuts", |c| c.page = ctl::Page::Shortcuts),
    ("updates", |c| c.page = ctl::Page::Updates),
    // As the installed app sees a new release (the website's picture of updating).
    ("update-available", |c| {
        c.page = ctl::Page::Updates;
        c.installed = true;
        c.update = update::Status::Available(update::Release {
            version: "1.1.0".into(),
            notes: "• Performance improvements\n• Bug fixes and polish".into(),
            url: String::new(),
            page: panel::REPO_URL.into(),
        });
    }),
    ("about", |c| c.page = ctl::Page::About),
];

/// Ctrl+C or a TERM from the terminal quits through the app, as ⌃⌥Q does, so the system
/// cursor comes back at once. (macOS also restores it if the process dies some other way.)
#[cfg(unix)]
mod signals {
    unsafe extern "C" {
        fn sigemptyset(set: *mut u32) -> i32;
        fn sigaddset(set: *mut u32, sig: i32) -> i32;
        fn pthread_sigmask(how: i32, set: *const u32, old: *mut u32) -> i32;
        fn sigwait(set: *const u32, sig: *mut i32) -> i32;
    }
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    const SIG_BLOCK: i32 = 1;

    fn set() -> u32 {
        let mut set = 0u32;
        // SAFETY: plain libc calls on a local sigset_t (a u32 on macOS).
        unsafe {
            sigemptyset(&mut set);
            sigaddset(&mut set, SIGINT);
            sigaddset(&mut set, SIGTERM);
        }
        set
    }

    /// Blocks the signals in this thread and every thread it starts afterwards. Call first.
    pub fn block() {
        let set = set();
        // SAFETY: as above.
        unsafe { pthread_sigmask(SIG_BLOCK, &set, std::ptr::null_mut()); }
    }

    /// Waits for one on a thread of its own, then runs `quit`; a second one exits outright.
    pub fn on_quit(quit: impl Fn() + Send + 'static) {
        std::thread::Builder::new().name("pointer-signals".into()).spawn(move || {
            let set = set();
            let mut sig = 0;
            // SAFETY: sigwait on signals this thread (and every other) blocks.
            let r = unsafe { sigwait(&set, &mut sig) };
            if std::env::var_os("POINTER_DEBUG").is_some() { eprintln!("pointer: signal {sig} (sigwait {r}), quitting"); }
            quit();
            unsafe { sigwait(&set, &mut sig); }
            std::process::exit(130);
        }).expect("signal thread");
    }
}

fn setup_live(s: &mut Stage, waker: Waker) {
    s.shared().wakers.lock().unwrap_or_else(|e| e.into_inner()).push(waker.clone());
    stage::watch(s.watch(waker));
}

fn run_live() {
    rune::run_windows_with(RunOptions { accessory: true }, |displays| {
        let shared = Shared::load(true);
        // POINTER_SILENT runs the audio output at zero volume (for checking it without noise).
        let level = if std::env::var_os("POINTER_SILENT").is_some() { 0.0 } else { 1.0 };
        audio::start_live(shared.get().sound, level);
        let mut app = settings_app(ctl_init).windowless().allow_unmodified_shortcuts().on_quit(|_, _| input::set_cursor_hidden(false));
        let mut overlays = Vec::new();
        for (i, display) in displays.iter().enumerate() {
            let bounds = display.native_bounds();
            let target = display.id.clone().map_or(DisplayTarget::Primary, DisplayTarget::Id);
            let spec = OverlaySpec::new("Pointer", (bounds[2] as f32, bounds[3] as f32), hud::view)
                .options(OverlayOptions {
                    display: target,
                    placement: Some(OverlayPlacement::FullScreen),
                    level: OverlayLevel::AboveOverlays,
                    input: OverlayInput::None,
                    // Screen recordings should show the pointer: that's what it's for.
                    exclude_from_capture: false,
                    prewarm: true,
                    ..Default::default()
                })
                // Above pop-up menus (101): with the system cursor hidden, the glove is the
                // only pointer you see, so a context menu mustn't cover it.
                .macos_level(102)
                .poll(stage::poll)
                .setup(setup_live)
                .compositor(gpu::Gfx::new());
            let overlay = app.overlay(spec, Stage::new(i, bounds, shared.clone(), Source::Live));
            overlay.show();
            overlays.push(overlay);
        }
        if std::env::var_os("POINTER_DEBUG").is_some() {
            let watched: Vec<OverlayHandle<Stage>> = overlays.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                for o in &watched { eprintln!("pointer: {}", o.surface_stats().summary()); }
                eprintln!("pointer: audio frames rendered {}", audio::rendered_frames());
            });
        }
        let handle = app.handle();
        let ctl_shared = shared.clone();
        let capture = shared.get().tool.captures();
        app = app.init(move || Ctl::new(ctl_shared, overlays, Some(handle)));
        // Once the app runs: the hotkeys, the menu bar item, and the update checks.
        app.handle().update(move |c, _| {
            c.register_all();
            if let Some(handle) = c.app.clone() { menubar::install(handle); }
            c.start_update_checks();
            c.capture(capture);
            // The installed app's first launch opens Settings, so the menu bar item and the
            // hotkeys are easy to find.
            if update::bundled() && c.shared.first_run() { c.show_page(ctl::Page::General); }
        });
        let st = shared.get();
        actions::set_tool_on(&shared, st.tool);
        shared.update(|s| {
            let help = bindings::keys(&s.bindings, "help");
            s.toast.detail = format!("{help} shows every hotkey · the menu bar has the rest");
            s.toast.keys = help;
        });
        #[cfg(unix)]
        {
            let quit = app.handle();
            signals::on_quit(move || quit.update(|_, tx| { input::set_cursor_hidden(false); tx.quit(); }));
        }
        // For unattended test runs: quit by itself after this many seconds.
        if let Ok(secs) = std::env::var("POINTER_SECONDS").unwrap_or_default().parse::<f64>() {
            let quit = app.handle();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                quit.update(|_, tx| { input::set_cursor_hidden(false); tx.quit(); });
            });
        }
        eprintln!("pointer {}: running on {} display(s). The menu bar item has settings and quit.", update::VERSION, displays.len());
        vec![WindowSpec::from_app("pointer", app)]
    });
}

fn headless_init() -> Stage {
    let mut s = Stage::new(0, [0.0, 0.0, 1920.0, 1080.0], Shared::load(false), Source::Window);
    s.backdrop = Backdrop::Light;
    s
}

fn at(_: f64) -> (math::V2, bool) {
    (v2(1080.0, 560.0), false)
}

fn tool_scene(s: &mut Stage, tool: Tool) {
    s.source = Source::Path(at);
    actions::set_tool_on(s.shared(), tool);
}

/// A pointer style, resting at the usual spot.
fn style_scene(s: &mut Stage, style: style::Style) {
    s.source = Source::Path(at);
    actions::set_style_on(s.shared(), style);
}

/// A pointer style, clicking once at 1.0 s.
fn style_click(s: &mut Stage, style: style::Style) {
    s.source = Source::Path(|t| (v2(1080.0, 560.0), (1.0..1.1).contains(&t)));
    actions::set_style_on(s.shared(), style);
}

/// Framed artwork, held still: the pointer aims at the fingertip spot with the hand fixed
/// off the bottom-right corner. `scripts/art.sh` renders these and cuts the icon's shape.
fn art(s: &mut Stage, style: style::Style, scale: f32, base: math::V2) {
    actions::set_style_on(s.shared(), style);
    s.shared().update(|st| st.scale = scale);
    s.fixed_base = Some(base);
}

/// The app icon, on a 1024×1024 canvas.
const ICON_TIP: math::V2 = v2(372.0, 344.0);

/// A shot of the launch film (`scripts/promo.sh`), on the 1920×1080 stage.
fn promo_scene(s: &mut Stage, shot: promo::Shot) {
    use promo::Shot;
    s.source = Source::Promo(shot);
    s.backdrop = match shot {
        Shot::Hook | Shot::Tap | Shot::Tools => Backdrop::Light,
        Shot::Pointers => Backdrop::Dark,
        Shot::Reveal | Shot::Tech | Shot::Cta | Shot::Wall => Backdrop::None,
    };
    s.start_out = shot.starts_out();
    let c = promo::cue(shot, 0.0);
    s.shared().update(|st| {
        (st.scale, st.tool, st.style, st.shown, st.blade) = (shot.scale(), c.tool, c.style, c.shown, promo::BLADE);
        st.help = false;
    });
}

/// The social card, on a 1200×630 canvas.
const CARD_TIP: math::V2 = v2(905.0, 236.0);

/// A settings thumbnail, on a 256×256 canvas (render with POINTER_TRANSPARENT=1).
fn thumb(s: &mut Stage, style: style::Style, scale: f32, tip: fn(f64) -> (math::V2, bool)) {
    s.source = Source::Path(tip);
    s.backdrop = Backdrop::None;
    art(s, style, scale, v2(330.0, 350.0));
}

type Scene = (&'static str, fn(&mut Stage));

const SCENES: &[Scene] = &[
    ("idle", |s| s.source = Source::Path(at)),
    ("dark", |s| { s.source = Source::Path(at); s.backdrop = Backdrop::Dark; }),
    ("bare", |s| { s.source = Source::Path(at); s.backdrop = Backdrop::None; }),
    ("press", |s| s.source = Source::Path(|t| (v2(1080.0, 560.0), t > 1.0))),
    ("tap", |s| s.source = Source::Path(|t| (v2(1080.0, 560.0), (1.0..1.1).contains(&t)))),
    ("charge", |s| s.source = Source::Path(|t| (v2(1080.0, 560.0), t > 1.0))),
    ("crack", |s| {
        s.source = Source::Path(|t| (v2(1080.0, 560.0), [1.0, 1.15, 1.3, 1.45, 1.6].iter().any(|a| t >= *a && t < a + 0.06)));
    }),
    ("sweep", |s| s.source = Source::Path(|t| {
        let u = (t as f32 - 1.0).clamp(0.0, 0.5) / 0.5;
        (v2(500.0 + 900.0 * crate::math::smoothstep(0.0, 1.0, u), 420.0), false)
    })),
    ("spotlight", |s| tool_scene(s, Tool::Spotlight)),
    ("laser", |s| {
        actions::set_tool_on(s.shared(), Tool::Laser);
        s.source = Source::Path(|t| {
            let u = t as f32;
            (v2(960.0 + 380.0 * (u * 1.7).cos(), 560.0 + 200.0 * (u * 3.4).sin()), false)
        });
    }),
    ("pen", |s| {
        actions::set_tool_on(s.shared(), Tool::Pen);
        s.source = Source::Path(|t| {
            let u = ((t as f32 - 0.8) / 1.2).clamp(0.0, 1.0);
            let a = -1.9 + u * std::f32::consts::TAU * 1.08;
            (v2(1180.0 + 160.0 * a.cos(), 690.0 + 105.0 * a.sin()), (0.8..2.0).contains(&t))
        });
    }),
    ("focus", |s| {
        actions::set_tool_on(s.shared(), Tool::Focus);
        s.source = Source::Path(|t| {
            let u = crate::math::smoothstep(0.8, 1.6, t as f32);
            (v2(760.0 + 410.0 * u, 515.0 + 85.0 * u), (0.8..1.6).contains(&t))
        });
    }),
    ("help", |s| { s.source = Source::Path(at); s.shared().update(|st| st.help = true); }),
    ("perf", |s| { s.source = Source::Path(at); s.shared().update(|st| st.perf = true); }),
    ("demo", |s| s.source = Source::Demo),
    ("saber", |s| style_scene(s, style::Style::Saber)),
    ("wand", |s| style_scene(s, style::Style::Wand)),
    ("mallet", |s| style_scene(s, style::Style::Mallet)),
    ("chicken", |s| style_scene(s, style::Style::Chicken)),
    ("saber-clash", |s| style_click(s, style::Style::Saber)),
    ("wand-cast", |s| style_click(s, style::Style::Wand)),
    ("mallet-bonk", |s| style_click(s, style::Style::Mallet)),
    ("chicken-squawk", |s| style_click(s, style::Style::Chicken)),
    ("saber-dark", |s| { style_scene(s, style::Style::Saber); s.backdrop = Backdrop::Dark; }),
    ("styles", |s| s.source = Source::Styles),
    ("art-icon", |s| {
        s.source = Source::Path(|_| (ICON_TIP, false));
        s.backdrop = Backdrop::Icon(ICON_TIP);
        art(s, style::Style::Glove, 3.4, v2(1130.0, 1220.0));
    }),
    ("promo-hook", |s| promo_scene(s, promo::Shot::Hook)),
    ("promo-reveal", |s| promo_scene(s, promo::Shot::Reveal)),
    ("promo-tap", |s| promo_scene(s, promo::Shot::Tap)),
    ("promo-tools", |s| promo_scene(s, promo::Shot::Tools)),
    ("promo-pointers", |s| promo_scene(s, promo::Shot::Pointers)),
    ("promo-tech", |s| promo_scene(s, promo::Shot::Tech)),
    ("promo-cta", |s| promo_scene(s, promo::Shot::Cta)),
    ("promo-wall", |s| promo_scene(s, promo::Shot::Wall)),
    ("art-card", |s| {
        s.source = Source::Path(|_| (CARD_TIP, false));
        s.backdrop = Backdrop::Card(CARD_TIP);
        art(s, style::Style::Glove, 3.5, v2(1330.0, 930.0));
    }),
    ("art-glove", |s| thumb(s, style::Style::Glove, 2.7, |_| (v2(64.0, 56.0), false))),
    ("art-saber", |s| thumb(s, style::Style::Saber, 0.8, |_| (v2(30.0, 26.0), false))),
    ("art-wand", |s| thumb(s, style::Style::Wand, 2.4, |_| (v2(70.0, 62.0), false))),
    ("art-mallet", |s| thumb(s, style::Style::Mallet, 1.75, |_| (v2(84.0, 146.0), false))),
    ("art-chicken", |s| thumb(s, style::Style::Chicken, 2.0, |_| (v2(52.0, 44.0), false))),
];

/// `--duration S` from the command line, if given.
fn duration_arg() -> Option<f64> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--duration").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

fn run_headless() {
    // POINTER_AUDIO=out.wav: also render the run's sounds, in sync with its frames.
    let wav = std::env::var("POINTER_AUDIO").ok();
    if wav.is_some() { audio::start_recording(); }
    let mut app = App::new(AppSpec { scenes: SCENES, poll: Some(stage::poll), ..AppSpec::new("Pointer", (1920.0, 1080.0), headless_init, hud::view) })
        .compositor(gpu::Gfx::new());
    // POINTER_TRANSPARENT=1: no backdrop at all, so a still keeps its alpha (the thumbnails).
    if std::env::var_os("POINTER_TRANSPARENT").is_some() {
        app = app.window_options(rune::WindowOptions { transparent: true, ..Default::default() });
    }
    for (chord, handler) in actions::defaults::<Stage>() { app = app.global_shortcut(chord, handler); }
    app.run();
    if let Some(path) = wav {
        match audio::write_wav(&path, duration_arg()) {
            Ok(()) => eprintln!("pointer: wrote {path}"),
            Err(e) => eprintln!("pointer: couldn't write {path}: {e}"),
        }
    }
}

fn main() {
    // `pointer --promo-shots`: the launch film's shots and their lengths, for scripts/promo.sh.
    if std::env::args().nth(1).as_deref() == Some("--promo-shots") {
        for (name, shot) in promo::SHOTS { println!("{name} {}", shot.length()); }
        return;
    }
    // `Pointer --update` (from Contents/MacOS): update from the terminal, as Settings would.
    if std::env::args().nth(1).as_deref() == Some("--update") {
        std::process::exit(update::run_cli());
    }
    const HEADLESS: &[&str] = &["--snap", "--video", "--frames", "--tree", "--trace", "--strip", "--script", "--list-scenes", "--list-windows", "--html", "--first-pixels"];
    if std::env::args().any(|a| HEADLESS.contains(&a.as_str())) {
        if std::env::args().any(|a| a == "--settings") {
            // The settings window, headless: `pointer --settings --scene shortcuts --snap s.png`.
            settings_app(|| {
                let mut c = ctl_init();
                c.st.bindings.clear();
                c
            }).run();
            return;
        }
        run_headless();
    } else {
        #[cfg(unix)]
        signals::block();
        run_live();
    }
}
