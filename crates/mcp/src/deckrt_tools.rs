//! deckrt: MCP tools for the objects deckrt adds to a slide (polls, popups, image swaps, wipes,
//! scrubs, zoom regions, 3D models, measurements, injection maps) and for speaker notes. Each one
//! turns plain arguments into an `interactive.insert` (or `slide.notes`) call, so an agent never
//! has to know the settings format the deck stores.

use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::tools::ToolResult;

fn num(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}
fn int(desc: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "description": desc})
}
fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}
fn boolean(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}
fn picture(desc: &str) -> Value {
    json!({
        "description": desc,
        "oneOf": [
            {"type": "string", "description": "File path"},
            {"type": "object", "properties": {"data": {"type": "string", "description": "Base64 file data"}, "name": {"type": "string", "description": "File name with its extension"}}, "required": ["data", "name"], "additionalProperties": false}
        ]
    })
}
fn pictures(desc: &str) -> Value {
    json!({"type": "array", "items": picture("A picture"), "description": desc})
}

/// The box, label and firing arguments every object takes.
fn common(mut props: Map<String, Value>, clickable: bool) -> Map<String, Value> {
    props.insert("x".into(), num("Left (pt)"));
    props.insert("y".into(), num("Top (pt)"));
    props.insert("w".into(), num("Width (pt)"));
    props.insert("h".into(), num("Height (pt)"));
    props.insert(
        "over".into(),
        int("Id of a shape or picture on the slide (from inspect_slide) whose box the object takes, e.g. the face photo a measurement goes over"),
    );
    props.insert("label".into(), string("Name in presenter view and the pane"));
    props.insert("shortcut".into(), string("One key that fires it during the show"));
    if clickable {
        props.insert(
            "trigger".into(),
            json!({"type": "string", "enum": ["click", "presenter"], "description": "click (default): a click on it in the show fires it; presenter: only presenter view fires it"}),
        );
        props.insert(
            "next_clicks".into(),
            int("Presses of Next (clicker, arrow keys) it takes before the show moves on, e.g. 2 for an image swap of 3 pictures"),
        );
    }
    props
}

fn obj(props: Map<String, Value>, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}

fn props(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn tool(name: &str, title: &str, desc: &str, schema: Value, read_only: bool) -> Value {
    json!({"name": name, "title": title, "description": desc, "inputSchema": schema, "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": false}})
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "add_poll",
            "Add poll",
            "A poll the audience answers on their phones (they scan the QR code presenter view shows). Kinds: choice (one answer, optionally a correct one), word_cloud (up to three words each), free_text, rating (1 to scale), ranking (put the options in order). The phones see it once the presenter opens voting. Default box: most of the slide under the title.",
            obj(
                common(
                    props(json!({
                        "question": string("The question"),
                        "kind": {"type": "string", "enum": ["choice", "word_cloud", "free_text", "rating", "ranking"], "description": "Default choice"},
                        "options": {"type": "array", "items": {"type": "string"}, "description": "Answers for choice and ranking (two or more)"},
                        "correct": int("Index of the right answer (choice); the reveal marks it"),
                        "scale": int("Top of a rating scale, 2 to 10 (default 5)"),
                        "leads_to": {"type": "array", "items": {"type": ["integer", "null"]}, "description": "Choice: per option, the slide index the show offers to jump to when that answer wins, or null"}
                    })),
                    false,
                ),
                &["question"],
            ),
            false,
        ),
        tool(
            "add_popup",
            "Add popup",
            "A labelled button; clicking it in the show opens a note panel with text, a picture or a web page, and clicking the panel closes it. Good for definitions and side details. Put several on one slide for a set of labels.",
            obj(
                common(
                    props(json!({
                        "text": string("Text the panel shows"),
                        "picture": picture("A picture the panel shows instead of text"),
                        "url": string("A web page the panel opens instead"),
                        "panel": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4, "description": "The panel's box [x, y, w, h] in pt (default: the right half of the slide)"}
                    })),
                    true,
                ),
                &["label"],
            ),
            false,
        ),
        tool(
            "add_image_swap",
            "Add image swap",
            "Several pictures in one frame, shown one at a time: a click (or Next) shows the next, or `picker` shows a strip to choose from. For stages, before/after series, variations.",
            obj(
                common(
                    props(json!({
                        "pictures": pictures("Two or more pictures, in order"),
                        "on_click": {"type": "string", "enum": ["advance", "picker"], "description": "Default advance"},
                        "start": int("Picture shown when the slide opens (default 0)")
                    })),
                    true,
                ),
                &["pictures"],
            ),
            false,
        ),
        tool(
            "add_wipe",
            "Add before/after wipe",
            "Two pictures under a divider the presenter drags across to compare them (before on the left).",
            obj(
                common(
                    props(json!({
                        "before": picture("The before picture"),
                        "after": picture("The after picture"),
                        "position": num("Divider position when the slide opens, 0 to 1 (default 0.5)")
                    })),
                    true,
                ),
                &["before", "after"],
            ),
            false,
        ),
        tool(
            "add_scrub",
            "Add scrub",
            "2 to 12 pictures stepped through with a slider, each with a label (Day 0, Day 7…). For change over time or dose.",
            obj(
                common(
                    props(json!({
                        "pictures": pictures("The pictures, in slider order"),
                        "labels": {"type": "array", "items": {"type": "string"}, "description": "One label per picture (default: the file names)"}
                    })),
                    true,
                ),
                &["pictures"],
            ),
            false,
        ),
        tool(
            "add_zoom",
            "Add zoom region",
            "A box on the slide that, clicked in the show, zooms the whole slide into that part; another click zooms out. Give the region as x, y, w, h (pt) or `over`.",
            obj(common(props(json!({"max_scale": num("Largest zoom (default 2.5)")})), true), &[]),
            false,
        ),
        tool(
            "add_3d_model",
            "Add 3D model",
            "A 3D model (.glb, .obj or .stl) the presenter turns by dragging and zooms with the wheel. Each press of the shortcut takes the next layer off (the file's top-level parts).",
            obj(
                common(
                    props(json!({
                        "model": picture("The model file (path or base64)"),
                        "hidden_layers": {"type": "array", "items": {"type": "string"}, "description": "Layer names hidden when the slide opens"},
                        "link_group": string("Models on one slide with the same name turn together")
                    })),
                    false,
                ),
                &["model"],
            ),
            false,
        ),
        tool(
            "add_measure",
            "Add measurement",
            "Measuring lines over a photo (usually a face) that the presenter drags into place in the show: thirds (hairline, brow, nose base, chin), fifths, angle or ratio. Use `over` with the photo's id.",
            obj(
                common(
                    props(json!({
                        "preset": {"type": "string", "enum": ["thirds", "fifths", "angle", "ratio"], "description": "Default thirds"},
                        "color": string("Line colour #RRGGBB (default yellow)")
                    })),
                    true,
                ),
                &[],
            ),
            false,
        ),
        tool(
            "add_injection_map",
            "Add injection map",
            "An area over a photo where the presenter places dose points (a click) and arrows (a drag from the insertion point to the target) during the talk, with a running total per product. Use `over` with the photo's id. Default products: Toxin in U and Filler in mL.",
            obj(
                common(
                    props(json!({
                        "products": {"type": "array", "items": {"type": "object", "properties": {
                            "name": string("Toxin, Filler, Skin booster…"),
                            "unit": string("U, mL…"),
                            "color": string("#RRGGBB"),
                            "dose": num("Dose a new point starts with")
                        }, "required": ["name"], "additionalProperties": false}, "description": "The products the presenter picks from"},
                        "show_total": boolean("Show the totals under the map (default true)")
                    })),
                    true,
                ),
                &[],
            ),
            false,
        ),
        tool(
            "list_interactive",
            "List interactive objects",
            "The deckrt objects on a slide: id, kind, label, box and settings.",
            obj(props(json!({"slide": int("Slide index (default: current)")})), &[]),
            true,
        ),
        tool(
            "open_in_deckrt",
            "Open in the deckrt window",
            "Move the work into the open deckrt window, so the person sees the deck and can present it. A deck built headless goes across whole. Needs deckrt open with AI Agents › Let agents work in this window on.",
            obj(Map::new(), &[]),
            false,
        ),
        tool(
            "set_notes",
            "Set speaker notes",
            "Replace a slide's speaker notes (what presenter view shows the presenter, and the script the coach and captions use).",
            obj(props(json!({"text": string("The notes"), "slide": int("Slide index (default: current)")})), &["text"]),
            false,
        ),
    ]
}

pub fn handles(name: &str) -> bool {
    definitions().iter().any(|t| t.get("name").and_then(Value::as_str) == Some(name))
}

fn exec(b: &mut dyn Backend, cmd: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command": cmd, "params": params}))
}

fn f(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64).filter(|v| v.is_finite())
}

/// The slide's size in points.
fn slide_size(b: &mut dyn Backend) -> (f64, f64) {
    let v = b.call("document.inspect", json!({})).unwrap_or_default();
    let w = v["slideSize"][0].as_f64().filter(|w| *w > 0.0).unwrap_or(960.0);
    let h = v["slideSize"][1].as_f64().filter(|h| *h > 0.0).unwrap_or(540.0);
    (w, h)
}

/// The box asked for: `over` a shape, or x, y, w, h. `None` lets deckrt place it.
fn rect(b: &mut dyn Backend, a: &Value) -> Result<Option<[f64; 4]>, String> {
    if let Some(id) = a.get("over").and_then(Value::as_u64) {
        let sl = exec(b, "slide.inspect", json!({}))?;
        let shape =
            sl["shapes"].as_array().and_then(|s| s.iter().find(|s| s["id"].as_u64() == Some(id))).ok_or(format!("no shape {id} on this slide"))?;
        let bx: Vec<f64> = shape["box"].as_array().map(|v| v.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
        return match bx.as_slice() {
            [x, y, w, h] => Ok(Some([*x, *y, *w, *h])),
            _ => Err(format!("shape {id} has no box")),
        };
    }
    Ok(match (f(a, "x"), f(a, "y"), f(a, "w"), f(a, "h")) {
        (Some(x), Some(y), Some(w), Some(h)) => Some([x, y, w, h]),
        _ => None,
    })
}

/// A picture argument as an `interactive.insert` file entry.
fn file(v: &Value) -> Option<Value> {
    match v {
        Value::String(p) => Some(json!({"path": p})),
        Value::Object(o) => Some(json!({"data": o.get("data")?, "name": o.get("name").cloned().unwrap_or(json!("picture.png"))})),
        _ => None,
    }
}

fn files(a: &Value, key: &str) -> Vec<Value> {
    a.get(key).and_then(Value::as_array).map(|v| v.iter().filter_map(file).collect()).unwrap_or_default()
}

/// Insert `kind` with `config`, the common settings from `a`, `files` and the box.
fn insert(
    b: &mut dyn Backend,
    kind: &str,
    a: &Value,
    config: Value,
    files: Vec<Value>,
    default_rect: Option<[f64; 4]>,
    clickable: bool,
) -> Result<Value, String> {
    let mut object = json!({"type": kind, "config": config});
    if let Some(l) = a.get("label").and_then(Value::as_str) {
        object["label"] = json!(l);
    }
    if let Some(k) = a.get("shortcut").and_then(Value::as_str).and_then(|s| s.chars().next()) {
        object["shortcut"] = json!(k.to_string());
    }
    if clickable {
        object["trigger"] = json!(if a.get("trigger").and_then(Value::as_str) == Some("presenter") { "presenter" } else { "audience_click" });
        if let Some(n) = a.get("next_clicks").and_then(Value::as_u64) {
            object["next_clicks"] = json!(n.min(50));
        }
    }
    let mut p = json!({"kind": kind, "object": object});
    if !files.is_empty() {
        p["files"] = json!(files);
    }
    if let Some(r) = rect(b, a)?.or(default_rect) {
        p["rect"] = json!(r);
    }
    exec(b, "interactive.insert", p)
}

fn wrap(r: Result<Value, String>) -> ToolResult {
    match r {
        Ok(v) => ToolResult::json(&v),
        Err(e) => ToolResult::error(e),
    }
}

pub fn call(b: &mut dyn Backend, name: &str, a: &Value) -> ToolResult {
    wrap(run(b, name, a))
}

fn run(b: &mut dyn Backend, name: &str, a: &Value) -> Result<Value, String> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_string);
    match name {
        "add_poll" => {
            let kind = s("kind").unwrap_or_else(|| "choice".into());
            let options: Vec<String> = a
                .get("options")
                .and_then(Value::as_array)
                .map(|o| o.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            if matches!(kind.as_str(), "choice" | "ranking") && options.len() < 2 {
                return Err(format!("a {kind} poll needs two or more options"));
            }
            let correct = a.get("correct").and_then(Value::as_u64);
            if let Some(c) = correct
                && c as usize >= options.len()
            {
                return Err(format!("`correct` is {c} but there are {} options", options.len()));
            }
            let config = json!({
                "kind": kind,
                "question": s("question").unwrap_or_default(),
                "options": options,
                "correct": correct,
                "reveal_correct": correct.is_some(),
                "scale": a.get("scale").and_then(Value::as_u64).unwrap_or(5).clamp(2, 10),
            });
            let (w, h) = slide_size(b);
            let r = insert(b, "poll", a, config, vec![], Some([w * 0.125, h * 0.185, w * 0.75, h * 0.63]), false)?;
            if let Some(targets) = a.get("leads_to").and_then(Value::as_array).filter(|t| !t.is_empty()) {
                exec(b, "interactive.update", json!({"id": r["id"], "object": r["object"], "targetSlides": targets}))?;
            }
            Ok(r)
        }
        "add_popup" => {
            let (w, h) = slide_size(b);
            let content = if let Some(u) = s("url") {
                json!({"kind": "web", "url": u})
            } else {
                json!({"kind": "text", "text": s("text").unwrap_or_default()})
            };
            let panel: Vec<f64> = a.get("panel").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            let (pos, size) = match panel.as_slice() {
                [x, y, pw, ph] => ([x / w, y / h], [pw / w, ph / h]),
                _ => ([0.45, 0.3], [0.45, 0.4]),
            };
            let config = json!({"content": content, "panel_position": pos, "panel_size": size});
            let files = a.get("picture").and_then(file).into_iter().collect();
            insert(b, "popup", a, config, files, None, true)
        }
        "add_image_swap" => {
            let files = files(a, "pictures");
            if files.is_empty() {
                return Err("an image swap needs pictures".into());
            }
            let config =
                json!({"on_click": s("on_click").unwrap_or_else(|| "advance".into()), "index": a.get("start").and_then(Value::as_u64).unwrap_or(0)});
            insert(b, "image_swap", a, config, files, None, true)
        }
        "add_wipe" => {
            let files: Vec<Value> = [a.get("before"), a.get("after")].into_iter().flatten().filter_map(file).collect();
            if files.len() != 2 {
                return Err("a wipe needs a before and an after picture".into());
            }
            let config = json!({"initial_position": f(a, "position").unwrap_or(0.5).clamp(0.0, 1.0)});
            insert(b, "wipe", a, config, files, None, true)
        }
        "add_scrub" => {
            let files = files(a, "pictures");
            let r = insert(b, "scrub", a, json!({}), files, None, true)?;
            if let Some(labels) = a.get("labels").and_then(Value::as_array) {
                let mut o = r["object"].clone();
                o["config"]["labels"] = json!(labels);
                exec(b, "interactive.update", json!({"id": r["id"], "object": o}))?;
            }
            Ok(r)
        }
        "add_zoom" => {
            let Some([x, y, rw, rh]) = rect(b, a)? else { return Err("a zoom region needs its box: x, y, w, h or `over`".into()) };
            let (w, h) = slide_size(b);
            let config = json!({"source_rect": [x / w, y / h, rw / w, rh / h], "max_scale": f(a, "max_scale").unwrap_or(2.5).clamp(1.1, 10.0), "duration_ms": 300});
            insert(b, "zoom", a, config, vec![], None, true)
        }
        "add_3d_model" => {
            let model = a.get("model").and_then(file).ok_or("missing `model`")?;
            let mut config = json!({});
            if let Some(hl) = a.get("hidden_layers") {
                config["hidden_layers"] = hl.clone();
            }
            if let Some(g) = s("link_group") {
                config["link_group"] = json!(g);
            }
            insert(b, "model", a, config, vec![model], None, false)
        }
        "add_measure" => {
            let mut config = json!({"preset": s("preset").unwrap_or_else(|| "thirds".into())});
            if let Some(c) = s("color") {
                config["color"] = json!(c);
            }
            insert(b, "measure", a, config, vec![], None, true)
        }
        "add_injection_map" => {
            let mut config = json!({"show_total": a.get("show_total").and_then(Value::as_bool).unwrap_or(true)});
            if let Some(ps) = a.get("products").and_then(Value::as_array) {
                let colors = ["#4c8dff", "#ff5c8d", "#2ea05c", "#ffb049", "#b56cff", "#00b5c8"];
                let products: Vec<Value> = ps
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        json!({
                            "name": p.get("name").and_then(Value::as_str).unwrap_or("Product"),
                            "unit": p.get("unit").and_then(Value::as_str).unwrap_or("U"),
                            "color": p.get("color").and_then(Value::as_str).unwrap_or(colors[i % colors.len()]),
                            "default_dose": p.get("dose").and_then(Value::as_f64).unwrap_or(1.0),
                        })
                    })
                    .collect();
                config["products"] = json!(products);
            }
            insert(b, "injection_map", a, config, vec![], None, true)
        }
        "list_interactive" => {
            let mut p = json!({});
            if let Some(i) = a.get("slide").and_then(Value::as_u64) {
                p["slide"] = json!(i);
            }
            exec(b, "interactive.list", p)
        }
        "open_in_deckrt" => b.use_window(),
        "set_notes" => {
            let mut p = json!({"text": s("text").unwrap_or_default()});
            if let Some(i) = a.get("slide").and_then(Value::as_u64) {
                p["index"] = json!(i);
            }
            exec(b, "slide.notes", p)
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}
