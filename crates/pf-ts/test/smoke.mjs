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
  // v1.0.10 audit fix: napi-rs deserializes JS plain objects into
  // Rust `BTreeMap<String, String>`; JS `Map` instances silently
  // serialize to `{}` and never reach Rust. The prior version of
  // this test passed `new Map([...])`, which hid the env-leak bug
  // because the Rust side simply received empty env. Use a plain
  // object — that is what real JS callers use and what the typed
  // signature documents.
  //
  // Note on var names: `PWD` matches the default scrub regex
  // (`pwd` is in the secret list — working-directory paths often
  // leak usernames/customer names), so we use `WORKSPACE` and
  // `USER` here, which are non-secret-shaped and survive verbatim.
  const cid = pf.snapshotFilesystem(
    store,
    "test",
    sandbox,
    { WORKSPACE: sandbox, USER: "smoke" },
    [{ role: "user", content: "hi" }],
  );
  assert.match(cid, /^sha256:[0-9a-f]{64}$/);
  const manifestJson = pf.readManifest(store, cid);
  const m = JSON.parse(manifestJson);
  assert.equal(m.schema_version, 1);
  assert.equal(m.agent.kind, "test");
  // Confirm the env actually crossed the boundary (catches future
  // regressions where someone accidentally re-introduces Map usage).
  const envBytes = pf.readBlob(store, m.world.env);
  const envBlob = JSON.parse(Buffer.from(envBytes).toString("utf8"));
  assert.equal(envBlob.vars.WORKSPACE, sandbox);
  assert.equal(envBlob.vars.USER, "smoke");
});

test("checkout round-trip writes the same files back", () => {
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(store, "t", sandbox, {}, []);
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
  const cid = pf.snapshotFilesystem(store, "t", sandbox, {}, []);
  const report = pf.merge(store, cid, cid);
  assert.equal(report.overall, "clean");
  assert.equal(report.ancestor, cid);
});

test("v1.0.10: default env scrub redacts secret-shaped names", () => {
  // Mirrors the Python SDK regression test
  // `test_default_scrub_redacts_secret_shaped_env`. Prior versions
  // of the TS SDK stored env values verbatim, so JS callers doing
  //   snapshotFilesystem(store, kind, root,
  //                      { OPENAI_API_KEY: "...", PWD: root }, [])
  // leaked the raw API-key bytes into the .pfimg.
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(
    store,
    "redact-test",
    sandbox,
    {
      OPENAI_API_KEY: "sk-real-secret-must-not-appear",
      GITHUB_TOKEN: "ghp_real-secret-must-not-appear",
      DATABASE_PASSWORD: "real-secret-must-not-appear",
      MY_API_KEY: "real-secret-must-not-appear",
      // Non-secret-shaped names must survive verbatim.
      PWD: sandbox,
      USER: "smoke",
    },
    [],
  );
  const m = JSON.parse(pf.readManifest(store, cid));
  const envBytes = pf.readBlob(store, m.world.env);
  const envText = Buffer.from(envBytes).toString("utf8");
  const envBlob = JSON.parse(envText);
  assert.equal(envBlob.vars.OPENAI_API_KEY, "<redacted>");
  assert.equal(envBlob.vars.GITHUB_TOKEN, "<redacted>");
  assert.equal(envBlob.vars.DATABASE_PASSWORD, "<redacted>");
  assert.equal(envBlob.vars.MY_API_KEY, "<redacted>");
  assert.equal(envBlob.vars.USER, "smoke");
  // Hard guarantee: the secret bytes do not appear anywhere in the
  // serialized blob.
  assert.equal(envText.includes("sk-real-secret-must-not-appear"), false);
  assert.equal(envText.includes("ghp_real-secret-must-not-appear"), false);
  assert.equal(envText.includes("real-secret-must-not-appear"), false);
});

test("v1.0.10: defaultScrubEnv = false opts out", () => {
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(
    store,
    "t",
    sandbox,
    { OPENAI_API_KEY: "sk-test-value" },
    [],
    { defaultScrubEnv: false },
  );
  const m = JSON.parse(pf.readManifest(store, cid));
  const envBlob = JSON.parse(Buffer.from(pf.readBlob(store, m.world.env)).toString("utf8"));
  assert.equal(envBlob.vars.OPENAI_API_KEY, "sk-test-value");
});

test("v1.0.10: effects ledger is HMAC-chained (not always empty)", () => {
  // Prior versions of the TS SDK always wrote
  // `{"kind":"effects.ledger.v1","entries":0}\n` regardless of
  // whether the caller wanted ACRFence. Now `opts.effects = [...]`
  // routes through `pf_effects::Ledger::append` exactly like the
  // Python SDK and CLI.
  const root = mkdtempSync(join(tmpdir(), "pf-ts-"));
  const sandbox = join(root, "sandbox");
  makeSandbox(sandbox);
  const store = pf.PfStore.open(join(root, "store"));
  const cid = pf.snapshotFilesystem(
    store,
    "t",
    sandbox,
    {},
    [],
    {
      effects: [
        {
          toolId: "send_email",
          argsHash: "sha256:" + "a".repeat(64),
          resultHash: "sha256:" + "b".repeat(64),
          idempotencyKey: "msg-001",
          sideEffectClass: "irreversible",
        },
        {
          toolId: "git_push",
          argsHash: "sha256:" + "c".repeat(64),
          resultHash: "sha256:" + "d".repeat(64),
          idempotencyKey: "push-001",
          sideEffectClass: "irreversible",
        },
      ],
    },
  );
  const m = JSON.parse(pf.readManifest(store, cid));
  const ledger = Buffer.from(pf.readBlob(store, m.effects.ledger)).toString("utf8");
  const [headerLine, ...entryLines] = ledger.trim().split("\n");
  const header = JSON.parse(headerLine);
  assert.equal(header.kind, "effects.ledger.v1");
  // Tamper-detection mode: header carries embedded session secret
  // so `pf verify` can validate without out-of-band material.
  assert.equal(header.verification_mode, "tamper-detection");
  assert.ok(header.session_secret_hex && header.session_secret_hex.length >= 32);
  // 2 entries, each with non-empty session_hmac (was "" in prior
  // raw-JSONL bug).
  const realEntries = entryLines.filter((l) => l.trim() !== "");
  assert.equal(realEntries.length, 2);
  for (const line of realEntries) {
    const e = JSON.parse(line);
    assert.ok(e.session_hmac && e.session_hmac.length >= 32,
      `ledger entry has empty session_hmac (raw-JSONL regression): ${line}`);
  }
});
