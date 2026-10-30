import { test } from "node:test";
import assert from "node:assert/strict";
import { parseYouTubeLink } from "./ytLink";

test("YouTube and YouTube Music links give the video id", () => {
  const id = "dQw4w9WgXcQ";
  assert.deepEqual(parseYouTubeLink(`https://www.youtube.com/watch?v=${id}&list=RD1&t=42s`), { id, music: false });
  assert.deepEqual(parseYouTubeLink(`https://music.youtube.com/watch?v=${id}&si=abc`), { id, music: true });
  assert.deepEqual(parseYouTubeLink(`https://youtu.be/${id}?si=x`), { id, music: false });
  assert.deepEqual(parseYouTubeLink(`youtube.com/shorts/${id}`), { id, music: false });
  assert.deepEqual(parseYouTubeLink(`https://m.youtube.com/watch?v=${id}`), { id, music: false });
});

test("anything else is not a link to play", () => {
  for (const text of [
    "dQw4w9WgXcQ",
    "lofi hip hop",
    "https://example.com/watch?v=dQw4w9WgXcQ",
    "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
    "https://www.youtube.com/watch?v=short",
    "https://www.youtube.com/playlist?list=PL123",
    "javascript:alert(1)",
  ]) {
    assert.equal(parseYouTubeLink(text), null, text);
  }
});
