# Changes

Each numbered version of deckrt has a section here, newest first. The section becomes the notes
on the version's page under Releases when the build makes that version. Unreleased lists what is
in the test build so far.

## Unreleased

- **Sliders for picture and fill transparency**, and finer picture controls: Brightness,
  Contrast, Saturation, Transparency and Crop each have a slider and a number box, and the
  picture changes while you drag. Saturation is now kept when the deck is saved.
- **Bigger notes in presenter view.** The notes have a panel of their own, drag the dividers to
  give them more room, Hide next slide gives them the whole column, and Ctrl + mouse wheel sizes
  the text. The layout is kept for the next show.
- **Previews without the slide show.** Transitions › Preview plays on the slide, picking an
  effect plays it (AutoPreview), and hovering over an effect in the gallery shows it moving.
- **Pictures in the Insert menus** for Chart, WordArt, Action and SmartArt.
- **Insert › Screenshot**: pictures of your open windows, and Screen Clipping.
- **The sample deck** is in File › New: one slide for each feature, with notes to read aloud.

## 0.1.0 (2026-10-18)

The first numbered version.

Opening, editing and saving PowerPoint files comes from DeckCraft. deckrt adds:

- **Interactive objects** on the Insert tab: a 3D model you turn with its layers, an image swap,
  a before-and-after wipe, a zoom region, a popup, a poll, a scrub slider, a measurement overlay
  and an injection map. They are saved inside the .pptx.
- **Objects in the show's order.** Next can step an object before the show moves on, and an
  object can run by itself when the slide opens or after its animations, with a wait in seconds.
  Double-click an object in the editor to try it there.
- **Phones in the audience** join with one QR code to vote in polls, ask questions, send
  reactions, fill in a feedback form at the end and save a handout of the slides and notes as a
  PDF. One phone at a time can be given control to turn a model, point or draw.
- **Live Captions** from the microphone, in English or Thai, run on the laptop. The speech model
  (150 MB) downloads once the first time.
- **Rehearse with Coach** listens while you practise and reports each slide's time, pace, filler
  words and the parts read straight off the slide. It can keep the pace for presenter view and
  add what you said to the notes.
- **Record** keeps what you say on each slide as its narration and timing, and **Export to
  Video** makes an MP4 from the deck with the narration.
- **Tidy Slide** lines up, sizes and spaces the slide's shapes and fixes text that is too small or
  runs out of its box.
- **Find a Slide** searches the decks in your folders and inserts a slide from any of them.
- **Check Before Sharing** finds patient IDs in text and notes, cropped-away parts of pictures,
  photo details and hidden text before you send the file. **Blur Area** blurs part of a
  photo for good once the deck is saved.
- **Touch in the show.** Two fingers pinch the slide in and out and drag the zoomed view.

Known gaps:

- Windows only. The installer isn't signed, so Windows asks before running it the first time.
- A 3D model has no 3D Model tab and is turned by its round handle in the editor.
- A resized swap or scrub picture is stretched in the editor while the show fits it in its box.
