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
| `crates/engine/src/cmd/interactive.rs` | The engine commands: insert, update (with `targetSlides` to name a poll's slides by index), add/remove picture, replace model, delete, list, import sidecar, refresh poster. All undoable. |
| `crates/model/src/tag_media.rs` | `"media:<id>"` tokens inside `DECKRT.*` tag values, so the PPTX writer and the media walker can find pictures named in the JSON |
| `crates/ui-egui/src/deckrt/` | The editor side: ribbon group (`mod.rs`), settings pane (`pane.rs`), canvas drawing (`canvas.rs`), the live model view (`model_view.rs`, `gpu.rs`), the dialogs; and the show side: `show.rs` (object states, drawing over the slide, pointer and keys, presenter rows, displays), `phones.rs` (the phones during a show, the QR, the Phones section), `pictures.rs` (media as textures) |
| `crates/model3d/src/convert.rs` | `.obj` (with its `.mtl` and textures) and `.stl` to `.glb` on insert and Replace Model; `.fbx` is refused with a pointer to Blender's glTF export |
| `crates/captions` | Live captions: model files and their download (curl), settings, phrase cutting, and with the `engine` feature the microphone (cpal) and recogniser (whisper.cpp through whisper-rs) threads |
| `crates/engine/src/cmd/deckrt_check.rs` | `deckrt.check`: the check before a talk (media, fonts, overflow, off-slide shapes, objects with missing files, 4:3, file size), each finding with its fix as commands |
| `crates/engine/src/cmd/deckrt_privacy.rs` | `deckrt.privacy` (the check before sharing), `deckrt.applyCrops`, `deckrt.stripMetadata`, `deckrt.removeNotes`, `deckrt.blurArea` |
| `crates/captions/src/coach.rs` | Rehearse with Coach's analysis: phrases laid against the slides shown, filler words (English and Thai), pauses, pace, slides read out |
| `crates/engine/src/cmd/deckrt_coach.rs` | `deckrt.keepPace`: each slide's rehearsed seconds in the slide tag `DECKRT.PACE` |
| `crates/ui-egui/src/deckrt/coach.rs`, `finder.rs` | Rehearse with Coach (the listening, the report, the pace line in presenter view); Find a Slide (the deck index, thumbnails, search, insert) |
| `crates/ui-egui/src/deckrt/check.rs`, `privacy.rs`, `captions.rs`, `questions.rs` | Their windows and buttons; Blur Area's drag; the caption band; questions from phones |
| `crates/engine/tests/deckrt_roundtrip.rs`, `crates/pptx/tests/deckrt_tags.rs` | Round trips of every object through .pptx and .deckcraft, the ecm sidecar migration, the tag media relationships |
| `crates/phones/phone/test/page-test.mjs` | Runs the phone page inside jsdom against a running show (join, vote, ask for control); needs Node and `npm install jsdom ws`. Not part of `cargo test` |

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
| `crates/model/src/trendline.rs` | one range check written the way the current clippy asks (`cargo clippy -D warnings` failed on upstream's line with Rust 1.97) |
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
| `crates/ui-egui/src/dialogs.rs` | `deckrt.*` dialog ids go to `deckrt::dialog`; the close dialog's own `file.close` carries `{"discard": true}`; the close and quit prompts are one line ("Save the changes to “talk”?"); titles and About say deckrt; the logo is the fork's own drawing |
| `crates/ui-egui/src/backstage.rs`, `menus.rs` | names and links |
| `crates/ui-egui/src/show.rs` | `Show.ext` (deckrt's state for the show); `zoom`, `blank`, `pen` are `pub(crate)`; `Zoom::region` and `zoom_uv` for the zoom region object; `render_frame` skips the shapes the show draws live (`hidden_in_show`); the shortcut hook at the top of `keyboard`; in `ui`: `place_show` at the start, `logic` each frame, `audience_avail` and `draw_qr` for the QR column, `audience_pointer` every frame before the click is read (a drag is many frames with no click), `took_scroll` before the page flick, `draw_audience` after the slide picture, `end_frame`, `end_show` and `restore_window` at the end; `presenter_window` takes `&mut app.deckrt`, uses `presenter_builder`, `move_presenter_if_asked`, `presenter_toolbar`, `presenter_slide` (objects and rows under the current slide), `presenter_phones` (between the next slide and the notes), and follows a poll's winning slide (`go_to`) |
| `apps/deckcraft/src/main.rs` | the supervisor runs first on Windows (`supervise::wanted`) and logs to `logs/startup.log`; `App` marks the first frame (`supervise::mark_ready`); `--no-supervise`; `list_displays` is `deckcraft_show_ext::displays::list` |
| `apps/deckcraft/src/logging.rs` | `AppLogger::attach_file`: an appended log file with no rotation, for the supervisor; `DEFAULT_FILTER` lets the binary's own `deckrt*` info lines through (the supervisor's attempts) |
| `apps/deckcraft/src/graphics.rs` | the software adapter selector for the supervisor's last attempt (`DECKRT_SOFTWARE_GPU`) |
| `apps/deckcraft/Cargo.toml`, `build.rs`, `src/main.rs` | the binary is `deckrt`; Windows resources; `pick_open_many` and the `model` purpose; the wgpu device is handed to `deckrt::gpu::Gpu` |
| `apps/deckcraft-cli` | the `links` subcommand is gone |
| `assets/app-icon/*`, `docs/brand/*` | the fork's own icon; the ArtCraft marks are removed |
| `crates/engine/src/cmd/mod.rs` (round 3) | `pub mod deckrt_check; pub mod deckrt_privacy;` and their `specs()` |
| `crates/ui-egui/src/lib.rs` (round 3) | `start_show` calls `deckrt::check::gate` first: the check opens instead of the show when it finds a problem |
| `crates/ui-egui/src/ribbon.rs` (round 3) | Check Deck and Live Captions on the Slide Show tab; Check Before Sharing and Blur Area on the Review tab |
| `crates/engine/src/cmd/mod.rs` (round 4) | `pub mod deckrt_coach;` and its `specs()` |
| `crates/ui-egui/src/show.rs` (round 4) | `slide_start` and `show_start` are `pub(crate)` for the coach and the pace line |
| `crates/ui-egui/src/ribbon.rs` (round 4) | Rehearse with Coach after Rehearse Timings on the Slide Show tab; Find a Slide after New Slide on the Home tab |
| `apps/deckcraft/src/main.rs` (round 4) | `pick_open("folder")` opens a folder picker, for Find a Slide's Add folder |
| `apps/deckcraft/src/main.rs` (round 3) | `app.deckrt.settings_dir` so captions keep their settings and model files in the settings folder |
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
- The audience screen is moved with `ViewportCommand::SetMonitor(i)`, where `i` indexes winit's
  `available_monitors()`. On Windows both that list and `crates/show-ext` come from
  `EnumDisplayMonitors`, so the indices agree. Under Xvfb winit sees one monitor whatever
  `xrandr --setmonitor` says (Xvfb has one CRTC), so the second-display placement cannot be
  watched in the container; the display picker, Swap and the presenter window's move can (see
  FORK_REPORT.md).
- `interactive.update` takes `targetSlides: [index | null]` for a poll, so the CLI, the control
  channel and tests can set the winning slides without knowing the `DECKRT.SLIDE` keys.

- The check before a talk runs on Present only when the deck changed since it last passed, and
  only stops the show for problems; notes (text past its box, 4:3) don't stop it.
- Captions run on the laptop, never online. The model (base 150 MB or small 490 MB from
  Hugging Face's whisper.cpp files) is fetched once with Windows' own `curl.exe` into
  `<settings>/speech/`. The recogniser gets the slide's words as its prompt so terms like
  "fibrillin" come out right. `DECKRT_CAPTIONS_TEST=<file>` plays lines from a text file instead
  of the microphone, which is how the container tests the band, presenter controls and phones.
- Blur Area pixelates then blurs the dragged part and saves the picture as new media, so the
  original pixels leave the file on save. Undo brings them back until then.
- Strip Photo Details removes JPEG APP1/APP13/COM and PNG text/eXIf blocks byte by byte, without
  re-encoding, so the picture itself is unchanged.
- Patient identifiers the sharing check looks for: HN, MRN, DOB labels and 13-digit IDs, in slide
  text and notes. It points at them; it doesn't change text on its own.
- Questions from phones: five waiting per phone, 300 per show, same question twice from one
  phone refused. The presenter shows one at a time on the screen, above the caption band.

- Rehearse with Coach uses the captions' speech model and microphone; captions stay off while
  it listens. Its prompt starts with filler words so whisper writes the ums down instead of
  tidying them away. Phrases carry their times (`Event::Line { text, start, end }`), counted
  from when the microphone opened, so each lands on the slide that was up when it was said.
  Pace isn't counted for Thai, which has no spaces between words. "Read out" means half or more
  of the slide's runs of four words (eight letters in Thai) turned up in what was said.
- The coach doesn't write auto-advance timings the way Rehearse Timings does. Keep this pace
  stores seconds per slide in `DECKRT.PACE`; presenter view's line compares the time since the
  show began with the kept seconds of the slides before this one plus this one's so far, and
  says on pace within 20 s.
- Find a Slide keeps its index in `<settings>/slide-index.json` and a 320 px JPEG per slide in
  `<settings>/slide-thumbs/<hash>/`. A deck is read again only when its size or time changes.
  It skips decks over 400 MB, Office lock files (`~$`), OneDrive files that are only in the
  cloud (reading them would download them), and folders like `node_modules` and `AppData`.
  The open deck is left out of the results. Inserting goes through `slide.reuse`, so objects
  and their pictures come along.

## PowerPoint feel: next run

Gaps that would keep the app from feeling like PowerPoint, ranked, with a rough size. Sizes:
small is an afternoon, medium a day or two, large a week.

1. **The ribbon does not shrink to the window.** The Home tab needs about 1600 points; on a
   1920-pixel laptop at 125% scaling (1536 points) its right end (Find, Replace, Replace Fonts,
   Selection Pane, Design Ideas) is cut off. PowerPoint collapses groups into drop-downs. The
   upstream test `replacefonts_tests::ribbon_button_opens_the_dialog_and_replaces` fails at its
   1400-point window for this reason alone. Large.
2. **Objects do not play in the editor.** A wipe, scrub, swap, popup and poll show their fallback
   picture with a badge; only the model is live. PowerPoint plays media in place on a click, and
   the same would do here (a double-click runs the object in the editor). Medium.
3. **The pane looks like egui, not Office.** Text fields and buttons have no frame until hovered,
   an unticked box is invisible, and the pane's own Delete is a bare word. Upstream's theme does
   this to every pane; a widget style with frames would change the whole app at once. Medium.
4. **A PowerPoint model gets no 3D Model tab.** PowerPoint shows a tab with views, pan and zoom
   and reset when a model is selected; here the settings are in the pane and the view is turned
   by the round handle. Medium.
5. **Objects are outside the animation system.** They cannot be triggered by an animation or a
   click sequence, only by the presenter, a key or a click on them. Medium.
6. **The model is turned by a handle in the editor, not by dragging it.** PowerPoint drags the
   model itself and moves it by its frame. A direct drag with the move on the frame would match.
   Small.
7. **Editor and show disagree on a resized picture.** A swap or scrub picture is fitted inside
   its box in the show but stretched to it in the editor once the box has been resized. Small.
8. **No context menu entries.** Right-clicking an object gives the shape menu only; PowerPoint
   would offer the object's own actions (Reset, Replace file, Settings). Small.
9. **The fallback shapes use fixed colours.** A poll's box is dark blue and a popup's trigger
   accent 1 whatever the theme; PowerPoint content follows the theme. Small.
10. **Presenter view opens on one display too.** With one display the presenter window opens on
    top of the full-screen show. PowerPoint shows only the slide show there unless asked. Small.
11. **The show's objects take no touch gestures.** A pinch does not zoom a model and a two-finger
    drag does not pan a zoom region; PowerPoint's show takes touch. Medium.
12. **No Office file association from the zip.** Only the MSI registers deckrt under Open with.
    Small.
13. **No thumbnails for a model in the Selection Pane** and no object icons there. Small.
14. **The Interactive group has no caption**, like the other groups upstream; PowerPoint names
    its groups. Small (every group).
