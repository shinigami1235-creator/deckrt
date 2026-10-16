# deckrt: notes for whoever works on this fork

deckrt is a fork of DeckCraft (https://github.com/storytold/slidecraft, MIT OR Apache-2.0). It adds
nine interactive objects for lectures, a live 3D model renderer, a phone server for polls and
model control, and a second-display slide show. Everything the fork adds lives in its own crates;
upstream files carry small marked hooks so `git merge upstream/main` stays cheap.

Upstream's `CLAUDE.md` and `AGENTS.md` are written for DeckCraft contributors. Two parts of them
bind this fork too: the asset policy (no Microsoft or Adobe assets, only provably open licences)
and the never-crash rule. The rest (pushing to origin main, `../../craftrules`, keeping ROADMAP.md
current) does not apply here.

## Where the fork's code is

| Crate or folder | What it holds |
|---|---|
| `crates/interactive` | The nine object types, their JSON tag codec (`DECKRT.OBJECT`), show state, the measure and injection maths, poll tallies, `.glb` node names, and the import of the old `.deckrt` sidecars |
| `crates/model3d` | `.glb` loading, the orbit camera, a software rasteriser (posters, thumbnails, no-GPU fallback) and the wgpu renderer (`gpu` feature) |
| `crates/phones` | The phone server: HTTP + WebSocket, the phone page (`phone/index.html`), LAN addresses, the Windows hotspot, the results file |
| `crates/show-ext` | The connected displays (`EnumDisplayMonitors` on Windows, `xrandr` on X11) and which one the audience screen and the presenter window go to |
| `apps/deckcraft/src/supervise.rs` | The Windows start-up supervisor: one child per graphics backend until a window comes up (see Phase D) |
| `crates/engine/src/cmd/interactive.rs` | The engine commands: insert, update, add/remove picture, replace model, delete, list, import sidecar, refresh poster. All undoable. |
| `crates/model/src/tag_media.rs` | `"media:<id>"` tokens inside `DECKRT.*` tag values, so the PPTX writer and the media walker can find pictures named in the JSON |
| `crates/ui-egui/src/deckrt/` | The editor side: ribbon group (`mod.rs`), settings pane (`pane.rs`), canvas drawing (`canvas.rs`), the live model view (`model_view.rs`, `gpu.rs`), the dialogs; and the show side: `show.rs` (object states, drawing over the slide, pointer and keys, presenter rows, displays), `phones.rs` (the phones during a show, the QR, the Phones section), `pictures.rs` (media as textures) |
| `crates/engine/tests/deckrt_roundtrip.rs`, `crates/pptx/tests/deckrt_tags.rs` | Round trips of every object through .pptx and .deckcraft, the ecm sidecar migration, the tag media relationships |

## How objects are stored

- Each object is an ordinary shape on its slide. Its settings are JSON in the shape tag
  `DECKRT.OBJECT` (`{"v":1,"type":"poll","config":{...},"label":...,"trigger":...,"shortcut":...,"visible":...,"anchor":...}`).
  Tag names are matched without regard to case because PowerPoint's own tag API upper-cases them.
- Pictures and models the settings name are media items of the presentation, written as
  `"media:<id>"`. On save each becomes a relationship of the tags part (`ppt/tags/_rels/tagN.xml.rels`),
  so the parts travel with the file; on open they become media ids again.
- The fallback other apps see: a swap and a scrub show their first picture (the shape is that
  picture), a wipe its before picture, a poll a dark box with the question and lettered answers,
  a popup a rounded box with its label, a zoom region, a measurement and an injection map an
  invisible rectangle.
- A 3D model is PowerPoint's own 3-D model (`ShapeKind::Model3d { media, poster }`): the `.glb`
  as the am3d part, a poster PNG drawn by the software rasteriser as the fallback picture. Layers,
  initial rotation, camera, trigger and shortcut are in the tag.
- Poll targets name slides by a durable key in the slide tag `DECKRT.SLIDE`, so reordering keeps
  them.
- `.deckcraft` keeps tags and media as they are, so nothing extra is needed there.

## Sidecar migration

Opening `talk.pptx` with `talk.deckrt` next to it imports the sidecar's objects once
(`cmd::interactive::after_open`, called from `file.open`). Each object is matched by the file's
slide id, then by shape id, then by the old fingerprint score (0.7 or better), else placed at the
sidecar's recorded position. Assets are read from `assets/` beside the sidecar and added to the
deck's media. The deck is marked with the presentation tag `DECKRT.SIDECAR` so a saved deck is not
imported again, the undo history is cleared, and the user sees one dialog saying what came over.
The sidecar file is not written to.

## Hooks into upstream files

Every change to an upstream file, with the reason. Search for `deckrt` in the file to find it.

| File | Hook |
|---|---|
| `Cargo.toml` | The three fork crates in `[workspace.dependencies]` |
| `crates/engine/Cargo.toml` | depends on `deckcraft-interactive`, `deckcraft-model3d` |
| `crates/engine/src/cmd/mod.rs` | `pub mod interactive;` and `interactive::specs()` in the command list |
| `crates/engine/src/cmd/file.rs` | `interactive::after_open(s)` after a file opens (sidecar migration); two messages say deckrt instead of DeckCraft |
| `crates/engine/src/cmd/format.rs` | one message says deckrt |
| `crates/engine/src/links.rs` | `APP_NAME`, `UPSTREAM`; the ArtCraft community links are gone |
| `crates/engine/src/media_refs.rs` | the media walker also visits `"media:<id>"` tokens in `DECKRT.*` tag values (so they are counted as used, remapped on paste, and not dropped on save) |
| `crates/model/src/lib.rs` | `pub mod tag_media;` |
| `crates/pptx/src/lib.rs`, `read/mod.rs` | `file_slide_ids()` (the `p:sldId` ids the sidecars name); tag reading turns `"rel:rIdN"` tokens back into `"media:<id>"` |
| `crates/pptx/src/write/mod.rs` | tag writing turns `"media:<id>"` tokens into relationships of the tags part |
| `crates/pptx/src/write/shapes.rs` | a 3-D model's `p:nvPr` carries its tags like a picture's |
| `crates/render/src/lib.rs` | a `Model3d` shape draws its poster picture (upstream drew nothing) |
| `crates/mcp/src/server.rs` | server name and links |
| `crates/ui-egui/Cargo.toml` | depends on the fork crates; `gpu3d` feature |
| `crates/ui-egui/src/lib.rs` | `pub mod deckrt;`, `Services::pick_open_many`, `SlideApp::deckrt` state, `deckrt::logic` and `deckrt::end_frame` calls in the frame, `drain_requests` is `pub(crate)`; `SlideApp::run` sends `file.close` through the Save / Don't Save / Cancel dialog when the deck has unsaved changes (upstream #60: the palette and the control channel bypassed it); messages say deckrt; `app.links` is gone |
| `crates/ui-egui/src/canvas.rs` | the slide texture leaves out live models (`textures.hide`), `deckrt::canvas::draw` after the slide image, `deckrt::canvas::pointer` before the slide's pointer handling |
| `crates/ui-egui/src/textures.rs` | `hide: Vec<ShapeId>` passed as `RenderOpts::skip` and part of the cache key |
| `crates/ui-egui/src/ribbon.rs` | `deckrt::ribbon_group` on the Insert tab; window title uses `APP_NAME`; the Learn More button is gone |
| `crates/ui-egui/src/panes.rs` | the `interactive` pane |
| `crates/ui-egui/src/dialogs.rs` | `deckrt.*` dialog ids go to `deckrt::dialog`; the close dialog's own `file.close` carries `{"discard": true}`; titles and About say deckrt; the logo is the fork's own drawing |
| `crates/ui-egui/src/backstage.rs`, `menus.rs` | names and links |
| `crates/ui-egui/src/show.rs` | `Show.ext` (deckrt's state for the show); `zoom`, `blank`, `pen` are `pub(crate)`; `Zoom::region` and `zoom_uv` for the zoom region object; `render_frame` skips the shapes the show draws live (`hidden_in_show`); the shortcut hook at the top of `keyboard`; in `ui`: `place_show` at the start, `logic` each frame, `audience_avail` and `draw_qr` for the QR column, `audience_pointer` every frame before the click is read (a drag is many frames with no click), `took_scroll` before the page flick, `draw_audience` after the slide picture, `end_frame`, `end_show` and `restore_window` at the end; `presenter_window` takes `&mut app.deckrt`, uses `presenter_builder`, `move_presenter_if_asked`, `presenter_toolbar`, `presenter_slide` (objects and rows under the current slide), `presenter_phones` (between the next slide and the notes), and follows a poll's winning slide (`go_to`) |
| `apps/deckcraft/src/main.rs` | the supervisor runs first on Windows (`supervise::wanted`) and logs to `logs/startup.log`; `App` marks the first frame (`supervise::mark_ready`); `--no-supervise`; `list_displays` is `deckcraft_show_ext::displays::list` |
| `apps/deckcraft/src/logging.rs` | `AppLogger::attach_file`: an appended log file with no rotation, for the supervisor |
| `apps/deckcraft/src/graphics.rs` | the software adapter selector for the supervisor's last attempt (`DECKRT_SOFTWARE_GPU`) |
| `apps/deckcraft/Cargo.toml`, `build.rs`, `src/main.rs` | the binary is `deckrt`; Windows resources; `pick_open_many` and the `model` purpose; the wgpu device is handed to `deckrt::gpu::Gpu` |
| `apps/deckcraft-cli` | the `links` subcommand is gone |
| `assets/app-icon/*`, `docs/brand/*` | the fork's own icon; the ArtCraft marks are removed |
| `.github/workflows/*` | upstream's workflows are parked in `.github/upstream-workflows-disabled/` |

## Decisions made during the build

- The tag is written as `DECKRT.OBJECT` (upper case, the way PowerPoint's Tags API stores names)
  and read without regard to case.
- A migrated sidecar clears the undo history: the objects are part of the opened deck, not a
  string of edits. The deck stays unsaved so a save keeps them.
- `Model3d` shapes draw their poster in the CPU renderer. That changes nothing on decks without
  models (checked: the ecm deck renders all 70 slides pixel-identical to the unmodified fork).
- A PowerPoint 3D model with no tag is drawn and turned like a deckrt model (`object::read_any`
  gives it the default settings; the engine fills its layers from the `.glb`). The first change
  made in the pane puts the tag on it.
- A model's `initial_rotation` is model-viewer's `orientation`, `[roll, pitch, yaw]` about the
  model's Z, X and Y axes, because that is what the earlier deckrt wrote into its sidecars. The
  pane calls them Tilt, Nod and Turn.
- A model's shortcut key peels it: each press hides the next layer in file order; after the last
  one, every layer shows again. A model with no layers goes back to its starting view.
- The audience screen and the presenter window share the object states but each reads its own
  pointer, so a drag belongs to the window it started in (`Ext::drag_role`); the other window's
  frames leave it alone. The first run lost every drag this way.
- Phones with control can draw on the slide (the phone page's Draw and Erase). The strokes live
  in `Ext::ink` for the current slide only, in the phone's own colour, drawn on both windows and
  into the live frame, and the presenter can clear them.
- On Windows the supervisor logs to `logs/startup.log` (appended, never rotated) because each
  child rotates `deckcraft.log` and four attempts would push the parent's lines out.
- `crates/show-ext` allows `unsafe` (the one crate in the fork that does) for
  `EnumDisplayMonitors`; the workspace lint forbids it everywhere else.

## PowerPoint feel: next run

Gaps that would keep the app from feeling like PowerPoint, ranked, with a rough size.

(filled in at the end of the run)
