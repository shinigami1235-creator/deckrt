//! deckrt: the button audit. Every enabled button on every ribbon tab (and every item in the
//! menu a button opens) is clicked on a fresh copy of the sample deck with the right thing
//! selected, and what it did is written down: the deck changed, a dialog, a pane, a menu, the
//! show, the tool, a status line, or nothing at all. Buttons that do nothing, panic or report an
//! error are the ones to look at.
//!
//! Not part of `cargo test`; run it with
//! `DECKRT_AUDIT_OUT=/tmp/audit.jsonl cargo test -p deckcraft-ui-egui audit -- --ignored --nocapture`.

use std::io::Write;

use deckcraft_engine::Session;
use deckcraft_model::ShapeKind;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};

use crate::{Services, SlideApp};

/// A place to start from: which slide, what is selected, which tab.
struct Context {
    name: &'static str,
    tab: &'static str,
    setup: fn(&mut Session),
    /// After the app is up: open a pane or a dialog.
    prep: Option<fn(&mut SlideApp)>,
    /// Where the buttons to click are.
    region: Region,
}

#[derive(Clone, Copy, PartialEq)]
enum Region {
    Ribbon,
    /// The task pane on the right.
    Pane,
    /// Whatever `prep` brought up (a dialog).
    New,
}

fn select_kind(s: &mut Session, slide: usize, pick: fn(&ShapeKind) -> bool) {
    let _ = s.execute("slide.go", &json!({"index": slide}));
    let id = s.doc().ok().and_then(|st| st.current_slide().and_then(|sl| sl.shapes.iter().find(|sh| pick(&sh.kind)).map(|sh| sh.id.0)));
    if let Some(id) = id {
        let _ = s.execute("edit.select", &json!({"ids": [id]}));
    }
}

/// The panes and dialogs: run with `DECKRT_AUDIT_SET=panes`.
fn pane_contexts() -> Vec<Context> {
    macro_rules! pane {
        ($name:expr, $id:expr, $ftab:expr, $setup:expr) => {
            Context {
                name: $name,
                tab: "Home",
                setup: $setup,
                prep: Some(|app: &mut SlideApp| {
                    app.ui.pane = Some($id.into());
                    app.ui.format_tab = $ftab.into();
                }),
                region: Region::Pane,
            }
        };
    }
    macro_rules! dialog {
        ($id:expr, $setup:expr) => {
            Context {
                name: concat!("dialog-", $id),
                tab: "Home",
                setup: $setup,
                prep: Some(|app: &mut SlideApp| app.dialog = Some(crate::dialogs::Dialog::new($id))),
                region: Region::New,
            }
        };
    }
    fn shape(s: &mut Session) {
        select_kind(s, 21, |k| matches!(k, ShapeKind::Shape));
    }
    fn picture(s: &mut Session) {
        select_kind(s, 20, |k| matches!(k, ShapeKind::Picture { .. }));
    }
    vec![
        pane!("pane-fill", "format", "fill", shape),
        pane!("pane-effects", "format", "effects", shape),
        pane!("pane-size", "format", "size", shape),
        pane!("pane-text", "format", "text", shape),
        pane!("pane-picture", "format", "picture", picture),
        pane!("pane-background", "background", "fill", |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }),
        pane!("pane-animation", "animation", "fill", shape),
        pane!("pane-selection", "selection", "fill", shape),
        pane!("pane-comments", "comments", "fill", shape),
        pane!("pane-designer", "designer", "fill", shape),
        pane!("pane-model", super::PANE, "fill", |s| select_live(s, 1)),
        pane!("pane-swap", super::PANE, "fill", |s| select_live(s, 2)),
        pane!("pane-wipe", super::PANE, "fill", |s| select_live(s, 3)),
        pane!("pane-scrub", super::PANE, "fill", |s| select_live(s, 4)),
        pane!("pane-zoom", super::PANE, "fill", |s| select_live(s, 5)),
        pane!("pane-popup", super::PANE, "fill", |s| select_live(s, 6)),
        pane!("pane-poll", super::PANE, "fill", |s| select_live(s, 7)),
        pane!("pane-measure", super::PANE, "fill", |s| select_live(s, 9)),
        pane!("pane-injection", super::PANE, "fill", |s| select_live(s, 10)),
        dialog!("table", shape),
        dialog!("slideSize", shape),
        dialog!("sizePosition", shape),
        dialog!("headerFooter", shape),
        dialog!("hyperlink", shape),
        dialog!("setupShow", shape),
        dialog!("zoom", shape),
        dialog!("comment", shape),
        dialog!("altText", shape),
        dialog!("paragraph", shape),
        dialog!("tabs", shape),
        dialog!("font", shape),
        dialog!("about", shape),
        dialog!("preferences", shape),
        dialog!("autocorrect", shape),
        dialog!("gridOptions", shape),
        dialog!("export", shape),
        dialog!("symbol", shape),
        dialog!("photoAlbum", shape),
        dialog!("replaceFonts", shape),
        dialog!("equation", shape),
        dialog!("dateTime", shape),
        dialog!("spelling", shape),
        dialog!("find", shape),
        dialog!("accessibility", shape),
        dialog!("language", shape),
        dialog!("customShow", shape),
        dialog!("smartart", shape),
        dialog!("bullets", shape),
        dialog!("action", shape),
        dialog!("themeColors", shape),
        dialog!("themeFonts", shape),
        dialog!("renameShape", shape),
        dialog!("outline", shape),
        dialog!("trim", shape),
        dialog!("pasteSpecial", shape),
    ]
}

fn select_live(s: &mut Session, slide: usize) {
    let _ = s.execute("slide.go", &json!({"index": slide}));
    let id = s.doc().ok().and_then(|st| st.current_slide().and_then(|sl| sl.shapes.iter().find(|sh| deckcraft_interactive::object::is_live(sh)).map(|sh| sh.id.0)));
    if let Some(id) = id {
        let _ = s.execute("edit.select", &json!({"ids": [id]}));
    }
}

fn contexts() -> Vec<Context> {
    if std::env::var("DECKRT_AUDIT_SET").as_deref() == Ok("panes") {
        return pane_contexts();
    }
    vec![
        Context { name: "home-nothing", tab: "Home", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "home-shape", tab: "Home", setup: |s| select_kind(s, 21, |k| matches!(k, ShapeKind::Shape)), prep: None, region: Region::Ribbon },
        Context { name: "insert", tab: "Insert", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "draw", tab: "Draw", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "design", tab: "Design", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "transitions", tab: "Transitions", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "animations", tab: "Animations", setup: |s| select_kind(s, 21, |k| matches!(k, ShapeKind::Shape)), prep: None, region: Region::Ribbon },
        Context { name: "slideshow", tab: "Slide Show", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "record", tab: "Record", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "review", tab: "Review", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 20})); }, prep: None, region: Region::Ribbon },
        Context { name: "view", tab: "View", setup: |s| { let _ = s.execute("slide.go", &json!({"index": 2})); }, prep: None, region: Region::Ribbon },
        Context { name: "shape-format", tab: "Shape Format", setup: |s| select_kind(s, 21, |k| matches!(k, ShapeKind::Shape)), prep: None, region: Region::Ribbon },
        Context { name: "picture-format", tab: "Picture Format", setup: |s| select_kind(s, 20, |k| matches!(k, ShapeKind::Picture { .. })), prep: None, region: Region::Ribbon },
        Context {
            name: "table",
            tab: "Table Design",
            setup: |s| {
                let _ = s.execute("slide.go", &json!({"index": 22}));
                let _ = s.execute("insert.table", &json!({"rows": 3, "cols": 3}));
            },
            prep: None,
            region: Region::Ribbon,
        },
        Context {
            name: "table-layout",
            tab: "Layout",
            setup: |s| {
                let _ = s.execute("slide.go", &json!({"index": 22}));
                let _ = s.execute("insert.table", &json!({"rows": 3, "cols": 3}));
            },
            prep: None,
            region: Region::Ribbon,
        },
        Context {
            name: "chart",
            tab: "Chart Design",
            setup: |s| {
                let _ = s.execute("slide.go", &json!({"index": 22}));
                let _ = s.execute("insert.chart", &json!({"type": "Column"}));
            },
            prep: None,
            region: Region::Ribbon,
        },
        Context {
            name: "smartart",
            tab: "SmartArt Design",
            setup: |s| {
                let _ = s.execute("slide.go", &json!({"index": 22}));
                let _ = s.execute("insert.smartArt", &json!({"kind": "process", "items": ["One", "Two", "Three"]}));
            },
            prep: None,
            region: Region::Ribbon,
        },
    ]
}

const W: f32 = 1920.0;
const H: f32 = 1000.0;
/// The ribbon's band: the tab row and the group card.
const RIBBON_TOP: f32 = 55.0;
const RIBBON_BOTTOM: f32 = 175.0;

fn harness(cx: &Context) -> Harness<'static, SlideApp> {
    let mut s = Session::new();
    s.execute("deckrt.openSample", &json!({})).expect("sample");
    (cx.setup)(&mut s);
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = cx.tab.to_string();
    let mut h = Harness::builder().with_size(egui::vec2(W, H)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.state_mut().ui.tab = cx.tab.to_string();
    h.run_steps(3);
    if let Some(prep) = cx.prep {
        prep(h.state_mut());
        h.run_steps(4);
    }
    h
}

/// The same start without `prep`: what was on screen before the pane or dialog.
fn baseline(cx: &Context) -> Vec<egui::Rect> {
    let plain = Context { name: cx.name, tab: cx.tab, setup: cx.setup, prep: None, region: cx.region };
    all_rects(&harness(&plain))
}

/// The enabled controls in the context's region.
fn targets(cx: &Context, h: &Harness<'_, SlideApp>) -> (Vec<Target>, Vec<String>) {
    match cx.region {
        Region::Ribbon => ribbon_buttons(h),
        Region::Pane | Region::New => {
            let before = if cx.region == Region::New { baseline(cx) } else { vec![] };
            let left = W - h.state().ui.pane_width - 4.0;
            let mut on = vec![];
            let mut off = vec![];
            let nodes = h
                .query_all_by_role(Role::Button)
                .chain(h.query_all_by_role(Role::CheckBox))
                .chain(h.query_all_by_role(Role::ComboBox))
                .chain(h.query_all_by_role(Role::RadioButton));
            for n in nodes {
                let r = n.rect();
                let inside = match cx.region {
                    Region::Pane => r.center().x > left && r.center().y > RIBBON_BOTTOM,
                    _ => !before.iter().any(|b| (b.center() - r.center()).length() < 1.0),
                };
                if !inside {
                    continue;
                }
                let label = n.accesskit_node().label().unwrap_or_default().replace('\n', " ");
                if n.accesskit_node().is_disabled() {
                    off.push(label);
                } else {
                    on.push(Target { label, rect: r });
                }
            }
            (on, off)
        }
    }
}

/// What the app looks like, for telling what a click changed.
#[derive(Clone, Debug, PartialEq)]
struct Snap {
    revision: u64,
    undo: usize,
    docs: usize,
    dialog: Option<String>,
    pane: Option<String>,
    tab: String,
    tool: String,
    show: bool,
    backstage: bool,
    status: Option<String>,
    selection: String,
    slide: usize,
    view: String,
    nodes: usize,
    ui: String,
    previews: (bool, bool, bool),
}

fn snap(h: &Harness<'_, SlideApp>) -> Snap {
    let app = h.state();
    let st = app.session.active();
    Snap {
        revision: st.map_or(0, |d| d.revision),
        undo: st.map_or(0, |d| d.history.undo.len()),
        docs: app.session.documents().len(),
        dialog: app.dialog.as_ref().map(|d| d.id.clone()),
        pane: app.ui.pane.clone(),
        tab: app.ui.tab.clone(),
        tool: format!("{:?}", app.session.tool.kind),
        show: app.show.is_some(),
        backstage: app.backstage_open(),
        status: app.status.as_ref().map(|s| s.0.clone()),
        selection: st.map(|d| format!("{:?}", d.selection.shapes)).unwrap_or_default(),
        slide: st.map_or(0, |d| d.selection.slide),
        view: format!("{:?}", app.ui.view),
        nodes: h.query_all(egui_kittest::kittest::by()).count(),
        ui: serde_json::to_string(&app.ui).unwrap_or_default(),
        previews: (app.anim_preview_active(), super::preview::active(app), app.deckrt.screenshot_open()),
    }
}

fn effect(a: &Snap, b: &Snap, popup: usize) -> Vec<String> {
    let mut out = vec![];
    if b.revision != a.revision || b.undo != a.undo {
        out.push(format!("deck changed (undo {} → {})", a.undo, b.undo));
    }
    if b.docs != a.docs {
        out.push(format!("documents {} → {}", a.docs, b.docs));
    }
    if b.dialog != a.dialog {
        out.push(format!("dialog {:?}", b.dialog));
    }
    if b.pane != a.pane {
        out.push(format!("pane {:?}", b.pane));
    }
    if b.tab != a.tab {
        out.push(format!("tab {}", b.tab));
    }
    if b.tool != a.tool {
        out.push(format!("tool {}", b.tool));
    }
    if b.show != a.show {
        out.push("show".into());
    }
    if b.backstage != a.backstage {
        out.push("backstage".into());
    }
    if b.selection != a.selection || b.slide != a.slide {
        out.push("selection".into());
    }
    if b.view != a.view {
        out.push(format!("view {}", b.view));
    }
    if b.ui != a.ui && b.pane == a.pane && b.tab == a.tab && b.view == a.view {
        out.push("view setting".into());
    }
    if b.previews != a.previews {
        out.push(format!("preview {:?}", b.previews));
    }
    if popup > 0 {
        out.push(format!("menu with {popup} buttons"));
    } else if b.nodes > a.nodes + 2 {
        out.push(format!("{} new items on screen", b.nodes - a.nodes));
    }
    if b.status != a.status
        && let Some(s) = &b.status
    {
        out.push(format!("status: {s}"));
    }
    out
}

#[derive(Clone, Debug)]
struct Target {
    label: String,
    rect: egui::Rect,
}

/// The enabled buttons in the ribbon band (and those of the More menu are found by clicking More).
fn ribbon_buttons(h: &Harness<'_, SlideApp>) -> (Vec<Target>, Vec<String>) {
    let mut on = vec![];
    let mut off = vec![];
    let nodes = h.query_all_by_role(Role::Button).chain(h.query_all_by_role(Role::CheckBox)).chain(h.query_all_by_role(Role::ComboBox));
    for n in nodes {
        let r = n.rect();
        if r.center().y < RIBBON_TOP || r.center().y > RIBBON_BOTTOM {
            continue;
        }
        let label = n.accesskit_node().label().unwrap_or_default().replace('\n', " ");
        // The tab row: switching tabs is not what is being checked.
        if label == "File" || crate::ribbon::TABS.contains(&label.as_str()) {
            continue;
        }
        if n.accesskit_node().is_disabled() {
            off.push(label);
        } else {
            on.push(Target { label, rect: r });
        }
    }
    (on, off)
}

/// Buttons that appeared outside the ribbon band after a click: a menu's items.
fn popup_buttons(h: &Harness<'_, SlideApp>, before: &[egui::Rect]) -> Vec<Target> {
    h.query_all_by_role(Role::Button)
        .filter(|n| {
            let r = n.rect();
            !n.accesskit_node().is_disabled() && !before.iter().any(|b| (b.center() - r.center()).length() < 1.0)
        })
        .map(|n| Target { label: n.accesskit_node().label().unwrap_or_default().replace('\n', " "), rect: n.rect() })
        .collect()
}

fn all_rects(h: &Harness<'_, SlideApp>) -> Vec<egui::Rect> {
    h.query_all_by_role(Role::Button).map(|n| n.rect()).collect()
}

fn click_at(h: &mut Harness<'_, SlideApp>, r: egui::Rect) {
    let p = r.center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(4);
}

/// One click on a fresh copy, optionally a second on an item of the menu the first opened.
fn try_click(cx: &Context, first: &Target, second: Option<&Target>) -> Result<(Vec<String>, Vec<Target>), String> {
    let first = first.clone();
    let second = second.cloned();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut h = harness(cx);
        let rects = all_rects(&h);
        let a = snap(&h);
        click_at(&mut h, first.rect);
        let opened = popup_buttons(&h, &rects);
        match second {
            None => {
                let b = snap(&h);
                (effect(&a, &b, opened.len()), opened)
            }
            Some(item) => {
                // Bring the item into view first: long menus scroll.
                let near = |r: egui::Rect| (r.center() - item.rect.center()).length();
                if let Some(n) = h.query_all_by_role(Role::Button).filter(|n| n.accesskit_node().label().unwrap_or_default().replace('\n', " ") == item.label).min_by(|a, b| near(a.rect()).total_cmp(&near(b.rect()))) {
                    n.scroll_to_me();
                }
                h.run_steps(3);
                let target = h
                    .query_all_by_role(Role::Button)
                    .filter(|n| n.accesskit_node().label().unwrap_or_default().replace('\n', " ") == item.label)
                    .map(|n| n.rect())
                    .min_by(|a, b| near(*a).total_cmp(&near(*b)))
                    .unwrap_or(item.rect);
                let mid = snap(&h);
                let rects2 = all_rects(&h);
                click_at(&mut h, target);
                let b = snap(&h);
                let more = popup_buttons(&h, &rects2);
                let mut e = effect(&mid, &b, 0);
                if !more.is_empty() && e.is_empty() {
                    e.push(format!("submenu with {} buttons", more.len()));
                }
                (e, vec![])
            }
        }
    }));
    res.map_err(|p| p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into()))
}

#[test]
#[ignore = "the button audit; slow, writes a report"]
fn audit() {
    let out = std::env::var("DECKRT_AUDIT_OUT").unwrap_or_else(|_| "/tmp/deckrt-audit.jsonl".into());
    let only = std::env::var("DECKRT_AUDIT_ONLY").ok();
    let mut f = std::fs::File::create(&out).expect("report file");
    let mut write = |v: Value| {
        let _ = writeln!(f, "{v}");
    };
    for cx in contexts() {
        if only.as_deref().is_some_and(|o| o != cx.name) {
            continue;
        }
        let h = harness(&cx);
        let (buttons, disabled) = targets(&cx, &h);
        drop(h);
        write(json!({"context": cx.name, "tab": cx.tab, "buttons": buttons.iter().map(|b| format!("{} @{:.0}", b.label, b.rect.center().y)).collect::<Vec<_>>(), "disabled": disabled}));
        for b in &buttons {
            match try_click(&cx, b, None) {
                Err(p) => write(json!({"context": cx.name, "button": b.label, "panic": p})),
                Ok((e, items)) => {
                    write(json!({"context": cx.name, "button": b.label, "effect": e}));
                    // The menu's items, each on a fresh copy, when the click only opened a menu.
                    let only_menu = e.len() == 1 && (e[0].starts_with("menu with") || e[0].ends_with("new items on screen"));
                    for it in items.iter().take(if only_menu { 80 } else { 0 }) {
                        match try_click(&cx, b, Some(it)) {
                            Err(p) => write(json!({"context": cx.name, "button": b.label, "item": it.label, "panic": p})),
                            Ok((e, _)) => write(json!({"context": cx.name, "button": b.label, "item": it.label, "effect": e})),
                        }
                    }
                }
            }
        }
    }
}
