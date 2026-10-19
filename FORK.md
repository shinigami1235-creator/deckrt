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
| `crates/captions/src/recorder.rs` | The microphone into memory for narration (22 050 Hz mono); `DECKRT_RECORD_TEST=<wav>` plays a file in instead |
| `crates/engine/src/cmd/deckrt_record.rs` | `deckrt.setNarration`, `deckrt.clearNarration`: each slide's recording as an audio clip tagged `DECKRT.NARRATION`, started by a Play effect |
| `crates/video-out` | Export to Video's writer: NV12 frames and 16-bit PCM into H.264 and AAC in an MP4, through Media Foundation's sink writer on Windows. Allows `unsafe` for the COM calls, like `crates/show-ext` |
| `crates/ui-egui/src/deckrt/record.rs`, `video.rs`, `crowd.rs` | The Record tab and recording during a show; the Export to Video window and its thread; reactions from phones and the feedback form |
| `crates/engine/src/cmd/deckrt_tidy.rs` | `deckrt.tidy`: Tidy This Slide, worked out on a copy of the slide and applied as one undo step |
| `crates/ui-egui/src/deckrt/check.rs`, `privacy.rs`, `captions.rs`, `questions.rs` | Their windows and buttons; Blur Area's drag; the caption band; questions from phones |
| `crates/engine/tests/deckrt_roundtrip.rs`, `crates/pptx/tests/deckrt_tags.rs` | Round trips of every object through .pptx and .deckcraft, the ecm sidecar migration, the tag media relationships |
| `crates/ui-egui/src/deckrt/format_picture.rs`, `crates/engine/src/cmd/deckrt_live.rs` | Live sliders with number boxes (picture corrections, colour, transparency, crop; shape fill transparency); a drag runs its command at every step inside one undo step |
| `crates/pptx/src/deckrt.rs` | Picture saturation in the file (`a14:saturation` on an `a14:imgLayer`) |
| `crates/ui-egui/src/deckrt/notes.rs`, `prefs.rs` | Presenter view's notes panel, dividers and Hide next slide; settings kept in `<settings>/deckrt-prefs.json` |
| `crates/ui-egui/src/deckrt/preview.rs` | Transition preview on the editor canvas, AutoPreview, gallery tiles that play their effect under the pointer |
| `crates/ui-egui/src/deckrt/insert_previews.rs` | Pictures in the Chart, Change Chart Type, WordArt and Action menus (drawn from a scratch deck with the open deck's theme) and the SmartArt dialog |
| `crates/show-ext/src/capture.rs`, `crates/ui-egui/src/deckrt/screenshot.rs` | Insert › Screenshot: window pictures (`PrintWindow`), the screen (`BitBlt`, deckrt's windows left out with `WDA_EXCLUDEFROMCAPTURE`), Screen Clipping's window; `DECKRT_SCREEN_TEST=<png>` stands in for the screen |
| `packaging/sample/`, `assets/sample/deckrt-feature-tour.pptx`, `crates/engine/src/cmd/deckrt_sample.rs` | The sample deck: drawn pictures and model, the build script, the deck embedded in the program and opened by `deckrt.openSample` |
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
| `crates/engine/src/cmd/mod.rs` (round 5) | `pub mod deckrt_record;` and its `specs()`; `deckrt.addNotes` lives in `deckrt_coach.rs` |
| `crates/pptx/src/write/shapes.rs` (round 5) | an audio or video clip's tags go in its `p:nvPr` like a picture's (upstream dropped them) |
| `crates/ui-egui/src/show.rs` (round 5) | no advancing by the slides' timings while recording or with the coach (`deckrt::record::active`); narration doesn't start by itself while recording or with Play Narrations off (`deckrt::record::may_play`, in `show_media` and `run_media_cues`) |
| `crates/ui-egui/src/ribbon.rs` (round 5) | the Record tab is deckrt's (`deckrt::record::ribbon`); the Slide Show tab's Record button is `deckrt::record::slide_show_button` |
| `.github/workflows/windows-build.yml` (round 5) | also runs `deckcraft-video-out`'s tests and `deckcraft-ui-egui`'s `deckrt::video` tests, which make real MP4s; `video-check.yml` runs just the video crate on pushes to `wip/video-check` |
| `crates/engine/src/cmd/mod.rs` (round 6) | `pub mod deckrt_tidy;` and its `specs()` |
| `crates/ui-egui/src/widgets.rs` (round 6) | `group` fits the ribbon: groups past the window's edge go into a More menu at the right end (`fit_begin`, `fit_end`), each in its own card; a hidden group is drawn off screen and invisible so its caller still gets its result |
| `crates/ui-egui/src/ribbon.rs` (round 6) | `fit_begin` and `fit_end` around the tab's groups; Tidy Slide after Arrange on the Home tab |
| `crates/ui-egui/src/theme.rs` (round 6) | `field_border`: text fields, check boxes, radio buttons and framed buttons keep a thin border like Office's |
| `crates/ui-egui/src/canvas.rs` (round 6) | the object's own entries first in the right-click menu (`deckrt::context_items`) |
| `crates/ui-egui/src/show.rs` (round 6) | every Next (keys, a click on the slide, a swipe, the Next buttons) asks `deckrt::show::take_next` first, so an object in the click order runs before the show moves on |
| `crates/engine/src/cmd/mod.rs` (round 8) | `pub mod deckrt_live; pub mod deckrt_sample;` and the sample's `specs()` |
| `crates/pptx/src/lib.rs`, `read/dml.rs`, `write/dml.rs` (round 8) | `mod deckrt;`; a picture's saturation is read from and written to its image layer (upstream lost it on save) |
| `crates/ui-egui/src/panes.rs` (round 8) | Format Shape › Picture is `deckrt::format_picture::pane` |
| `crates/ui-egui/src/ribbon.rs` (round 8) | Picture Format's Corrections, Color and Transparency menus; the Shape Fill menus' transparency slider; Transitions › Preview plays on the canvas with AutoPreview under its arrow; Animations › Preview's arrow; picking an effect calls `deckrt::preview::auto_*`; `paint_transition_tile` and `paint_anim_tile` let a hovered tile play; Insert's Screenshot, Chart, WordArt and Action menus and Change Chart Type are deckrt's |
| `crates/ui-egui/src/dialogs.rs` (round 8) | the SmartArt dialog picks its layout from pictures |
| `crates/ui-egui/src/canvas.rs` (round 8) | no selection handles over a transition preview |
| `crates/ui-egui/src/show.rs` (round 8) | presenter view's columns come from `deckrt::notes::columns` and its right column from `deckrt::notes::right_column`; `notes_px` is `pub(crate)` and `NOTES_MAX` is 72; `state_for`, `quad_mesh` and `part_textures` are `pub(crate)` for the canvas preview; `next_rect` is kept for its test |
| `crates/ui-egui/src/lib.rs` (round 8) | `deckrt::screenshot::frame` each frame for Screen Clipping's window |
| `crates/ui-egui/src/backstage.rs` (round 8) | the sample deck's tile on File › New |
| `crates/ui-egui/src/ribbon.rs`, `menus.rs` (round 8) | Present in Presenter View after Presenter View on the Slide Show tab, and Alt+F5 for it (`deckrt::show::present_in_presenter_view`) |
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

- Narration is kept the way PowerPoint keeps it: an audio clip per slide in the bottom-right,
  hidden in the show, with a Play effect first in the slide's animations, and the slide's
  Advance After set to the recording's length. The clip is a 16-bit mono WAV at 22 050 Hz
  (about 2.6 MB a minute) since deckrt has no audio encoder of its own. Slides passed in under
  half a second keep what they had. Pause stops the recording and the slide's clock together.
- Export to Video holds each slide for its narration, else its Advance After time, else the
  seconds asked for; hidden slides are left out; a transition becomes a fade of up to 2 s.
  Animations show finished and videos on slides show their poster frame. The video runs at
  10 frames a second with the encoder's quality setting (75) instead of a fixed bitrate, so a
  still slide costs almost nothing after its first frame; sound is AAC at 96 kbit/s, mono.
  Media Foundation is used because it ships with Windows (hardware encoding where there is
  some) and nothing permissive and pure-Rust encodes AAC.
- Reactions: four, drawn by deckrt (heart, star, `!`, `?`) rather than emoji, one per phone
  every 1.2 s, at most 40 in the air. The feedback form opens on the phones when the
  presenter asks, and by itself once when the show reaches its end. Both go in
  `<deck>.results.json`.
- The handout is printed by the phone's own browser (Save as PDF), so Thai notes come out in
  the phone's fonts; the deck's slide pictures come from the same phone-sized JPEGs as the
  Slides tab.

- Tidy This Slide only moves shapes placed on the slide; placeholders keep their layout's
  position. Shapes count as a row when their middles are within 18 pt and their heights within
  20 %; such a row gets one size (the median), one top and even gaps, and a column the same
  sideways. A text box up to 60 pt under a shape becomes its caption. Text under 12 pt comes up
  to its fellow captions' size, or 12 pt. A text box whose text runs over grows when there is
  room below, else its text is set to shrink.
- Objects in the click order: `next_clicks` on an object is how many presses of Next it takes
  after the slide's own animations, before the show moves on. An object can also run by itself:
  `with_slide` (as the slide opens, like With Previous on the first effect) or
  `after_animations` (once the slide's animations have played, like After Previous), each after
  `delay` seconds, once per visit to the slide. All are in the object's JSON, so PowerPoint
  keeps them and ignores them.
- Touch in the show: two fingers on the slide pinch it in (from where they started) and back
  out, and drag the zoomed view, kept inside the slide (`Zoom::touch`, a hook in
  `crates/ui-egui/src/show.rs`). Over a 3D model the pinch zooms the model instead.
- Double-clicking an object in the editor runs the slide's objects in place, through the show's
  own drawing and pointer code; Esc, a click off the slide or another slide stops them.
- With one display the show opens alone, as PowerPoint does, except when recording or
  rehearsing with the coach, which need presenter view's controls.
- The poll's box is the theme's second dark colour (`dk2`) with `lt1` text, in the file and in
  the show.
- Versions: deckrt numbers its own versions from 0.1.0 (the workspace version was upstream's
  0.7.0 until then). A push to main whose Cargo.toml version has no release yet makes the
  numbered release `deckrt-v<version>` (DeckCraft's own tags are `v…`), marked Latest, with
  the installer under its own name and as `deckrt-windows-x64.msi`, and the notes from that
  version's CHANGELOG.md section. The build makes the tag since this session's git connection
  can't push tags. Pushes to main make the `latest-build` pre-release,
  which `Update deckrt.bat` installs; its Help › About line carries the commit and date. The
  MSI's MajorUpgrade allows downgrades, so the two can be installed over each other.

- Live sliders: a drag starts an engine interaction named `deckrt-live:<label>` (the canvas's
  drags use the same mechanism), runs the command at every step and ends it on release, so the
  slide changes while dragging and Undo takes the whole drag back. A drag whose slider disappears
  is ended at the end of the frame.
- Saturation is written as PowerPoint 2010 writes it, an `a14:saturation` effect on an
  `a14:imgLayer`, but over the same picture the blip shows (deckrt applies it when drawing).
  PowerPoint shows such a picture unsaturated until it is edited there. A layer that is a
  different image (PowerPoint bakes the effect into the blip) isn't read back, so it isn't applied
  twice.
- Transitions › Preview used to open the slide show; it plays on the canvas now, from the slide
  before (black before the first), with the same layers and textures as the show, morph
  included. Picking a transition or an animation previews it while AutoPreview is on (the
  default, as in PowerPoint); an animation preview plays only the selected shapes' effects.
- Gallery tiles under the pointer play their effect: transitions between a grey and a blue card,
  animations on a small shape run through the real timeline.
- Screen Clipping leaves deckrt's own windows out of the copy with `SetWindowDisplayAffinity`
  instead of hiding the main window, since a hidden or minimised window stops getting frames.
  On Windows before 10 version 2004 deckrt shows in the copy. The clipping window covers the
  display deckrt is on.
- Present in Presenter View (Alt+F5, as in PowerPoint) keeps presenter view on one screen: it
  covers the display and the show stays in the editor's window behind it instead of going full
  screen over it.
- The sample deck in the program is built only from pictures and a model drawn in code, so it
  can ship to anyone. Gid's own sample deck (with figures from his lectures) stays out of the
  repository.

## PowerPoint feel: next run

Gaps that would keep the app from feeling like PowerPoint, ranked, with a rough size. Sizes:
small is an afternoon, medium a day or two, large a week.

1. Done (round 6, a More menu rather than PowerPoint's collapsed groups). **The ribbon does not shrink to the window.** The Home tab needs about 1600 points; on a
   1920-pixel laptop at 125% scaling (1536 points) its right end (Find, Replace, Replace Fonts,
   Selection Pane, Design Ideas) is cut off. PowerPoint collapses groups into drop-downs. The
   upstream test `replacefonts_tests::ribbon_button_opens_the_dialog_and_replaces` fails at its
   1400-point window for this reason alone. Large.
2. Done (round 6). **Objects do not play in the editor.** A wipe, scrub, swap, popup and poll show their fallback
   picture with a badge; only the model is live. PowerPoint plays media in place on a click, and
   the same would do here (a double-click runs the object in the editor). Medium.
3. Done (round 6). **The pane looks like egui, not Office.** Text fields and buttons have no frame until hovered,
   an unticked box is invisible, and the pane's own Delete is a bare word. Upstream's theme does
   this to every pane; a widget style with frames would change the whole app at once. Medium.
4. **A PowerPoint model gets no 3D Model tab.** PowerPoint shows a tab with views, pan and zoom
   and reset when a model is selected; here the settings are in the pane and the view is turned
   by the round handle. Medium.
5. Done (round 6 and 7: the click order, with the slide, after the animations, delays). **Objects are outside the animation system.** They cannot be triggered by an animation or a
   click sequence, only by the presenter, a key or a click on them. Medium.
6. **The model is turned by a handle in the editor, not by dragging it.** PowerPoint drags the
   model itself and moves it by its frame. A direct drag with the move on the frame would match.
   Small.
7. **Editor and show disagree on a resized picture.** A swap or scrub picture is fitted inside
   its box in the show but stretched to it in the editor once the box has been resized. Small.
8. Done (round 6). **No context menu entries.** Right-clicking an object gives the shape menu only; PowerPoint
   would offer the object's own actions (Reset, Replace file, Settings). Small.
9. Done (round 6). **The fallback shapes use fixed colours.** A poll's box is dark blue and a popup's trigger
   accent 1 whatever the theme; PowerPoint content follows the theme. Small.
10. Done (round 6). **Presenter view opens on one display too.** With one display the presenter window opens on
    top of the full-screen show. PowerPoint shows only the slide show there unless asked. Small.
11. Done (round 6 and 7: pinch zooms a model; two fingers pinch and pan the slide). **The show's objects take no touch gestures.** A pinch does not zoom a model and a two-finger
    drag does not pan a zoom region; PowerPoint's show takes touch. Medium.
12. **No Office file association from the zip.** Only the MSI registers deckrt under Open with.
    Small.
13. **No thumbnails for a model in the Selection Pane** and no object icons there. Small.
14. **The Interactive group has no caption**, like the other groups upstream; PowerPoint names
    its groups. Small (every group).
