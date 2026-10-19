#!/usr/bin/env python3
"""Builds the sample deck that comes with deckrt (File › New › deckrt feature tour) from the
pictures make_assets.py draws:

    python3 make_assets.py /tmp/media
    python3 build_sample.py /tmp/media path/to/deckcraft-cli assets/sample/deckrt-feature-tour.pptx

One slide for each feature. The speaker notes are a script to read aloud when trying the
recording, the captions and the coach.
"""

import json
import subprocess
import sys

M, CLI, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
STAGE = OUT + ".stage.deckcraft"
cmds = []


def c(cid, params=None):
    cmds.append(cid if params is None else f"{cid}={json.dumps(params)}")


def text(rect, s, size=18):
    c("insert.textBox", {"rect": rect, "text": s})
    c("format.size", {"size": size})


def obj(kind, rect, settings=None, paths=None):
    p = {"kind": kind, "rect": rect}
    if paths:
        p["paths"] = paths
    if settings:
        p["object"] = settings
    c("interactive.insert", p)


c("design.theme", {"name": "Harbor"})

# 1 Title
c("slide.new", {"layout": "title", "title": "deckrt feature tour",
                "body": "One slide for each feature. The speaker notes are a script to read aloud."})
c("slide.delete", {"index": 0})

# 2 3D model with layers
c("slide.new", {"layout": "titleOnly", "title": "3D model with layers"})
text([50, 130, 420, 330],
     "The Earth in four layers: crust, mantle, outer core and inner core.\n\n"
     "In the show, drag the globe to turn it and scroll to zoom. Press L to take off one layer at a time. "
     "Press it again after the inner core and every layer comes back.")
obj("model", [520, 100, 380, 400], {"type": "model", "label": "Earth layers", "shortcut": "l"}, [f"{M}/Earth layers.glb"])
c("transition.set", {"kind": "fade"})

# 3 Image swap
c("slide.new", {"layout": "titleOnly", "title": "Image swap"})
text([50, 130, 300, 330], "Three phases of the Moon in one frame.\n\nClick the picture to show the next one.")
obj("image_swap", [380, 110, 530, 380],
    {"type": "image_swap", "label": "Moon phases", "trigger": "audience_click", "config": {"on_click": "advance"}},
    [f"{M}/Moon crescent.jpg", f"{M}/Moon first quarter.jpg", f"{M}/Moon full.jpg"])
c("transition.set", {"kind": "push", "option": "l"})

# 4 Wipe
c("slide.new", {"layout": "titleOnly", "title": "Wipe"})
text([50, 130, 260, 330], "One hill in winter and in spring.\n\nDrag the line across to compare.")
obj("wipe", [330, 110, 585, 395], {"type": "wipe", "label": "Winter and spring", "trigger": "audience_click"},
    [f"{M}/Hill in winter.jpg", f"{M}/Hill in spring.jpg"])
c("transition.set", {"kind": "wipe", "option": "l"})

# 5 Scrub
c("slide.new", {"layout": "titleOnly", "title": "Scrub"})
text([50, 130, 260, 330], "A bean plant on four days, on one slider.\n\nDrag it to step from Day 0 to Day 14. "
     "The labels come from the file names.")
obj("scrub", [330, 110, 520, 393], {"type": "scrub", "label": "Bean plant", "trigger": "audience_click"},
    [f"{M}/Day 0.jpg", f"{M}/Day 3.jpg", f"{M}/Day 7.jpg", f"{M}/Day 14.jpg"])

# 6 Zoom region
c("slide.new", {"layout": "titleOnly", "title": "Zoom region"})
text([50, 130, 250, 330], "Click the box over the market square to zoom into it. Click again to zoom back out.")
c("insert.picture", {"path": f"{M}/Town map.png", "rect": [330, 95, 380, 428]})
zr = [568, 200, 104, 92]
obj("zoom", zr, {"type": "zoom", "label": "Market square", "trigger": "audience_click",
                 "config": {"source_rect": [zr[0] / 960, zr[1] / 540, zr[2] / 960, zr[3] / 540], "max_scale": 3.0, "duration_ms": 300}})

# 7 Popup
c("slide.new", {"layout": "titleOnly", "title": "Popup"})
text([50, 130, 400, 150], "Click a label to open its note. Click the note to close it.")
obj("popup", [80, 300, 230, 60], {"type": "popup", "label": "Crust", "trigger": "audience_click", "config": {
    "content": {"kind": "text", "text": "The crust is the thin rocky skin of the Earth. Under the oceans it is about 7 km thick, "
                "and under the continents about 35 km."},
    "panel_size": [0.45, 0.4], "panel_position": [0.45, 0.3]}})
obj("popup", [80, 380, 230, 60], {"type": "popup", "label": "Inner core", "trigger": "audience_click", "config": {
    "content": {"kind": "text", "text": "The inner core is a ball of iron and nickel about 1,220 km across. "
                "It is as hot as the surface of the Sun and stays solid because of the pressure."},
    "panel_size": [0.45, 0.4], "panel_position": [0.45, 0.3]}})

# 8 Poll
c("slide.new", {"layout": "titleOnly", "title": "Poll"})
obj("poll", [120, 100, 720, 340], {"type": "poll", "label": "Hottest layer", "config": {
    "question": "Which layer of the Earth is the hottest?",
    "options": ["Crust", "Mantle", "Outer core", "Inner core"],
    "correct": 3, "reveal_correct": True}})
text([120, 455, 720, 60],
     "Phones vote after scanning the QR code in presenter view. After the reveal, presenter view offers slide 9 "
     "since Inner core leads there.", 14)

# 9 Answer
c("slide.new", {"layout": "titleAndContent", "title": "The inner core is the hottest",
                "body": "The inner core is about 5,200 °C, as hot as the surface of the Sun.\n"
                        "The poll on slide 8 offers this slide when Inner core wins."})

# 10 Measure
c("slide.new", {"layout": "titleOnly", "title": "Measure"})
text([50, 130, 300, 330],
     "Facial thirds over a picture.\n\nIn the show, drag the handles to the hairline, brow, nose base and chin. "
     "Fifths, Angle and Ratio are in the panel.")
face = [460, 80, 380, 440]
c("insert.picture", {"path": f"{M}/Face drawing.png", "rect": face})
obj("measure", face, {"type": "measure", "label": "Facial thirds", "trigger": "audience_click"})

# 11 Injection map
c("slide.new", {"layout": "titleOnly", "title": "Injection map"})
text([50, 130, 330, 330],
     "Click the face in the show to place a point. Each point takes the product picked in presenter view, "
     "Toxin in units or Filler in mL, and the total shows under the map.")
c("insert.picture", {"path": f"{M}/Face drawing.png", "rect": face})
obj("injection_map", face, {"type": "injection_map", "label": "Injection map", "trigger": "audience_click"})

# 12 Phones
c("slide.new", {"layout": "titleAndContent", "title": "Audience phones",
                "body": "Start the show with Present. Presenter view shows a QR code under the next slide, and Show QR to audience puts it on the big screen.\n"
                        "Phones scan it, type a name and vote in the poll on slide 8.\n"
                        "Grant gives one phone control of the globe on slide 2, and lets it point and draw on the slide.\n"
                        "If the venue wifi blocks phones, Start hotspot makes the laptop its own network."})

# 13 Word cloud
c("slide.new", {"layout": "titleOnly", "title": "Word cloud"})
obj("poll", [120, 100, 720, 340], {"type": "poll", "label": "Spring words", "config": {
    "kind": "word_cloud", "question": "What comes to mind when you hear spring?", "options": []}})
text([120, 455, 720, 60], "Phones type up to three words. The words most phones sent come up biggest.", 14)

# 14 Ranking
c("slide.new", {"layout": "titleOnly", "title": "Ranking"})
obj("poll", [120, 100, 720, 340], {"type": "poll", "label": "Planet sizes", "config": {
    "kind": "ranking", "question": "Put these planets in order of size, biggest first.",
    "options": ["Earth", "Mars", "Jupiter", "Venus"]}})
text([120, 455, 720, 60], "Phones drag the answers into their own order. The room's order shows when the answers close.", 14)

# 15 Rating
c("slide.new", {"layout": "titleOnly", "title": "Rating"})
obj("poll", [120, 100, 720, 340], {"type": "poll", "label": "Clarity", "config": {
    "kind": "rating", "scale": 5, "question": "How clear has this tour been so far?", "options": []}})

# 16 Questions and reactions
c("slide.new", {"layout": "titleAndContent", "title": "Questions and reactions",
                "body": "Phones ask from the Questions tab, with their name or without, and back the questions they want answered.\n"
                        "Presenter view lists the most backed first. Show on screen puts one up for the room.\n"
                        "Love it, Great, Wow and Lost me float up the side of the screen. Presenter view counts them per slide."})

# 17 Live captions
c("slide.new", {"layout": "titleAndContent", "title": "Live captions",
                "body": "Slide Show › Live Captions. Download the speech model once, then captions run on this laptop.\n"
                        "They show along the bottom of the screen and on the phones, in Thai or English.\n"
                        "Each slide's own words help it spell names like Jupiter."})

# 18 Rehearse with Coach
c("slide.new", {"layout": "titleAndContent", "title": "Rehearse with Coach",
                "body": "Slide Show › Rehearse with Coach. Talk through the deck out loud.\n"
                        "The report gives each slide's time, words a minute, filler words, long pauses and slides read out word for word.\n"
                        "Keep this pace, and presenter view says how far ahead or behind you are in the real talk."})

# 19 Record and video
c("slide.new", {"layout": "titleAndContent", "title": "Record and export to video",
                "body": "Record › Record from Beginning. Each slide keeps what you say while it is up.\n"
                        "The recording plays with the slide, in deckrt and in PowerPoint.\n"
                        "Record › Export to Video makes an MP4 with your voice and timings."})

# 20 Checks
c("slide.new", {"layout": "titleAndContent", "title": "Checks before a talk and before sharing",
                "body": "Check Deck runs when you press Present: videos that won't play, missing fonts, text off the slide.\n"
                        "Review › Check Before Sharing finds patient numbers, cropped parts of photos, photo details and notes.\n"
                        "Review › Blur Area blurs the part of a photo you drag over."})

# 21 Picture tools
c("slide.new", {"layout": "titleOnly", "title": "Picture tools"})
text([50, 130, 330, 330],
     "Click the picture, then open Picture Format. Corrections, Color and Transparency have sliders: "
     "drag one and the picture changes as you drag.\n\nInsert › Screenshot puts a picture of another window on the slide.")
c("insert.picture", {"path": f"{M}/Hill in spring.jpg", "rect": [420, 120, 480, 326]})

# 22 Animations and transitions
c("slide.new", {"layout": "titleOnly", "title": "Animations and transitions"})
text([50, 130, 400, 300],
     "Each box on the right flies in on a click. On the Animations tab, Preview plays them here on the slide.\n\n"
     "Hover over an effect in the gallery to see it move before you pick it.")
for i, (label, effect) in enumerate([("Crust", "fly"), ("Mantle", "fade"), ("Core", "zoom")]):
    c("shape.insert", {"preset": "roundRect", "rect": [520, 140 + i * 110, 300, 80], "text": label})
    c("animation.set", {"effect": effect, "class": "entrance"})
c("transition.set", {"kind": "morph"})

# 23 Find a Slide
c("slide.new", {"layout": "titleAndContent", "title": "Find a Slide",
                "body": "Home › Find a Slide searches every deck in Documents, Desktop and Downloads.\n"
                        "Type a word from a title, the text or the notes. Pick the slides and press Insert."})

# 24 Messy slide for Tidy Slide
c("slide.new", {"layout": "titleOnly", "title": "Tidy Slide"})
c("shape.insert", {"preset": "roundRect", "rect": [52, 150, 250, 150]})
c("shape.insert", {"preset": "roundRect", "rect": [361, 142, 232, 165]})
c("shape.insert", {"preset": "roundRect", "rect": [641, 157, 260, 140]})
text([58, 330, 240, 80], "Crust: the thin rocky skin, 7 to 35 km thick.", 9)
text([370, 322, 228, 80], "Mantle: hot rock that flows very slowly.", 14)
text([700, 338, 420, 80], "Core: iron and nickel, liquid outside and solid in the middle under the pressure.", 14)

# 25 End
c("slide.new", {"layout": "title", "title": "Thank you",
                "body": "The feedback form is on your phones. The handout is in the Slides tab."})

NOTES = [
    "Hello everyone. This is a quick tour of deckrt, the presentation app I use instead of PowerPoint. Each slide shows one thing it can do. Keep your phones out since a few slides ask for them.",
    "This is the Earth in four layers. I can drag it to turn it and scroll to zoom in. Each time I press L, one layer comes off: first the crust, then the mantle, then the outer core. Press L once more and everything comes back.",
    "These are three phases of the Moon in one frame. Each click shows the next one: a crescent, the first quarter, then the full moon.",
    "This is the same hill in winter and in spring. I drag the line across to compare the two. Watch the trees as the line passes over them.",
    "This is a bean plant on four days: day zero, day three, day seven and day fourteen. Dragging the slider steps through the growth. Most of the height comes in the second week.",
    "This is a map of a small town. Clicking the box zooms the slide into the market square, and clicking again zooms back out.",
    "Each label opens a short note. The crust is the thin rocky skin of the Earth. The inner core is as hot as the surface of the Sun and stays solid because of the pressure.",
    "Time for a question. Scan the code on the screen and vote. Which layer of the Earth is the hottest? You have thirty seconds.",
    "Most of you got it. The inner core is about five thousand two hundred degrees, as hot as the surface of the Sun.",
    "Here I can measure the facial thirds on a picture. I drag the handles to the hairline, the brow, the base of the nose and the chin. Fifths, angles and ratios are in the same panel.",
    "This is the injection map. Each click places a point with the product and dose I picked, toxin in units or filler in millilitres. The total shows under the face.",
    "Your phones can do more than vote. I can hand one phone control of the globe, and that phone can point and draw on the slide. If the venue wifi blocks phones, the laptop makes its own hotspot.",
    "Type up to three words that come to mind when you hear spring. The biggest words on the screen are the ones most of you typed.",
    "Put these four planets in order of size, biggest first. I'll show the room's order when I close the answers.",
    "How clear has this tour been so far? Rate it from one to five on your phone.",
    "If you have a question, open the Questions tab and ask it, with your name or without. Back the ones you want answered and I'll take the top ones at the end. The heart, star, wow and lost me buttons float up the side of the screen.",
    "Everything I say shows up at the bottom of the screen and on your phones. It runs on this laptop with no internet, and it understands Thai and English.",
    "Before a talk I rehearse with the coach. It tells me how long each slide took, how fast I spoke, how many times I said um, and which slides I read out word for word.",
    "I'm recording this talk right now. Each slide keeps what I said on it, and Export to Video turns the whole deck into a video for anyone who missed it.",
    "Before I present, Check Deck finds videos that won't play and text that runs off the slide. Before I share a deck, Check Before Sharing finds patient numbers, cropped parts of photos and the details hidden in photos. Blur Area blurs a face for good.",
    "The picture tools have sliders now. I click the picture, open Picture Format and drag Brightness, and the picture changes as I drag. Screenshot puts a picture of another window straight onto the slide.",
    "Each box flies in on a click. On the Animations tab, Preview plays them right here on the slide, and hovering over an effect shows it moving before I pick it.",
    "Find a Slide searches every deck on this laptop. If I type Earth, every slide I have made about the Earth shows up, and I can pull any of them into this deck.",
    "This slide is a mess on purpose. The boxes don't line up, the gaps are uneven, one line of text is tiny and the last one runs off the slide. Tidy Slide on the Home tab fixes all of it in one click.",
    "Thank you for listening. The feedback form is on your phones now, and the handout with your own notes is in the Slides tab.",
]
for i, t in enumerate(NOTES):
    c("slide.notes", {"index": i, "text": t})
c("slide.go", {"index": 0})

args = [CLI, "run"]
for x in cmds:
    args += ["--cmd", x]
args += ["--save", STAGE]
r = subprocess.run(args, capture_output=True, text=True)
print(r.stdout[-1500:], r.stderr[-3000:])
if r.returncode:
    sys.exit(1)

# The poll's winning answer leads to the answer slide: Inner core (option 4) to slide 9.
r = subprocess.run([CLI, "run", "--in", STAGE, "--cmd", 'slide.go={"index":7}', "--cmd", "interactive.list", "--print"],
                   capture_output=True, text=True)
items = json.loads(r.stdout.split("interactive.list →", 1)[1].strip().splitlines()[0])
poll = next(i for i in items if i["kind"] == "poll")
r = subprocess.run([CLI, "run", "--in", STAGE, "--cmd", 'slide.go={"index":7}',
                    "--cmd", "interactive.update=" + json.dumps({"id": poll["id"], "object": poll["object"], "targetSlides": [None, None, None, 8]}),
                    "--cmd", 'slide.go={"index":0}',
                    "--save", OUT], capture_output=True, text=True)
print(r.stdout[-1500:], r.stderr[-1500:])
subprocess.run(["rm", "-f", STAGE])
sys.exit(r.returncode)
