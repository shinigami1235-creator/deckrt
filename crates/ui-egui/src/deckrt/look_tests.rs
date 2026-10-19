//! deckrt: the ribbon, menus, dialogs and the Animation Pane laid out as PowerPoint's, driven
//! through the UI the way a person would.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::{Services, SlideApp};

fn harness(tab: &str, setup: impl FnOnce(&mut Session)) -> Harness<'static, SlideApp> {
    let mut s = Session::new();
    s.execute("deckrt.openSample", &json!({})).expect("sample");
    let _ = s.execute("slide.go", &json!({"index": 0}));
    setup(&mut s);
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = tab.to_string();
    let mut h = Harness::builder().with_size(egui::vec2(1920.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.state_mut().ui.tab = tab.to_string();
    h.run_steps(4);
    h
}

fn click(h: &mut Harness<'_, SlideApp>, label: &str) {
    let r = h.query_all_by_label(label).find(|n| n.accesskit_node().role() != Role::Label).unwrap_or_else(|| panic!("no `{label}`")).rect();
    let p = r.center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(6);
}

fn has(h: &Harness<'_, SlideApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn shape_count(h: &Harness<'_, SlideApp>) -> usize {
    h.state().session.active().map(|d| d.shapes().len()).unwrap_or(0)
}

#[test]
fn the_font_group_arrow_opens_the_font_dialog() {
    let mut h = harness("Home", |s| {
        let _ = s.execute("edit.selectAll", &json!({}));
    });
    click(&mut h, "Font options");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("font"));
}

#[test]
fn the_inline_shapes_pick_a_shape_to_draw() {
    let mut h = harness("Home", |_| {});
    click(&mut h, "Oval");
    assert!(matches!(&h.state().session.tool.kind, deckcraft_engine::ToolKind::Shape { preset } if preset == "ellipse"));
    // Used last, it comes first in the menu.
    click(&mut h, "More shapes");
    assert!(has(&h, "Recently Used Shapes"));
}

#[test]
fn wordart_offers_powerpoints_twenty_styles() {
    let mut h = harness("Insert", |_| {});
    click(&mut h, "WordArt");
    for _ in 0..12 {
        h.run_steps(1); // the pictures fill in a few a frame
    }
    for name in deckcraft_engine::cmd::deckrt_wordart::NAMES {
        assert!(has(&h, name), "{name}");
    }
    let before = shape_count(&h);
    click(&mut h, deckcraft_engine::cmd::deckrt_wordart::NAMES[12]);
    assert_eq!(shape_count(&h), before + 1);
}

#[test]
fn insert_chart_is_a_dialog_with_types_by_category() {
    let mut h = harness("Insert", |s| {
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
    });
    click(&mut h, "Chart");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("chart"));
    click(&mut h, "Line");
    click(&mut h, "Line with Markers");
    click(&mut h, "OK");
    assert!(h.state().dialog.is_none());
    let d = h.state().session.active().unwrap();
    let chart = d.shapes().iter().find_map(|s| if let deckcraft_model::ShapeKind::Chart(c) = &s.kind { Some(c.clone()) } else { None });
    assert_eq!(chart.map(|c| c.kind), Some(deckcraft_model::chart::ChartType::LineMarkers));
}

#[test]
fn smartart_dialog_takes_enter_in_its_text_box() {
    let mut h = harness("Insert", |s| {
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
    });
    let before = shape_count(&h);
    click(&mut h, "SmartArt");
    click(&mut h, "Pyramid");
    click(&mut h, "Basic Pyramid");
    // Typing a new line in the text box doesn't insert the graphic.
    let text = h
        .query_all_by_role(Role::MultilineTextInput)
        .find(|n| n.accesskit_node().value().is_some_and(|v| v.contains("Plan")))
        .expect("the text box")
        .rect();
    h.input_mut().events.push(egui::Event::PointerButton { pos: text.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.input_mut().events.push(egui::Event::PointerButton { pos: text.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("smartart"));
    click(&mut h, "OK");
    assert!(h.state().dialog.is_none());
    assert!(shape_count(&h) > before);
}

#[test]
fn the_animation_pane_row_arrow_opens_its_menu_and_reorders() {
    let mut h = harness("Animations", |s| {
        let _ = s.execute("slide.new", &json!({"layout": "title"}));
        let _ = s.execute("edit.selectAll", &json!({}));
        let _ = s.execute("animation.set", &json!({"effect": "fly", "class": "entr"}));
        let ids: Vec<u32> = s.active().unwrap().shapes().iter().map(|x| x.id.0).collect();
        let _ = s.execute("edit.select", &json!({"ids": [ids[0]]}));
        let _ = s.execute("view.pane", &json!({"pane": "animation"}));
    });
    h.state_mut().ui.pane = Some("animation".into());
    h.run_steps(4);
    let first = h.state().session.active().unwrap().current_slide().unwrap().animations[0].shape;
    click(&mut h, "Effect menu");
    assert!(has(&h, "Remove"), "the row's menu");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    let _ = h.state_mut().session.execute("edit.select", &json!({"ids": [first.0]}));
    h.run_steps(3);
    // The pane's own arrow (the ribbon has a Move Later too).
    let r = h.query_all_by_label("Move Later").last().unwrap().rect();
    h.input_mut().events.push(egui::Event::PointerMoved(r.center()));
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::PointerButton { pos: r.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(1);
    h.input_mut().events.push(egui::Event::PointerButton { pos: r.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(4);
    let now = h.state().session.active().unwrap().current_slide().unwrap().animations[1].shape;
    assert_eq!(now, first);
}

#[test]
fn a_click_on_the_open_tab_keeps_the_ribbon_and_a_double_click_hides_it() {
    let mut h = harness("Home", |_| {});
    let tab = |h: &Harness<'_, SlideApp>| h.query_all_by_label("Home").find(|n| n.rect().min.y > 45.0 && n.rect().min.y < 90.0).unwrap().rect().center();
    let press = |h: &mut Harness<'_, SlideApp>, p: egui::Pos2, frames: usize| {
        h.input_mut().events.push(egui::Event::PointerMoved(p));
        h.input_mut().events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        h.run_steps(1);
        h.input_mut().events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        h.run_steps(frames);
    };
    let p = tab(&h);
    press(&mut h, p, 40);
    assert!(!h.state().ui.ribbon_collapsed, "one click leaves it open");
    press(&mut h, p, 2);
    press(&mut h, p, 40);
    assert!(h.state().ui.ribbon_collapsed, "a double-click hides it");
    press(&mut h, p, 2);
    press(&mut h, p, 40);
    assert!(!h.state().ui.ribbon_collapsed, "a double-click brings it back");
}

#[test]
fn change_shape_offers_only_closed_shapes() {
    let mut h = harness("Shape Format", |s| {
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
        let _ = s.execute("shape.insert", &json!({"preset": "rect"}));
    });
    h.state_mut().ui.tab = "Shape Format".into();
    h.run_steps(3);
    click(&mut h, "Edit Shape");
    assert!(has(&h, "Oval"));
    for gone in ["Line", "Curve", "Freeform: Scribble", "Text Box"] {
        assert!(!h.query_all_by_label(gone).any(|n| n.rect().min.y > 150.0), "{gone} offered");
    }
}
