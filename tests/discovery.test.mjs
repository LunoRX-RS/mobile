import { test } from "bun:test";
import assert from "node:assert/strict";
import { useFeeds, loadFeed, invalidateHomeFeed } from "../src/lib/discovery.ts";
test("history changes discard stale recommendations before refreshing",async()=>{
  const pending=[];
  const before=globalThis.window;
  Object.defineProperty(globalThis,"window",{configurable:true,value:{__TAURI_INTERNALS__:{invoke:(cmd)=>{assert.equal(cmd,"discovery_feed");return new Promise(resolve=>pending.push(resolve));}}}});
  try {
    useFeeds.setState({feeds:{}});
    const old=loadFeed("home");assert.equal(pending.length,1);
    invalidateHomeFeed();assert.equal(pending.length,2);
    pending[1]([{title:"New history",tracks:[]}]);await Promise.resolve();await Promise.resolve();
    pending[0]([{title:"Old history",tracks:[]}]);await old;
    assert.equal(useFeeds.getState().feeds["home:charts"].sections[0].title,"New history");
  }finally{Object.defineProperty(globalThis,"window",{configurable:true,value:before});}
});
