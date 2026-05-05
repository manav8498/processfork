# TypeScript SDK

Install:

```bash
npm install @processfork/sdk
```

## Quick reference

```typescript
import { PfStore, snapshotFilesystem, checkoutFilesystem, merge, readManifest } from "@processfork/sdk";

const store = PfStore.open("~/.processfork");

const cid = snapshotFilesystem(
  store,
  "claude-code",
  "/tmp/sandbox",
  new Map([["PWD", "/tmp/sandbox"]]),
  [{ role: "user", content: "go" }],
);

const manifest = readManifest(store, cid);
checkoutFilesystem(store, cid, "/tmp/restored");

const report = merge(store, cidA, cidB);
console.log(report.overall);  // "clean" | "conflicted"
```

## Types

Auto-generated `index.d.ts` covers every exported function +
`MergeReport` / `WorldConflict` / `Message` / `MergeOpts`. The thin
TS wrapper at `crates/pf-ts/ts/index.ts` adds a typed `Manifest`
interface.

## Source

[`crates/pf-ts/`](https://github.com/manav8498/processfork/tree/main/crates/pf-ts).
