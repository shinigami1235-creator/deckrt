// Runs the phone page, as the server serves it, inside jsdom against a running show: connects,
// joins, votes on the slide's poll and asks for control, and prints what the page shows at each
// step. Needs Node 18 or later and, in the folder it runs from, `npm install jsdom ws`.
//
//   node page-test.mjs "http://127.0.0.1:8720/?t=<token>"
//
// The token is in the presenter window's Phones section (or the app log). The poll steps only
// print something when the show is on a slide with a poll and voting is open.
import { JSDOM } from "jsdom";
import WebSocket from "ws";
const url = process.argv[2]; // http://127.0.0.1:8720/?t=TOKEN
const html = await (await fetch(url)).text();
const dom = new JSDOM(html, { url, runScripts: "dangerously", pretendToBeVisual: true, beforeParse(window) {
  window.WebSocket = WebSocket;

  window.HTMLCanvasElement.prototype.getContext = () => ({ clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, stroke() {}, set strokeStyle(v) {}, set lineWidth(v) {}, set lineCap(v) {}, set lineJoin(v) {} });
} });
const { window } = dom;
const $ = (id) => window.document.getElementById(id);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await sleep(1500);
console.log("status:", $("status").textContent);
$("name").value = "Page tester";
$("join-btn").click();
await sleep(1500);
const api = window.deckrtPhone;
console.log("joined:", api.state.joined, "name:", $("me").textContent, "join hidden:", $("join").hidden);
console.log("poll hidden:", $("poll").hidden, "question:", $("question").textContent);
const options = window.document.querySelectorAll("#options .option");
console.log("options:", [...options].map((b) => b.textContent + (b.disabled ? "(disabled)" : "")));
if (options.length) { options[1].click(); await sleep(1500); console.log("note:", $("poll-note").textContent, "mine:", [...options].map((b) => b.dataset.mine)); }
console.log("control hidden:", $("control").hidden, "viewer hidden:", $("viewer").hidden, "viewer-ask hidden:", $("viewer-ask").hidden, "img:", ($("viewer-img").getAttribute("src") || "").slice(0, 30));
$("viewer-ask").hidden ? $("ask").click() : $("viewer-ask").click();
await sleep(1500);
console.log("banner:", $("banner").textContent, "| control-note:", $("control-note").textContent);
console.log("sent:", api.sent.map((m) => m.t).join(","));
console.log("wakes:", api.wakes.map((w) => w.text));
await sleep(500);
window.close();
process.exit(0);
