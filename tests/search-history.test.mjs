import { test, beforeEach } from "bun:test";
import assert from "node:assert/strict";
import { useUi, hydrateRecentSearches } from "../src/lib/store.ts";
const storage = new Map();
Object.defineProperty(globalThis, "window", { configurable: true, value: { localStorage: { getItem: k => storage.get(k) ?? null, setItem: (k,v) => storage.set(k,v) } } });
beforeEach(() => { storage.clear(); useUi.setState({ recentSearches: [] }); });
test("search history persists, deduplicates and removes individual queries", () => {
  useUi.getState().pushRecentSearch(" BU ");useUi.getState().pushRecentSearch("lofi");useUi.getState().pushRecentSearch("BU");
  useUi.setState({recentSearches:[]});hydrateRecentSearches();assert.deepEqual(useUi.getState().recentSearches,["BU","lofi"]);
  useUi.getState().removeRecentSearch("BU");useUi.setState({recentSearches:[]});hydrateRecentSearches();assert.deepEqual(useUi.getState().recentSearches,["lofi"]);
  useUi.getState().clearRecentSearches();hydrateRecentSearches();assert.deepEqual(useUi.getState().recentSearches,[]);
});
test("history is bounded and corrupt stored values are ignored", () => {
  for(let i=0;i<30;i++)useUi.getState().pushRecentSearch(`query ${i}`);
  assert.equal(useUi.getState().recentSearches.length,20);
  storage.set("lunorx.recentSearches",JSON.stringify([null,42,"", "valid","valid","x".repeat(257)]));hydrateRecentSearches();assert.deepEqual(useUi.getState().recentSearches,["valid"]);
  storage.set("lunorx.recentSearches","{invalid");assert.doesNotThrow(hydrateRecentSearches);
});
