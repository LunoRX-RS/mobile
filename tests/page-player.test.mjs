import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";

const source = readFileSync(new URL("../src-tauri/plugins/lunor-native/android/src/main/java/io/lunorx/nativeplugin/PagePlayer.kt", import.meta.url), "utf8");
const script = source.match(/val SCRIPT = """([\s\S]*?)"""\.trimIndent\(\)/)[1];

function page() {
  let tick;
  let attempts = 0;
  let reject = false;
  let ad = false;
  const media = {
    paused: true, ended: false, error: null, readyState: 4,
    currentTime: 1, duration: 180, muted: false, volume: 1, playbackRate: 1,
    addEventListener() {},
    play() {
      attempts++;
      if (reject) return Promise.reject(new Error("not ready"));
      this.paused = false;
      return Promise.resolve();
    },
    pause() { this.paused = true; },
  };
  const window = {};
  const location = { search: "?v=abcdefghijk" };
  const document = {
    querySelector: (selector) => selector === "video" ? media : null,
    getElementById: () => ({ classList: { contains: () => ad } }),
  };
  runInNewContext(script, {
    window, document, location, URLSearchParams,
    console: { log() {} }, setInterval: (fn) => { tick = fn; },
  });
  return { media, location, document, player: window.__lunorx, tick: () => tick(),
    attempts: () => attempts, reject: (value) => { reject = value; }, ad: (value) => { ad = value; } };
}

test("page retries a rejected start and an unexpected pause while play is wanted", async () => {
  const p = page();
  p.reject(true);
  p.player.play();
  await Promise.resolve();
  p.reject(false);
  p.tick();
  await Promise.resolve();
  assert.equal(p.media.paused, false);
  p.media.paused = true;
  p.tick();
  await Promise.resolve();
  assert.equal(p.media.paused, false);
});

test("page respects an explicit pause and never restarts ended or failed media", async () => {
  const p = page();
  p.player.play();
  await Promise.resolve();
  p.player.pause();
  const attempts = p.attempts();
  p.tick();
  assert.equal(p.attempts(), attempts);
  p.media.ended = true;
  p.player.play();
  p.tick();
  assert.equal(p.attempts(), attempts);
  p.media.ended = false;
  p.media.error = {};
  p.player.play();
  p.tick();
  assert.equal(p.attempts(), attempts);
});

test("a delayed start cannot undo a later pause and overlapping starts are bounded", async () => {
  const p = page();
  let finish;
  p.media.play = () => new Promise((resolve) => { finish = resolve; });
  p.player.play();
  const first = finish;
  p.tick();
  assert.equal(finish, first);
  p.player.pause();
  p.media.paused = false;
  finish();
  await Promise.resolve();
  assert.equal(p.media.paused, true);
});

test("the page's own next video is stopped so core keeps control of the queue", async () => {
  const p = page();
  p.player.play();
  await Promise.resolve();
  p.location.search = "?v=lmnopqrstuv";
  p.media.paused = true;
  const attempts = p.attempts();
  p.tick();
  assert.equal(p.attempts(), attempts);
  assert.equal(p.media.paused, true);
  assert.match(p.document.title, /^LUNORX_ST\|abcdefghijk\|.*\|ended$/);
});

test("an ad reaches its end through the media clock instead of a seek to the terminal position", async () => {
  const p = page();
  p.ad(true);
  p.media.duration = 6;
  p.player.play();
  await Promise.resolve();
  p.tick();
  assert.equal(p.media.currentTime, 5.9);
  p.ad(false);
  p.media.currentTime = 0;
  p.media.duration = 180;
  p.tick();
  assert.equal(p.media.playbackRate, 1);
  assert.equal(p.media.muted, false);
});
