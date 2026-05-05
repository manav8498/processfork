// SPDX-License-Identifier: MIT
//
// Smoke test for the TypeScript / Node SDK. Run with:
//
//     cd crates/pf-ts
//     npm install --save-dev @napi-rs/cli
//     npx napi build --release  # produces processfork.<triple>.node
//     node --test test/smoke.mjs
//
// Skips cleanly if the .node binary isn't built yet.

import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

let pf;
try {
  pf = await import("../index.js");
} catch (e) {
  console.warn("skipping: napi cdylib not built. Run `napi build --release` first.");
  process.exit(0);
}

function makeSandbox(root) {
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "main.ts"), "console.log('hello')\n");
  writeFileSync(join(root, "README.md"), "# demo\n");
}

test("digestOf is canonical", () => {
  // napi maps Rust `Vec<u8>` → JS `Array<number>` (not Node Buffer). v1.1
  // is planned to accept Buffer directly via napi-rs's Buffer type.
  const d = pf.digestOf([]);
  assert.equal(
    d,
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  );
});

test("PfStore.open is idempotent", () => {
  const dir = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const a = pf.PfStore.open(join(dir, "store"));
  const b = pf.PfStore.open(join(dir, "store"));
  assert.equal(a.physicalBytes(), b.physicalBytes());
});

test("snapshot → readManifest", () => {
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(
    store,
    "test",
    sandbox,
    new Map([["PWD", sandbox]]),
    [{ role: "user", content: "hi" }],
  );
  assert.match(cid, /^sha256:[0-9a-f]{64}$/);
  const manifestJson = pf.readManifest(store, cid);
  const m = JSON.parse(manifestJson);
  assert.equal(m.schema_version, 1);
  assert.equal(m.agent.kind, "test");
});

test("checkout round-trip writes the same files back", () => {
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(store, "t", sandbox, new Map(), []);
  const target = join(root, "restored");
  pf.checkoutFilesystem(store, cid, target);
  assert.equal(readFileSync(join(target, "src", "main.ts"), "utf8"), "console.log('hello')\n");
  assert.equal(readFileSync(join(target, "README.md"), "utf8"), "# demo\n");
});

test("merge of A with itself is clean", () => {
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(store, "t", sandbox, new Map(), []);
  const report = pf.merge(store, cid, cid);
  assert.equal(report.overall, "clean");
  assert.equal(report.ancestor, cid);
});
