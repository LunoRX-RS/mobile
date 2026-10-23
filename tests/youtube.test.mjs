import { test } from "node:test";
import assert from "node:assert/strict";
import { youtubeVideoId } from "../src/lib/youtube.ts";
test("accepts YouTube IDs and supported URL forms", () => {
  for (const value of ["dQw4w9WgXcQ", " https://youtu.be/dQw4w9WgXcQ?t=20 ",
    "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123",
    "https://music.youtube.com/watch?v=dQw4w9WgXcQ", "https://youtube.com/shorts/dQw4w9WgXcQ"]) {
    assert.equal(youtubeVideoId(value), "dQw4w9WgXcQ");
  }
});
test("rejects unrelated hosts, playlist-only links and malformed IDs", () => {
  for (const value of ["", "abc", "javascript:alert(1)", "http://youtube.com/watch?v=dQw4w9WgXcQ",
    "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ", "https://evil.test/watch?v=dQw4w9WgXcQ",
    "https://u@youtube.com/watch?v=dQw4w9WgXcQ", "https://youtube.com/playlist?list=abc",
    "https://youtu.be/dQw4w9WgXcQ/extra", "https://youtube.com/watch?v=invalid"]) {
    assert.equal(youtubeVideoId(value), null, value);
  }
});
