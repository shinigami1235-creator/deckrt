# deckrt: state of the build, October 2026

deckrt is the PowerPoint replacement built on DeckCraft, with the nine lecture objects from the
earlier Tauri app, a live 3D renderer, a phone server and a two-display slide show. This file says
what works, how each part was checked, what could not be checked in the build container, the bugs
that are known, and how to get the program. `FORK.md` describes the code.

## How to get the app

1. Open https://github.com/shinigami1235-creator/deckrt and click **Actions**.
2. Open the newest green **Windows build** run on the `wip/fable-partial` branch. The last one
   this report was checked against is run 38996822483 (commit `8531045`).
3. Under **Artifacts** at the bottom, download **deckrt-windows-x64-installer** (the MSI) or
   **deckrt-windows-x64** (a zip with `deckrt.exe`, `deckcraft-cli.exe` and `Start deckrt.bat`).
4. Run the MSI, or unzip the zip anywhere and double-click `deckrt.exe`. SmartScreen will say the
   program is unrecognised the first time since it is not signed: More info, then Run anyway.

If no window opens, `%APPDATA%\deckrt\logs\startup.log` lists which graphics backends were tried
and `deckcraft.log` what the last one did.

## What works

| Goal | State | Checked by |
|---|---|---|
| All nine objects insert from Insert › Interactive | Works | `03-insert-tab-interactive-group-measure-pane.png` shows the group; every kind was inserted through `interactive.insert` and listed back (`interactive.list`) |
| Each object has a pane with an Advanced section | Works | `phaseB-panes-all-types-and-advanced.png`, `phaseB-editor-model-advanced.png`, `03-…-measure-pane.png` |
| Each object round-trips through .pptx and .deckcraft | Works | `crates/engine/tests/deckrt_roundtrip.rs` (4 tests), `crates/pptx/tests/deckrt_tags.rs`; a deck with all nine kinds was saved as both formats from the app and listed back with `deckcraft-cli run --in … --cmd interactive.list` |
| The ecm deck opens with frans.glb migrated and drawn as a live mesh | Works | `01-ecm-open-sidecar-dialog.png` (the import dialog), `02-ecm-slide2-frans-live-mesh-editor.png` (the mesh in the editor), `04b-two-displays-show-slide2-…png` (in the show) |
| The ecm deck's 70 slides render as upstream does | Works | `deckcraft-cli render --all --scale 1` on the plain deck (no sidecar) compared pixel by pixel with the renders of the unmodified fork in `deliver/render-before`: 70 of 70 identical |
| Every object works in the show and from presenter view | Works | swap `06`, wipe `07`, zoom `08`, popup `09`, poll `10` and `11`, scrub `12`, measure `13`, injection map `14`, model `15` (the numbers are screenshot prefixes, see below) |
| Audience screen on a second display, display picker, Swap | Works in logic, placement unverified here | The app lists two displays and chooses "audience on display 2, presenter on display 1" (app log); the picker shows both (`18-presenter-display-picker-open.png`); picking Display 1 moved the presenter window to the other display and the audience to display 1 (`19-…png`). The move to the *second* display cannot be seen under Xvfb, see below |
| Phones: QR, name, polls with live results and reveal | Works | Four scripted phones joined and voted (`10-…png`: presenter sees 1/2/1, audience sees "4 votes"); reveal marks Fibroblast (`11-…png`); the QR column on the audience screen (`17-…png`) |
| A poll's winning slide offered | Works | `21-show-poll-winning-slide-offered.png`: after the reveal the row offers "Go to slide 11" and nothing moves until it is pressed (`22-show-after-go-to-slide-11-pressed.png` is after the press); `deckcraft-interactive` `poll::branch` tests cover ties and missing slides |
| Control granted to one named phone (model, pointer, drawing) | Works | `15-show-model-phone-lee-granted-control-turned-pointed-and-drew.png`: Lee asked, the presenter pressed Grant, Lee's drags turned frans, a touch put a pointer dot and a stroke drew a zigzag on both windows; `16-phone-live-frame-….jpg` is the picture the phones download |
| Windows hotspot fallback | Compiles on Windows, unverified | `crates/phones/src/hotspot.rs` builds and its tests run in the Windows workflow; the Start hotspot button and the manual steps are in the Phones section |
| Save / Don't Save / Cancel on close with unsaved changes | Works | `20-close-prompt-save-dont-save-cancel.png` (asked through `file.close` on the control channel, the path upstream let through); Save wrote the file and closed the deck; `SlideApp::run` test in `crates/ui-egui` |
| Windows start-up that falls back across graphics backends | Works on Linux, Windows unverified | `DECKRT_SUPERVISE=1` on Linux: dx12 failed, vulkan came up, `graphics-backend.txt` remembers `vulkan`, `startup.log` lists the attempt; `apps/deckcraft` tests (36) include the child-outcome test |

Screenshots are in `deliver/screenshots/` next to the repository, not inside it. Names starting
with `step0`, `phaseB` and `phaseC` are from the earlier runs on the same code; `run3-01` to
`run3-22` are from this run's build (the table above leaves out the `run3-` prefix).

## Tests

Run in the container on `wip/fable-partial`:

| Crate | Result |
|---|---|
| deckcraft-interactive | 33 passed |
| deckcraft-model3d | 5 passed |
| deckcraft-phones | 21 + 12 passed |
| deckcraft-show-ext | 5 passed |
| deckcraft-engine | 324 passed, 1 ignored; `deckrt_roundtrip` 4 passed; `show_clicks` 3 passed |
| deckcraft-pptx | all passed (incl. `deckrt_tags`) |
| deckcraft-model | 79 passed |
| deckcraft-render | all passed |
| deckcraft-ui-egui | 213 passed, 4 ignored, 1 failed |
| deckcraft (the app) | 36 passed |

The one failure, `replacefonts_tests::ribbon_button_opens_the_dialog_and_replaces`, is upstream's
own test at a 1400-point window: the Home tab is wider than that and the Replace Fonts button is
off the right edge. The fork does not touch the Home tab. It is item 1 of the next-run list in
`FORK.md`.

`cargo clippy` with the workspace lints is clean; `cargo build --release -p deckcraft -p deckcraft-cli`
builds the `deckrt` and `deckcraft-cli` binaries.

The Windows workflow (`.github/workflows/windows-build.yml`) builds both exes, runs the tests of
the fork crates and the app on Windows, and uploads the zip and the MSI. Runs 38992403485
(`8afe20b`) and 38996822483 (`8531045`) are green with both artifacts.

## What could not be checked here

- **Real Windows.** The container cannot compile for Windows; the Windows build and its tests ran
  on GitHub Actions only. Nobody has started `deckrt.exe` on Gid's laptop yet: the DirectX 12 /
  Vulkan / OpenGL / WARP fallback, the file dialogs, the MSI, and the hotspot are untested on a
  real machine.
- **A real second display.** Under Xvfb the X server has one CRTC, so winit reports one monitor
  no matter how many `xrandr` monitors are defined, and the audience window stays on the first.
  The app's own display list, the choice of which display gets what, the picker, Swap and the
  presenter window's move were watched; the audience window landing on the projector was not.
  Screenshot `04a-…` shows what Xvfb gives (the audience window over the presenter window on
  the first display); `04b-…-audience-window-moved-by-hand.png` has the audience window dragged
  to the second half of the screen by hand so both windows are in one picture.
- **Real phones.** The phones in the screenshots are Python WebSocket clients speaking the phone
  protocol. The phone page itself (`crates/phones/phone/index.html`) was exercised in jsdom with
  `crates/phones/phone/test/page-test.mjs` in an earlier run, not in a phone browser. Venue wifi
  that isolates clients, and the hotspot, are untested.
- **The GPU renderer on NVIDIA or Intel.** In the container the model is drawn by Mesa's
  software Vulkan (llvmpipe) through the same wgpu path; the software rasteriser fallback is
  what draws posters, thumbnails and the phones' pictures.

## Known bugs and rough edges

- The upstream test above fails until the ribbon shrinks to the window (next-run item 1).
- Objects other than the model do not play in the editor; they show their fallback picture with
  a badge (next-run item 2).
- A swap or scrub picture is fitted inside its box in the show but stretched in the editor once
  the box is resized (next-run item 7).
- The presenter window uses the top part of a 1080-pixel display and leaves the bottom quarter
  empty; upstream's layout.
- On X11 Linux (not Windows) the audience screen only moves to a second display when winit sees
  it as a separate monitor; on a laptop with a real second screen it does.

## Changes in this run

- The uncommitted formatting and clippy changes left by the earlier run were kept and committed.
- `interactive.update` takes `targetSlides` so a poll's slides can be set by index from the CLI
  and the control channel.
- The close and quit prompts are one line.
- The supervisor's failure line no longer says "exit status" twice, and its info lines reach
  `startup.log` (the default log filter now includes the binary's own targets).
