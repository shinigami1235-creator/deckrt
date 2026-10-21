//! deckrt: an agent builds a deck with every live object through MCP, saves it as .pptx and
//! finds the objects again; and the window-or-headless backend.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::{Backend, Headless, SHUT, Server, Window};

/// A 1×1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

fn call(s: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    serde_json::from_str(&s.handle_line(&line).expect("a reply")).expect("json")
}

fn raw(s: &mut Server, name: &str, args: Value) -> Value {
    call(s, "tools/call", json!({"name": name, "arguments": args}))["result"].clone()
}

fn tool(s: &mut Server, name: &str, args: Value) -> Value {
    let r = raw(s, name, args);
    assert_eq!(r["isError"], false, "{name} failed: {r}");
    let text = r["content"].as_array().and_then(|c| c.iter().find(|c| c["type"] == "text")).and_then(|c| c["text"].as_str()).unwrap_or("");
    serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
}

fn pic(name: &str) -> Value {
    json!({"data": PNG, "name": name})
}

#[test]
fn an_agent_builds_every_live_object_and_they_survive_pptx() {
    let mut s = Server::new(Box::new(Headless::new()));
    let tools = call(&mut s, "tools/list", json!({}));
    let names: Vec<String> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in [
        "add_poll",
        "add_popup",
        "add_image_swap",
        "add_wipe",
        "add_scrub",
        "add_zoom",
        "add_3d_model",
        "add_measure",
        "add_injection_map",
        "set_notes",
        "open_in_deckrt",
    ] {
        assert!(names.iter().any(|x| x == n), "{n}");
    }
    let init = call(&mut s, "initialize", json!({}));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("add_poll"));

    tool(&mut s, "new_presentation", json!({"theme": "Harbor"}));
    tool(&mut s, "add_slide", json!({"layout": "titleOnly", "title": "Skin layers"}));
    let poll = tool(
        &mut s,
        "add_poll",
        json!({"question": "Which layer holds the collagen?", "options": ["Epidermis", "Dermis", "Fat"], "correct": 1, "leads_to": [null, 0, null]}),
    );
    assert_eq!(poll["object"]["config"]["correct"], 1);
    tool(
        &mut s,
        "add_popup",
        json!({"label": "Dermis", "text": "Collagen and elastin.", "x": 60, "y": 380, "w": 200, "h": 50, "panel": [480, 120, 420, 300]}),
    );
    tool(&mut s, "add_image_swap", json!({"pictures": [pic("a.png"), pic("b.png"), pic("c.png")], "next_clicks": 2, "label": "Stages"}));
    tool(&mut s, "add_wipe", json!({"before": pic("before.png"), "after": pic("after.png"), "position": 0.3}));
    tool(&mut s, "add_scrub", json!({"pictures": [pic("d0.png"), pic("d7.png")], "labels": ["Day 0", "Day 7"]}));
    tool(&mut s, "add_zoom", json!({"x": 100, "y": 100, "w": 200, "h": 100, "max_scale": 3}));
    let photo = tool(&mut s, "insert_picture", json!({"data": PNG, "name": "face.png", "x": 500, "y": 80, "w": 380, "h": 440}));
    let photo_id = photo["id"].as_u64().unwrap();
    tool(&mut s, "add_measure", json!({"over": photo_id, "preset": "fifths"}));
    tool(&mut s, "add_injection_map", json!({"over": photo_id, "products": [{"name": "Filler", "unit": "mL", "dose": 0.2}]}));
    tool(&mut s, "set_notes", json!({"text": "Ask the room first, then open the poll."}));

    let list = tool(&mut s, "list_interactive", json!({}));
    let kinds: Vec<&str> = list.as_array().unwrap().iter().map(|o| o["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["poll", "popup", "image_swap", "wipe", "scrub", "zoom", "measure", "injection_map"]);
    let find = |k: &str| list.as_array().unwrap().iter().find(|o| o["kind"] == k).unwrap().clone();
    let popup = find("popup");
    assert_eq!(popup["object"]["config"]["panel_position"], json!([0.5, 120.0 / 540.0]));
    assert_eq!(popup["object"]["trigger"], "audience_click");
    assert_eq!(find("image_swap")["object"]["next_clicks"], 2);
    assert_eq!(find("scrub")["object"]["config"]["labels"], json!(["Day 0", "Day 7"]));
    let measure = find("measure");
    assert_eq!(measure["rect"], json!([500.0, 80.0, 380.0, 440.0]), "{measure}");
    assert_eq!(find("injection_map")["object"]["config"]["products"][0]["unit"], "mL");
    assert!(find("poll")["object"]["config"]["targets"][1].is_string(), "the answer leads to slide 0");

    // Mistakes come back as tool errors the agent can read.
    assert_eq!(raw(&mut s, "add_poll", json!({"question": "One?", "options": ["Only"]}))["isError"], true);
    assert_eq!(raw(&mut s, "add_zoom", json!({}))["isError"], true);
    assert_eq!(raw(&mut s, "add_measure", json!({"over": 99999}))["isError"], true);

    // Saved as .pptx and opened again, every object is there.
    let dir = std::env::temp_dir().join(format!("deckrt-mcp-objects-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("skin.pptx");
    tool(&mut s, "save_presentation", json!({"path": path.to_string_lossy()}));
    tool(&mut s, "open_presentation", json!({"path": path.to_string_lossy()}));
    tool(&mut s, "go_to_slide", json!({"index": 1}));
    let again = tool(&mut s, "list_interactive", json!({}));
    assert_eq!(again.as_array().unwrap().len(), 8, "{again}");
    let doc = tool(&mut s, "inspect_document", json!({}));
    assert_eq!(doc["slides"][1]["notes"], "Ask the room first, then open the poll.");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in deckrt window: answers every request with `answer(method, params)` and records
/// the methods.
fn fake_window(answer: fn(&str, &Value) -> Value) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let seen = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in l.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut out = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let v: Value = serde_json::from_str(&line).unwrap();
                    let m = v["method"].as_str().unwrap_or("").to_string();
                    let cmd = v["params"]["command"].as_str().map(|c| format!("{m} {c}")).unwrap_or(m.clone());
                    log.lock().unwrap().push(cmd);
                    let mut r = answer(&m, &v["params"]);
                    r["id"] = v["id"].clone();
                    if writeln!(out, "{r}").is_err() {
                        break;
                    }
                }
            });
        }
    });
    (addr, seen)
}

#[test]
fn without_a_window_the_work_is_headless_and_moves_in_later() {
    // Nothing listening: headless.
    let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
    let mut w = Window::new(&free);
    assert!(w.describe().contains("otherwise headless"));
    w.call("engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    assert!(!w.has_ui());
    assert!(w.describe().starts_with("headless"));
    assert!(w.use_window().is_err(), "no window to move to");

    // A window that keeps agents out: still headless, and the agent is told how to let it in.
    let (shut, _) = fake_window(|_, _| json!({"ok": false, "error": format!("{SHUT}: turn it on in AI Agents")}));
    let mut w = Window::new(&shut);
    w.call("engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    assert!(!w.has_ui());
    let e = w.use_window().unwrap_err();
    assert!(e.contains("Let agents work in this window"), "{e}");

    // A window that lets agents in: the headless deck goes across whole, and calls go there.
    let (open, seen) = fake_window(|m, _| match m {
        "deckrt.ping" => json!({"ok": true, "result": {"app": "deckrt"}}),
        _ => json!({"ok": true, "result": {"document": 1}}),
    });
    let mut w = Window::new(&open);
    // The first call found no window (it opened later): built headless.
    w.at_headless_for_test();
    w.call("engine.execute", json!({"command": "file.new", "params": {}})).unwrap();
    let r = w.use_window().unwrap();
    assert_eq!(r["moved"], true, "{r}");
    assert!(w.has_ui());
    w.call("document.inspect", json!({})).unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, ["deckrt.ping", "engine.execute file.openBytes", "document.inspect"]);
}

#[test]
fn an_open_window_is_used_from_the_first_call() {
    let (open, seen) = fake_window(|m, _| match m {
        "deckrt.ping" => json!({"ok": true, "result": {"app": "deckrt"}}),
        _ => json!({"ok": true, "result": {}}),
    });
    let mut w = Window::new(&open);
    w.call("document.inspect", json!({})).unwrap();
    assert!(w.has_ui());
    assert_eq!(seen.lock().unwrap().clone(), ["deckrt.ping", "document.inspect"]);
}
