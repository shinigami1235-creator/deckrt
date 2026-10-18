# deckrt

deckrt is a desktop presentation program that opens and saves PowerPoint files, edits them the way
PowerPoint does, and adds interactive objects PowerPoint does not have: a turnable 3D model with
layers, an image swap, a before-and-after wipe, a zoom region, a popup, a poll the audience answers
from their phones, a scrub slider, a measurement overlay and an injection map. Everything is saved
inside the .pptx, so one file holds the talk.

`deckrt` is a placeholder name until one is chosen.

## Download

deckrt runs on Windows 10 and 11, 64-bit.

1. Open the [newest version](https://github.com/shinigami1235-creator/deckrt/releases/latest) and
   click **deckrt-windows-x64.msi** under Assets to download the installer (about 52 MB).
2. Run it. Windows says "Windows protected your PC" since the installer isn't signed, so click
   **More info**, then **Run anyway**.
3. Open deckrt from the desktop or the Start menu.

A newer version installs over the old one the same way. [CHANGELOG.md](CHANGELOG.md) lists what
each version added.

The [test build](https://github.com/shinigami1235-creator/deckrt/releases/tag/latest-build) has
the newest work before it becomes a version. It changes every time the code does and may have
things that are half done.

## What deckrt adds

- Interactive objects on the slide: a 3D model, an image swap, a before-and-after wipe, a zoom
  region, a popup, a poll, a scrub slider, a measurement overlay and an injection map.
- Phones in the audience: polls, questions, reactions, a feedback form and a handout as a PDF.
- Live Captions in English or Thai from the laptop's microphone.
- Rehearse with Coach: each slide's time, pace, filler words and the parts read off the slide.
- Record narration per slide and Export to Video as an MP4.
- Tidy Slide, which lines up, sizes and spaces a slide's shapes in one click.
- Find a Slide, which searches the decks in your folders.
- Check Before Sharing and Blur Area for patient details and photos.

It is a fork of [DeckCraft](https://github.com/storytold/deckcraft) by the ArtCraft Team, an
open-source clean-room reimplementation of PowerPoint in Rust (MIT OR Apache-2.0). The DeckCraft
crates keep their names (`deckcraft-*`) so later upstream changes merge cleanly. The ArtCraft and
DeckCraft names and logos are trademarks of the ArtCraft Team and are not used to name this fork.

## Adding an interactive object

Open the deck, go to the slide, and on the Insert tab use the Interactive group:

- **3D model** asks for a `.glb` file. Its node names become the layer list and the model lands on
  the slide. In the editor, drag the round handle in its middle to turn it and use the wheel to
  zoom; in the show, drag the model itself. A 3D model inserted by PowerPoint turns the same way.
- **Image swap** asks for several pictures; they show in the order picked and a click shows the next.
- **Wipe** asks for two pictures, before and after, under one slider.
- **Zoom region** and **Popup** take no file. The region zooms up to fill the slide; the popup
  opens a panel with text, a picture or a web address.
- **Poll** asks a question with answers; phones vote, the room sees the count, and the reveal marks
  the right answer. Under Advanced each answer can lead to a slide.
- **Scrub** asks for 2 to 12 pictures in slider order, each with a label such as `Day 7`.
- **Measure** and **Injection map** take no file. Put the box over a photograph on the slide.

The new object lands over the selected shape and moves with it, or in the middle of the slide.
Drag it, resize it by the handles, copy, paste, delete and undo it like any other shape. The pane on
the right shows the object's name, the controls its type owns and Delete; **Advanced** holds the
trigger, the shortcut key, whether it is visible when the slide opens, which shape it moves with,
and the type's finer settings.

## In the show

Slide Show › From Beginning opens the show on the second display when one is attached and the
presenter view on the first. Each object's shortcut key works in both windows; a model's key
peels its layers one by one. The presenter view has one row per object on the current slide
(layers and a view reset for a model, next for a swap, a slider for a wipe, in and out for a zoom,
open and close for a popup, open voting, close voting and reveal for a poll, a slider for a scrub,
the presets and the reading for a measurement, the products, dose, undo and clear for an injection
map), a display picker with Swap, and the Phones section with the QR code the audience scans.

Phones: one QR code, a page in the phone's browser, a name. Phones vote in polls and see the
tally once it is revealed. The presenter grants one phone control at a time; that phone turns the
model, points, or draws on the slide, and the presenter takes control back or clears the drawing.
The server listens on the laptop's wifi address; when the venue wifi keeps phones apart, Start
hotspot turns on the Windows mobile hotspot and the QR follows it.

## How the objects are stored

Each object is an ordinary shape in the slide with a tag named `DECKRT.OBJECT` holding its settings
as JSON. The 3D model is PowerPoint's own 3D model shape, so real PowerPoint shows a turnable model
too. The other objects fall back to a picture or a text box in PowerPoint: a swap and a scrub show
their first picture, a wipe its before picture, a poll its question and answers, a popup its
trigger box. Pictures and models are parts of the package like any other media.

A deck made with the earlier deckrt app (a `.pptx` with a `.deckrt` sidecar next to it) gets its
objects brought in when it opens; a dialog says what came over. The sidecar file is left as it is,
and saving the deck keeps the objects inside the .pptx.

## Building it yourself

Windows: double-click `Start deckrt.bat` next to `deckrt.exe`. To build the program on your own
machine, double-click `Build deckrt.bat` once (it needs Rust from https://rustup.rs and the Visual
Studio C++ build tools), which puts `deckrt.exe` next to the .bat files.

From a terminal on any platform:

```sh
cargo run --release -p deckcraft            # the editor
cargo run --release -p deckcraft-cli -- render deck.pptx --all out/
```

On Linux the build needs the ALSA headers (`libasound2-dev` on Debian and Ubuntu).

If no window opens on Windows: deckrt starts itself with DirectX 12 first, then Vulkan, OpenGL
and Windows' software renderer, and remembers the one that worked. `%APPDATA%\deckrt\logs\startup.log`
lists the attempts and `deckcraft.log` the last one. `Start deckrt.bat` has two lines to force a
backend by hand.

## Making a version

The version number is in `Cargo.toml` (`cargo xtask version set 0.2.0`). Add a `## 0.2.0`
section to CHANGELOG.md, commit, then push the tag `deckrt-v0.2.0`. The Windows build makes the release
"deckrt 0.2.0" with the installer and the changelog section as its notes, and the Download link
above points to it from then on. Every push to main replaces the test build.

## Tests

```sh
cargo test -p deckcraft-interactive -p deckcraft-model3d -p deckcraft-phones
cargo test -p deckcraft-engine -p deckcraft-pptx -p deckcraft-ui-egui
```

## License

MIT OR Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
Copyright (c) 2026 ArtCraft Team and the DeckCraft contributors; fork changes copyright (c) 2026
the deckrt contributors. Required notices are in [NOTICE](NOTICE); every bundled asset is listed
with its author, source and licence in [ATTRIBUTION.md](ATTRIBUTION.md).

Microsoft and PowerPoint are trademarks of the Microsoft group of companies. deckrt is not
affiliated with, sponsored by or endorsed by Microsoft.
