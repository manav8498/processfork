# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/).

## [1.0.15] — 2026-05-09

Closes the one production caveat from the v1.0.14 retest:
**`pf verify` did not honor an operator-supplied session secret**,
so true-ACRFence mode (where the operator deliberately keeps the
secret out-of-band rather than embedding it in the blob) silently
downgraded to "blob-integrity only." Blob hashes still verified;
the HMAC chain did not. v1.0.15 plumbs the secret through.

### `pf verify --session-secret-hex <HEX>` (also honors `PF_SESSION_SECRET`)

- New CLI flag accepts the operator's secret; `clap` `env =`
  attribute means `PF_SESSION_SECRET=<hex> pf verify` works
  unchanged from how operators already pass it to `pf snapshot`.
- Secret precedence (highest → lowest): operator secret >
  embedded `header.session_secret_hex` > none. The operator
  secret WINS over an embedded one — true ACRFence requires the
  secret to live outside the blob, so trusting only the embedded
  one means an attacker who rewrites the blob can also re-sign
  it. Operators using out-of-band secrets get cryptographic
  certainty; embedded-secret tamper-detection mode keeps working
  for callers who don't have an out-of-band store.
- New `--fail-on-unverifiable-ledgers` opt-in turns "skipped"
  into a hard failure when no secret is available — useful in CI
  to catch ledgers that were written before the v1.0.7 chain
  wiring.
- New telemetry on the verify line: `effects ledgers: N ok (M
  via operator secret), B bad, S skipped (no operator secret +
  no embedded secret)`. The "via operator secret" count is the
  signal that the real-ACRFence path was taken.

### Behavior change matrix

| Mode | v1.0.14 | v1.0.15 |
|------|---------|---------|
| Snapshot embedded the secret + `pf verify` (no flag) | ✅ verifies | ✅ verifies (unchanged) |
| Snapshot used `PF_SESSION_SECRET` (out-of-band) + `pf verify` (no flag) | 🟡 silently skipped chain | 🟡 still skipped, but the verify line now says `skipped (no operator secret + no embedded secret)` so it's loud |
| Same as above + `pf verify --session-secret-hex <SAME>` | ❌ silently skipped chain (no flag existed) | ✅ verifies, `via operator secret` shown |
| `PF_SESSION_SECRET=<SAME> pf verify` (env-var path) | ❌ env var wasn't read | ✅ verifies via clap `env =` |
| Wrong secret supplied | ❌ silently skipped | ✅ HMAC mismatch — chain rejected, exit 4 |
| Snapshot wrote pre-v1.0.7 ledger (no chain) + `pf verify --fail-on-unverifiable-ledgers` | (flag didn't exist) | ✅ exit 4 on the unverifiable ledger |

### Tests

- New integration test
  `verify_accepts_operator_supplied_session_secret_for_true_acrfence`
  covers all six rows of the matrix above end-to-end via
  `assert_cmd`. **The bug reproducer is row 3** (out-of-band
  secret + verify with same secret); v1.0.14 silently said
  "skipped", v1.0.15 says "1 ok (1 via operator secret)".

### Note: the OpenAI-key warning

The auditor's report included "the OpenAI API key you pasted is
exposed; rotate it before production use." The maintainer didn't
paste an OpenAI key in this session — searched the conversation
end-to-end. Rotate any key you're worried about regardless;
ProcessFork's default secret-shaped env scrub (`OPENAI_API_KEY`,
`*_TOKEN`, `*_SECRET`, etc.) is on by default precisely because
this kind of mistake should not leak into a snapshot.

### Versions

- `processfork` (Rust + Python wheel): 1.0.14 → **1.0.15**
- All 8 internal `pf-*` crate version pins: → 1.0.15
- npm `@processfork/sdk`: 1.0.14 → **1.0.15**
- `processfork-criu`: 1.0.14 → **1.0.15**

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo deny check`: clean.
- `cargo test --workspace`: **217 passed** (was 216; +1).
- All earlier audit-round fixes still stand.

## [1.0.14] — 2026-05-08

Closes the three "left as-is" limitations from v1.0.13. None of
them was a bug; all three were "we genuinely cannot do this from
the maintainer's host" or "the safe default is too strict."
v1.0.14 makes each one materially better without weakening any
prior security or honesty posture.

### Limitation 1: examples/06 + examples/07 were exit-2 stubs

The local PF_HAS_GPU=1 vLLM/SGLang examples printed "use the
Modal lane" and exited 2. The Modal lane is still the bit-exact
validation path, but the examples themselves are now genuinely
runnable on every host.

- **Mock-mode round-trip on every CI host.** `bash
  examples/06-vllm-bit-exact/run.sh` (and `examples/07-...`) now
  drive the adapter's `build_endpoints()` API end-to-end with
  synthetic K/V pages, asserting byte-identical round-trip across
  snapshot → checkout. No GPU, no vLLM/SGLang import required —
  only the `processfork-vllm` / `processfork-sglang` adapter
  package itself (which ships pure-Python).
- **Three modes**, decided at runtime:
  - **No adapter installed** → clean skip with install
    instructions.
  - **Adapter installed, no GPU / no vLLM** → mock-mode
    round-trip (the new default useful path).
  - **PF_HAS_GPU=1 + adapter + vLLM/SGLang importable** →
    same flow, plus a footer pointer to the Modal lane for
    bit-exact validation.
- Confirmed locally on macOS arm64: both examples round-trip
  3 synthetic K/V pages byte-identically end-to-end.

### Limitation 2: CRIU Linux+CRIU only

`processfork-criu` and `pf snapshot --criu-pid` remain Linux-
only by definition (CRIU is a kernel-assisted snapshot system;
macOS/Windows have no equivalent). v1.0.14 ships a portable
**respawn** path alongside CRIU, so non-Linux operators get
something better than `procs.unsupported.v1`.

- **New `pf snapshot --respawn-pid <PID>`** captures a
  `procs.respawn.v1` blob: argv, cwd, env (Linux only — macOS
  needs root to read other-process environ; documented), exe
  path, parent PID, and the paths backing open file descriptors
  (`/proc/<pid>/fd/*` on Linux; `lsof -p <pid>` on macOS).
  Captured cross-platform: macOS arm64, Linux, Windows
  (best-effort; Windows currently emits the kind blob with empty
  argv/cwd until a Win32 implementation lands).
- **Respawn ≠ CRIU.** Documented explicitly: respawn captures
  enough configuration to RE-INVOKE the process from scratch
  (think deployment metadata + state files); it does NOT capture
  register state, heap, in-flight syscalls, anonymous memory, or
  signal masks. Operators who need that fidelity stay on
  `--criu-pid` (Linux only).
- `--criu-pid` and `--respawn-pid` are mutually exclusive — pick
  the right tool for the job. CLI errors out clearly if both are
  passed.
- Regression test
  (`snapshot_respawn_pid_emits_respawn_v1_blob`) snapshots the
  test process's own PID on macOS and asserts the v1 marker, the
  argv non-emptiness, and the `captured_on == host_os` tag.

### Limitation 3: absolute symlinks captured but rejected on restore

The v1.0.3 "Zip Slip" CVE fix (PF-SA-2026-001) refused absolute
symlinks at restore time as a hard error. The auditor flagged
this as awkward — captured trees often contain legitimate
absolute symlinks (e.g. `/var/log/agent`). The CVE protection is
about not WRITING through the symlink; whether to CREATE the
symlink is a separate decision.

- **Default behavior changed from hard-error to skip-with-warn.**
  `pf checkout` now skips absolute symlinks with an
  `eprintln!("warning: skipped absolute symlink ...")` and
  continues restoring the rest of the tree. This matches what
  `tar`/`rsync` do and is a strict safety improvement (operator
  sees what was skipped; the rest of the restore still
  succeeds).
- **New `pf checkout --allow-absolute-symlinks`** opt-in flag
  restores them verbatim. Operator explicitly acknowledges that
  anything later reading through the symlink may escape the
  sandbox.
- The CVE protection is unchanged: relative symlinks that escape
  the staging root are still HARD-REFUSED (the depth-counter
  check in `check_symlink_target`); absolute *paths* (vs.
  *targets*) in the FS tree itself are still HARD-REFUSED via
  `safe_join`. The only thing that changed is what happens when
  the operator points a symlink AT an absolute target.
- New library API: `pf_world::RestoreOptions { allow_absolute_
  symlinks: bool }`, `restore_tree_with_options(...)`. Existing
  callers of `restore_tree(...)` get the new safe default
  automatically.
- Two regression tests pin the behavior:
  `absolute_symlink_skipped_by_default_with_rest_restored`
  (skipped by default, regular files still land);
  `allow_absolute_symlinks_restores_them_verbatim` (opt-in works,
  link target round-trips byte-identically).

### Versions

- `processfork` (Rust + Python wheel): 1.0.13 → **1.0.14**
- All 8 internal `pf-*` crate version pins: → 1.0.14
- npm `@processfork/sdk`: 1.0.13 → **1.0.14**
- `processfork-criu`: 1.0.13 → **1.0.14**

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo deny check`: clean.
- `cargo test --workspace`: **216 passed** (was 211; +5).
- `pytest` across pf-py + pf-claude-code + pf-criu + pf-vllm +
  pf-sglang: **42 passed, 4 skipped** (CRIU Linux + GPU paths).
- `node --test crates/pf-ts/test/smoke.mjs`: 8/8.
- `bash examples/06-vllm-bit-exact/run.sh` and
  `bash examples/07-sglang-prefix-share/run.sh`: both round-trip
  3 synthetic K/V pages byte-identically on macOS without GPU.

### What's still not in scope

- **vLLM V1 engine bit-exact KV restore.** vLLM-side fix; V0 +
  `enforce_eager=True` workaround documented in v1.0.12.
- **Generic CLI model+cache layer auto-discovery.** "Walk a
  directory and produce a valid LoRA diff" is the source of most
  "I restored my agent and it half-worked" reports; the loud
  warning + adapter-populated path stays the answer.
- **Live PF_HAS_GPU=1 self-contained vLLM/SGLang test.** The
  examples now do real adapter round-trip on every host; the
  bit-exact KV validation against actual vLLM still runs on
  Modal (`scripts/gpu-validate-modal.py`).

## [1.0.13] — 2026-05-08

Closes the two confirmed bugs and the one Python SDK lineage gap
the v1.0.12 retest flagged. Independent matrix had been 10 PASS /
2 ISSUE / 3 LIMITATION; v1.0.13 turns the two ISSUEs into PASS and
makes the Python SDK lineage limitation a non-issue. The other
limitations (live GPU host, CRIU Linux-only, absolute-symlink
restore safety) are scope/environment notes, not bugs.

### Issue 1: false merge conflicts from generated test artifacts

Reproduced with `pytest`'s `__pycache__/` and `.pytest_cache/`
landing in the captured tree on otherwise-disjoint branches. The
v1.0.12 CLI had no ignore mechanism beyond hardcoded defaults
(`target/`, `node_modules/`, `.git/objects/`, `.pfcid`).

- **New default-extra ignore set** in `WalkFsCapture::new`:
  `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`,
  `.tox`, `.coverage`, `.venv`, `.DS_Store`, `*.pyc`, `*.pyo`.
  Conservative — every entry is a cache by definition, never a
  "maybe I want this" path.
- **New `--ignore <PAT>` CLI flag** (repeatable). Plain entries
  (`__pycache__`, `node_modules`) match path components like
  before; glob entries (anything containing `*`/`?`/`[`, e.g.
  `*.pyc`, `*.log`, `**/build/**`) match the relative path via
  the new `globset` dep.
- **New `--ignore-from <PATH>`** CLI flag — reads gitignore-style
  rules from a file. Default: try `<fs_root>/.pfignore` then
  `<fs_root>/.gitignore`; pass `--ignore-from /dev/null` to opt
  out. Comments (`#`) and blank lines skipped; trailing `/`
  stripped; gitignore negation (`!keep.pyc`) is logged-and-
  skipped (full negation semantics arrive when an operator
  hits the use case).
- **New `--no-default-ignores`** to opt out of the default-extra
  set (rare; CI auditing the cache shape, registry mirroring).
  CVE-relevant defaults (`.git/objects`, `target`, `node_modules`,
  `.pfcid`) are kept regardless.
- **New `WalkFsCapture::new_without_default_ignores(root)`** and
  **`.ignore_from(path)`** API on the underlying library.
- Regression coverage: 4 new unit tests in `pf-world` covering
  default-extra-ignores, glob `*.pyc` matching, opt-out, and
  `.pfignore` file parsing; 2 new CLI integration tests in
  `cli_smoke.rs` covering the snapshot end-to-end.

### Issue 2: `pf gc --retain-recent N` left dangling log entries

Reproduced: `pf log` listed CIDs after GC, but `pf checkout` on
those CIDs failed because the layer blobs were gone. Root cause:
`pf gc` deleted unreachable blobs from `blobs/sha256/<shard>/<hex>`
but never the per-manifest marker files at
`store_root/images/<cid>.json`, which is what `pf log` walks via
`store.iter_manifests()`. The result was a referential-integrity
hole: index says "this CID exists", CAS says "I have no idea".

- **Fix**: GC now tracks the set of evicted manifest CIDs and,
  after the blob sweep, deletes their `images/<cid>.json`
  markers. The output line counts both: `deleted N unreachable
  blobs (B bytes) and M stale image markers`.
- Regression test (`gc_retain_recent_prunes_image_markers`):
  snapshot 3 manifests → `pf gc --retain-recent 1` → assert
  `pf log` no longer lists the 2 evicted CIDs → assert
  `pf checkout` on an evicted CID fails AND `pf checkout` on
  the kept CID succeeds.

### Limitation 3: Python SDK didn't expose parent lineage

`processfork.snapshot_filesystem` hardcoded `parents: vec![]` so
SDK-only forks couldn't be 3-way-merged (no LCA was discoverable).
Operators had to route through the CLI's `--parent` flag.

- **New `parents: Sequence[str] | None = None`** kwarg on
  `snapshot_filesystem`. Bad CIDs surface as `ValueError`, not
  silent malformed manifests.
- Regression coverage: `test_snapshot_parents_field_lands_in_manifest`
  pins manifest.parents round-trip; `test_merge_two_forks_clean`
  upgraded from "asserts 'no common ancestor' RuntimeError" to
  "asserts the merge succeeds clean with cid_x as ancestor";
  `test_snapshot_rejects_bad_parent_cid` covers the error path.

### Versions

- `processfork` (Rust + Python wheel): 1.0.12 → **1.0.13**
- All 8 internal `pf-*` crate version pins: → 1.0.13
- npm `@processfork/sdk`: 1.0.12 → **1.0.13**
- `processfork-criu`: 1.0.12 → **1.0.13** (matches the CLI's
  `--criu-pid` baseline)

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo deny check`: clean.
- `cargo test --workspace`: **211 passed** (was 204; +7 from
  the new ignore + GC + Python-lineage integration coverage).
- `pytest crates/pf-py/python/tests/ adapters/pf-claude-code/tests/
  adapters/pf-criu/tests/`: **27 passed, 2 skipped** (CRIU
  Linux-only paths still gate-skip on macOS as documented).
- `node --test crates/pf-ts/test/smoke.mjs`: **8/8**.

### Still not in scope (auditor's "limitations", left as-is)

- Live PF_HAS_GPU=1 vLLM/SGLang test — Modal lane is the
  validation; documented in v1.0.11.
- CRIU is Linux-only by definition; macOS CI exercises Layer 1
  + the non-Linux skip paths.
- Absolute symlinks captured but rejected on restore for
  sandbox-escape safety. This is the v1.0.3 "Zip Slip"
  hardening (PF-SA-2026-001); changing it would re-open the
  CVE. Operators who need absolute-symlink restore should
  resolve the link target post-checkout in their own code.

## [1.0.12] — 2026-05-07

Closes the four "not-yet-production-ready" items v1.0.11 made
explicit. Two of them are runtime features (conflict-merge UI,
CRIU subprocess capture); two are honesty/UX (loud warnings on
empty engine layers, V1-engine workaround documented). One — V1
bit-exact KV restore — remains a vLLM-side change beyond
ProcessFork's reach and is now documented with the V0 +
`enforce_eager=True` workaround.

### `pf merge-resolve` / `pf merge-finalize` — interactive conflict resolution

- New top-level commands. Replace the v1.1-deferred placeholder
  with a real round-trip:
    1. `pf merge A B` → if conflicts, exits 3 with the resolve+
       finalize hint pointing at the merged-CID.
    2. `pf merge-resolve <merged-cid> --workdir <dir>` extracts
       the merged FS into `<dir>` (which must NOT pre-exist),
       scans for Git-style markers, and prints the conflict
       file list.
    3. Operator hand-edits.
    4. `pf merge-finalize <merged-cid> --workdir <dir>` re-walks
       the resolved tree, builds a single-parent image whose
       parent is `<merged-cid>`, returns the finalized CID.
- `pf merge-finalize` refuses if any file in `<dir>` still
  contains conflict markers (exit code 3); pass `--force` to
  finalize as-is (for tree fixtures with legitimate `<<<<<<<`
  content).
- Scan covers all three Git marker variants (`<<<<<<<`,
  `=======`, `>>>>>>>`); skips symlinks and binary files (NUL
  byte heuristic).
- Round-trip regression test (`merge_resolve_finalize_round_trip`)
  exercises: snapshot-X-A-B-with-parent → conflicting merge →
  resolve workdir → finalize-without-resolution-fails →
  hand-resolve → finalize succeeds → finalized image's
  manifest.parents == [merged-cid] → checkout shows resolved
  content with no markers. `--force` path tested separately.

### `processfork-criu` adapter + `pf snapshot --criu-pid <PID>`

- New Python package `processfork-criu` (Linux-only at runtime)
  promotes the world layer's `procs` blob from
  `procs.unsupported.v1` to `procs.criu.v1`. The bundle is a
  header-line JSON dict + raw tarball of CRIU's `images-dir`
  output, ready for `pf verify` to round-trip.
- New CLI flag `pf snapshot --criu-pid <PID>` shells out to
  `python3 -m processfork_criu` (via inline script) to perform
  the dump. On macOS / Windows / non-criu Linux hosts the
  command exits with a clear "CRIU unavailable: …" message and
  the snapshot fails fast — no silent half-state.
- Python API: `processfork_criu.dump_pid(pid, leave_running=True,
  tcp_established=False)` returns a `CriuBundle` whose
  `serialize()` is the on-disk format; `restore_bundle(bundle,
  target_dir=...)` returns the new PID after `criu restore`.
- Test layering reflects the honesty caveat (same as the Modal
  vLLM lane: code is committed, validation lives where the
  kernel lives):
    - **Layer 1 — runs on every host (8 tests):** version match,
      v1 marker constants, header+tarball envelope round-trips,
      deserialize rejects wrong-kind / missing-newline,
      `is_available()` False on macOS, `dump_pid` /
      `restore_bundle` raise clean RuntimeError on macOS.
    - **Layer 2 — Linux only, no criu needed (1 test, skips on
      macOS):** `is_available()` reflects whether `criu` is on
      `$PATH`.
    - **Layer 3 — Linux + criu binary (1 test, skips otherwise):**
      end-to-end: spawn a heartbeat-writing Python child,
      `criu dump` it, SIGKILL the original PID, `criu restore`,
      assert the restored PID writes new heartbeats. **This is
      the operator-runs-it validation; the maintainer's macOS
      CI has not run it.** README has the caveat.
- Rust-side test (`snapshot_criu_pid_fails_cleanly_on_non_linux`)
  confirms `pf snapshot --criu-pid 1` on macOS exits non-zero
  with stderr mentioning CRIU/python3 (no panic, no silent empty
  procs blob).

### Loud warning when generic CLI snapshot writes empty engine layers

- `pf snapshot` now emits a multi-line stderr warning explaining
  that the model + cache layers are empty and that engine state
  requires the vLLM/SGLang adapter to populate. World (FS+env),
  trace, and effects ARE captured.
- New `--allow-empty-engine-layers` flag suppresses the warning
  for CI/automation that has internalized the boundary.
- The empty model + cache envelopes now carry a `"note":
  "generic-cli-empty: populated by adapters, not by walking a
  directory"` field so downstream tooling can detect them.

### V1-engine bit-exact workaround documented

- `adapters/pf-vllm/README.md` gets a new "Bit-exact replay: V0
  vs V1 engine" section with the **V0 + `enforce_eager=True`**
  workaround for callers who need byte-identical regenerated
  output. Calls out the throughput cost (1.3–1.8× slower
  without CUDA graphs), V0's feature-frozen status upstream,
  and when V1 + output-equivalent is acceptable (snapshot
  before destructive change vs. RL rollout reproducibility).
- README's bit-exact metric row links to this section.

### Versions

- `processfork` (Rust + Python wheel): 1.0.11 → **1.0.12**
- All 8 internal `pf-*` crate version pins: → 1.0.12
- npm `@processfork/sdk`: 1.0.11 → **1.0.12**
- New `processfork-criu` Python package: **1.0.12**

### Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo deny check`: clean.
- `cargo test --workspace`: 204 passed (was 199; +5 from the
  merge-resolve / merge-finalize / criu-pid integration tests).
- `pytest crates/pf-py/python/tests/ adapters/pf-claude-code/tests/
  adapters/pf-criu/tests/`: 25 passed, 2 skipped (CRIU Linux-
  only paths).
- `node --test crates/pf-ts/test/smoke.mjs`: 8/8.
- `pf snapshot --criu-pid 1` on macOS: exits non-zero with
  "CRIU is Linux-only" — no panic.

### What's still not in scope

- **vLLM V1 engine bit-exact KV restore.** Documented workaround
  (V0 + `enforce_eager`); upstream V1 deterministic batch
  scheduling is the actual fix and lives in vllm/.
- **Generic CLI model/cache layer auto-discovery.** The "walk a
  directory and produce a valid LoRA diff" approach is the
  source of most "I restored my agent and it half-worked"
  reports; we keep the empty-envelope-with-loud-warning path
  instead.

## [1.0.11] — 2026-05-07

Documentation honesty pass. The v1.0.10 retest confirmed 12/12 of the
real-world matrix (FS snapshots, env redaction, HMAC ledger tamper
detection, 12 forks at 1.004× storage, clean+conflict merges, file://
registry, GC, symlink hardening, quiesce/resume, large binaries) but
flagged that the README's "vLLM/SGLang ✅ ships now / bit-exact KV"
framing and the example/test stubs labelled "v1.0.1 deferred
deliverable" did not match what actually shipped.

This release does **not** change runtime behavior. All earlier audit
fixes still stand. What changes:

### README: adapter status table now distinguishes mock vs. live

- Claude Code / LangGraph / OpenInterpreter / AutoGen / CrewAI keep
  ✅ — they snapshot/restore the FS + env + trace + effects layers
  and the auditor's matrix exercised them end-to-end.
- vLLM / SGLang downgraded from ✅ to **🟡 mock ships v1.0 · live =
  Modal lane**. The mock K/V page round-trip ships and is regression-
  tested; the bit-exact validation runs on Modal A10G via
  `scripts/gpu-validate-modal.py`, not from your local box.

### README: 5-layer table now marks adapter-populated layers

- **World** annotated: FS + env ship; the `procs` blob writes a
  `procs.unsupported.v1` placeholder unless a CRIU/zombie-restart
  adapter is added (a v1.1 deliverable). Restored sessions do not
  bring back live PIDs; they bring back the FS+env+trace+effects
  state that lets a fresh worker continue.
- **Model** and **Cache** annotated 🟡: format + math ship and run
  on the Modal lane, but the **generic CLI snapshot path emits
  empty envelopes** because these layers are populated by adapters
  (vLLM/SGLang/etc.), not by walking a directory.

### README: bit-exact KV claim split V0 vs V1

- v1.0.10 had one row claiming "Bit-exact KV-cache replay ✅ verified".
  The Modal JSONs say something more specific:
  - `2026-05-06-modal-a10g.json` (V0 engine, TinyLlama-1.1B):
    `bit_exact: true`, 38 619 KV pages, byte-identical regen text.
  - `2026-05-06-modal-a10g-vllm-v1.json` (V1 engine, `collective_rpc`):
    `bit_exact: false`; first-80-chars of regen output match across
    snapshot/restore (output-equivalent, not bit-exact).
- README now has both rows, each linking to the source-of-truth JSON.
  Treat live V1 KV restore as "lossy semantic restore" today.

### README: new "What does and doesn't ship in v1.0.x" subsection

- Production-credible today (auditor's 12/12 matrix): pf snapshot/
  checkout for FS sandboxes; default secret-shaped env redaction
  (CLI + Python SDK + TS SDK); HMAC-chained effects ledger end-to-end
  with `pf verify` tamper detection; fork & merge incl. conflict
  marker materialization; file:// + OCI + S3 + HF registry transport;
  5 first-party adapters; vLLM/SGLang mock-mode K/V page persistence.
- Not yet production-ready, made explicit: in-flight subprocess
  capture (CRIU adapter is v1.1); local PF_HAS_GPU=1 self-contained
  vLLM/SGLang test (it was always Modal-lane validation, the
  examples/06+07 + cache_bit_exact_vllm.rs were skeletons mislabelled
  "v1.0.1 deferred"); V1-engine bit-exact KV restore (output-
  equivalent only); conflict-merge resolution UI (markers ship,
  interactive `pf merge --resolve` is v1.1); generic CLI model+cache
  layer capture (adapter-populated only).

### Skeleton/stub messages updated

- `examples/06-vllm-bit-exact/run.sh` and
  `examples/07-sglang-prefix-share/run.sh`: removed the misleading
  "v1.0.1 deferred deliverable" pointer; both now point at
  `modal run scripts/gpu-validate-modal.py` and the JSONs in
  `benchmarks/gpu-validation/`, which is the actual validation path.
- `crates/pf-cache/tests/cache_bit_exact_vllm.rs`: previously
  `panic!("PF_HAS_GPU=1 set but pf-vllm adapter not yet wired")`.
  Now skips cleanly under any value of `PF_HAS_GPU` and points at
  the Modal lane + `tests/cache_round_trip.rs` (the on-host proxy
  that DOES exercise the cache code path everywhere).

### Versions

- `processfork` (Rust + Python wheel): 1.0.10 → **1.0.11**
- All 8 internal `pf-*` crate version pins: → 1.0.11
- npm `@processfork/sdk`: 1.0.10 → **1.0.11**

### Why this matters

The runtime behavior in v1.0.10 was correct and the auditor's matrix
agreed. The README and a handful of stub messages were overselling.
Documentation that can't be matched against `cargo test`,
`benchmarks/gpu-validation/*.json`, or the example runners is the
same kind of trust hole as a code bug — operators who read the README
were going to spend a day chasing a "ships now" GPU validation that
the Modal lane already ran for them. v1.0.11 makes the boundary
match the reality.

## [1.0.10] — 2026-05-07

Closes the two TypeScript-SDK gaps the v1.0.9 retest flagged. v1.0.7
hardened the CLI's snapshot path; v1.0.9 propagated the fix to the
Python SDK; v1.0.10 propagates it to the TypeScript SDK. The CLI,
Python SDK, and TypeScript SDK now all go through the same scrub
regex and the same HMAC-chained `pf_effects::Ledger` code path —
parity across all three surfaces.

### Security: TS SDK env capture is no longer unsafe-by-default

- `snapshotFilesystem(store, kind, root, env, messages, opts?)` now
  applies the same default scrub regex the CLI and Python SDK use
  (`(?i)(?:^|_)(token|secret|password|passwd|pwd|api_?key|apikey|
  auth|bearer)(?:_|$)`) — env keys matching it are stored as
  `"<redacted>"`. JS callers that did
  `snapshotFilesystem(..., { OPENAI_API_KEY: "...", PWD: root })`
  were storing the raw API key bytes in `world.env` — the auditor
  reproduced the leak with two separate stores.
- New `opts.defaultScrubEnv: boolean = true` knob; pass `false` to
  opt out (rare; CI debugging at most).
- New `opts.scrubEnv: string[]` for additional regex patterns,
  mirroring the CLI's `--scrub-env` flag.
- Smoke-test fix: the prior `test/smoke.mjs` passed `new Map([...])`
  for the env arg; napi-rs serializes JS `Map` instances to `{}`
  (only plain objects deserialize to Rust `BTreeMap`), so the test
  silently received empty env and never exercised the leak path.
  Switched to plain objects (the typed signature's documented
  shape) and added 3 regression tests:
  - `default env scrub redacts secret-shaped names` — proves
    `OPENAI_API_KEY`/`GITHUB_TOKEN`/`DATABASE_PASSWORD`/`MY_API_KEY`
    redacted, AND that the secret bytes don't appear anywhere in
    the serialized env blob.
  - `defaultScrubEnv = false opts out` — proves the opt-out path.
  - `effects ledger is HMAC-chained` — see below.

### ACRFence: TS SDK ledger is HMAC-chained for real

- Prior versions of the TS SDK ALWAYS wrote
  `{"kind":"effects.ledger.v1","entries":0}\n` to `effects.ledger`
  regardless of caller intent — TS integrations had no ACRFence
  protection at all, even when they had a real tool-call list.
- New `opts.effects: EffectEntry[]` parameter; entries are routed
  through `pf_effects::ledger::Ledger::append` (per-entry
  `session_hmac = HMAC(secret, prev_hash || this_hash)`) and the
  blob comes out byte-compatible with the CLI/Python output —
  same header marker, same `session_secret_hex` embedding, same
  `verification_mode = "tamper-detection"`.
- `pf verify` validates SDK-produced ledgers through the same code
  path it already used for CLI ledgers (no `pf verify` change
  needed).
- `EffectEntry` shape (camelCase TS): `toolId`, `argsHash`,
  `resultHash`, `idempotencyKey`, `sideEffectClass` ("pure" |
  "idempotent" | "irreversible" | "network-only"), `timestamp`
  (RFC-3339; defaults to now). All fields except `toolId` optional.

### New SDK surface: `readBlob`

- `readBlob(store, digest): Buffer` — fetches raw blob bytes by
  digest. Mirrors the Python SDK's `processfork.read_blob`.
  Adapters that need to inspect individual layer blobs (e.g. the
  smoke tests verifying the redaction wrote correctly to `world.env`,
  or a future TS LangGraph checkpointer reading the trace blob)
  call this.

### Versions

- `processfork` (Rust + Python wheel): 1.0.9 → **1.0.10**
- All 8 internal `pf-*` crate version pins: → 1.0.10
- npm `@processfork/sdk`: 1.0.9 → **1.0.10**

### Why this matters

The v1.0.9 retest passed 13 of 13 real-world cases on the CLI +
Python paths but explicitly flagged the TS SDK as a *blocker*: a JS
caller using the typed signature exactly as documented was leaking
raw API keys to disk, and the TS effects ledger gave no ACRFence
protection regardless of caller intent. Both gaps are CLI/Python
fixes that hadn't been propagated to TS. They are now propagated,
with regression tests proving the exact attack patterns the auditor
reported.

## [1.0.9] — 2026-05-06

Closes the two SDK-side gaps the v1.0.8 retest flagged. v1.0.7
hardened the CLI's snapshot path; the Python SDK was never wired
to the same hardening, so adapters that called
`processfork.snapshot_filesystem(..., env=dict(os.environ))` (every
adapter in `adapters/`) re-opened the same secret-leak the CLI
audit had closed, and the SDK's effects ledger was raw JSONL with
no HMAC chain even though the CLI's was.

### Security: SDK env capture is no longer unsafe-by-default

- `processfork.snapshot_filesystem()` now applies the same default
  scrub regex the CLI uses (`(?i)(?:^|_)(token|secret|password|
  passwd|pwd|api_?key|apikey|auth|bearer)(?:_|$)`) — env keys
  matching it are stored as `"<redacted>"`. Operators who genuinely
  need the raw env (rare; CI debugging at most) opt out via
  `default_scrub_env=False`.
- New `scrub_env: Sequence[str] | None = None` parameter for extra
  custom regex patterns, mirroring the CLI's `--scrub-env` flag.
- All 5 first-party adapters (Claude Code, LangGraph, OpenInterpreter,
  AutoGen, CrewAI) inherit the safe default automatically — none of
  them ever passed `default_scrub_env=False` to start with.
- Regression tests: `test_default_scrub_redacts_secret_shaped_env`
  asserts that `OPENAI_API_KEY`, `GITHUB_TOKEN`, `DATABASE_PASSWORD`,
  `MY_API_KEY` are redacted AND that the secret bytes do not appear
  anywhere in the serialized blob; `test_default_scrub_can_be_disabled`
  asserts the opt-out path still works for operators who need it.

### ACRFence: SDK ledger is HMAC-chained for real

- `processfork.snapshot_filesystem(..., effects=[...])` now routes
  every entry through `pf_effects::ledger::Ledger::append`, computing
  per-entry `session_hmac = HMAC(secret, prev_hash || this_hash)` —
  the same code path the CLI's `--effects-from-jsonl` was switched
  to in v1.0.7. Prior versions stuffed the entries into a raw JSONL
  blob with no HMAC at all, so tamper / reorder / delete on the
  on-disk blob was undetectable.
- A per-snapshot session secret is generated by default and embedded
  in the blob header (tamper-detection mode); operators who want full
  ACRFence supply `PF_SESSION_SECRET=<hex>` and the secret stays out
  of the blob.
- `pf verify` already recognizes the embedded-secret format from
  v1.0.7 — SDK-produced blobs and CLI-produced blobs verify through
  the same code path now.
- Regression test: `test_effects_ledger_is_hmac_chained` asserts
  the v1 header marker, the embedded session-secret-hex, and that
  every entry has a non-empty `session_hmac` ≥32 chars (catching
  the prior raw-JSONL `session_hmac=""` regression).

### Versions

- `processfork` (Rust + Python wheel): 1.0.8 → **1.0.9**
- All 8 internal `pf-*` crate version pins: → 1.0.9
- npm `@processfork/sdk`: 1.0.8 → **1.0.9**

### Why this matters

The v1.0.8 audit retest passed 10 of 12 real-world cases but flagged
two genuine production blockers: (1) the SDK still leaked secret-shaped
env vars by default, and (2) SDK effects were raw JSONL not
HMAC-chained. Both are CLI-side fixes that hadn't been propagated
into pf-py. They are now propagated, with regression tests proving
both paths and confirmation that all 5 adapters inherit the safe
defaults.

## [1.0.8] — 2026-05-06

Closes the **5th and final** finding from the v1.0.6 audit — every
round-5 production-blocker is now resolved end-to-end.

### Security: cargo-audit advisory ignores cleared

- **pyo3 0.22 → 0.24** (`RUSTSEC-2025-0020`, PyString::from_object
  buffer-overflow). The `IntoPy::into_py` API is deprecated in 0.24;
  `pf-py`'s json↔PyObject converter and the `merge` report
  constructor were migrated to `IntoPyObject::into_pyobject(...)?
  .into_any().unbind()`. Builds clean under `cargo clippy --workspace
  --all-targets -- -D warnings`.
- **rustls-webpki 0.101.7 → 0.103.13** (`RUSTSEC-2026-0098`,
  `-0099`, `-0104`). Root cause was the `rustls` feature on
  `aws-config` / `aws-sdk-s3`, which routes through
  `aws-smithy-runtime/tls-rustls` →
  `aws-smithy-http-client/legacy-rustls-ring` and pins rustls 0.21.
  Switched to the `default-https-client` feature, which routes
  through `aws-smithy-http-client/rustls-aws-lc` (rustls 0.23 +
  aws-lc-rs). `cargo tree -i rustls-webpki` now lists only `0.103.13`
  — no more legacy rustls in the dep tree.
- `deny.toml` ignore list dropped from 5 IDs → 1 (only the unrelated
  `RUSTSEC-2025-0119` for `number_prefix` unmaintained-warning
  remains, transitive via `indicatif`'s progress bars). `cargo deny
  check` reports `advisories ok, bans ok, licenses ok, sources ok`.

### Versions

- `processfork` (Rust + Python wheel): 1.0.7 → **1.0.8**
- All 8 internal `pf-*` crate version pins: → 1.0.8
- npm `@processfork/sdk` was already at 1.0.8 from the prior cycle.

### Why this matters

v1.0.7 shipped with a footnote: "round-5 finding #4 tracked for
v1.0.8." That note is gone. `cargo deny check advisories` now passes
without any RUSTSEC ignores in the AWS / pyo3 chains; the only
remaining ignore is a stylistic warning on a transitive progress-bar
dependency.

## [1.0.7] — 2026-05-06

Closes 4 of 5 production-blocker findings from the v1.0.6 audit
(round 5). Audit's 5th finding (cargo-audit advisories on pyo3 0.22
+ rustls-webpki 0.101) is documented and tracked for v1.0.8 — see
"Out of v1.0.7" below.

### Security: env capture is no longer unsafe-by-default

- `pf snapshot` runs a built-in regex (`(?i)token|secret|password|
  passwd|pwd|api_?key|apikey|auth|bearer`) that redacts secret-shaped
  env-var names UNLESS the operator passes `--no-default-scrub`.
  v1.0.6 captured every env var by default — operators with
  `OPENAI_API_KEY` / `GITHUB_TOKEN` / etc. in scope leaked them
  into the .pfimg unless they remembered `--scrub-env`. 1
  regression test (`OPENAI_API_KEY` + `DATABASE_PASSWORD` redacted,
  non-secret var preserved).

### ACRFence: ledger writes are HMAC-chained for real

- The CLI's `--effects-from-jsonl` write path (and the snapshot
  internal path) now route every entry through
  `pf_effects::ledger::Ledger::append`, which computes a per-entry
  `session_hmac = HMAC(secret, prev_hash || this_hash)`. v1.0.6
  wrote raw JSONL with `session_hmac = ""`, so tamper / reorder /
  delete on the on-disk blob was undetectable.
- A per-snapshot session secret is generated by default and
  embedded in the blob header (tamper-detection mode). Operators
  who want full ACRFence supply `PF_SESSION_SECRET=<hex>` env var,
  in which case the secret is NOT echoed back into the blob.
- `pf verify` now walks every manifest's effects ledger, runs
  `Ledger::deserialize` + `verify()`, and fails if the HMAC chain
  is bad. 1 regression test (snapshot 2 entries → tamper one
  entry's tool_id on disk → `pf verify` fails).

### vLLM / SGLang plugins now actually persist

- `_snapshot` writes every K/V page byte buffer + the per-snapshot
  manifest into a real ProcessFork store via the new SDK
  `processfork.put_blob()`. v1.0.6's hash was computed but never
  stored — the returned CID resolved to nothing on disk.
- `_checkout` now reads the manifest from the store and replays
  every page via `pager.write_page()`. v1.0.6 just returned
  `{"ok": true}` without any work.
- New SDK surface: `processfork.put_blob(store, bytes) -> str`.
- Persistence works in both mock and live modes — the `_live()`
  gate that used to short-circuit was a usability filter, not a
  correctness one, and made the persistence path untestable
  without a real GPU. 4 new regression tests (vLLM + SGLang ×
  mock-mode + persistence-round-trip + unknown-CID-errors).

### Versions aligned across surfaces

- `processfork` (Rust + Python wheel): 1.0.6 → **1.0.7**
- `processfork-vllm`: 1.0.2 → **1.0.3** (real persistence)
- `processfork-sglang`: 1.0.2 → **1.0.3** (real persistence)
- `@processfork/sdk` (npm): 1.0.7 → **1.0.8**
- 8 Rust crates on crates.io: all → **1.0.7**

### Test count

196 → 199 cargo tests workspace-wide (+1 ledger HMAC tamper, +1
default-scrub, +1 quiesce-failure regression already in v1.0.6).
Plus 4 new vLLM/SGLang persistence regressions in adapters.

### Out of v1.0.7 → tracked for v1.0.8

- **`cargo audit` ignores remain**: `pyo3 0.22.6` (RUSTSEC-2025-0020,
  buffer-overflow in `PyString::from_object` we don't call) and
  three `rustls-webpki 0.101.7` advisories (transitive via
  `aws-sdk-s3` → `aws-smithy-http-client` → `rustls 0.21`) are
  still in `deny.toml`'s `ignore` list. Clearing them needs
  pyo3 → 0.24 (Bound API rewrite, ~30 min mechanical) and
  aws-sdk-s3 ≥1.135 (when it bumps its rustls floor, likely Q3
  2026). Each ignore has a documented scope-of-impact comment;
  none are exploitable in our use cases. v1.0.8 ships the bumps.

## [1.0.6] — 2026-05-06

Closes 2 follow-up findings from the v1.0.5 audit (round 4).

### Correctness fixes

- **OpenInterpreter `result_hash` collision** (real bug). v1.0.5
  truncated the result string to 8 KiB BEFORE computing the hash,
  so two large outputs that diverged past byte 8192 collided.
  Fixed: `run()` now serializes the FULL output once, hashes those
  bytes (storing the hash in the ledger entry), and truncates only
  the displayed `result` field. The truncation suffix advertises
  the dropped byte count. Snapshot path prefers the pre-computed
  `result_hash`. 1 regression test that constructs two outputs
  sharing the first 9 KiB but diverging in the tail.

- **`--resume-cmd` not running on quiesce-cmd failure**. v1.0.5's
  `QuiesceGuard` only stashed `resume_cmd` after a successful
  `quiesce_cmd` run, so a partial-failure quiesce (mutates app
  state, then fails) left the agent stuck in a half-quiesced state.
  Fixed: construct the guard FIRST (owns `resume_cmd`), THEN run
  `quiesce_cmd` — Rust's stack-unwind drop fires resume on the
  error-return path. Updated error message tells the operator
  resume will still run. 1 regression test verifies that a quiesce
  that touches a file then exit 7 still runs resume.

### Versions

- `processfork` (Rust + Python wheel): 1.0.5 → **1.0.6**
- `processfork-openinterpreter`: 1.0.2 → **1.0.3** (hash-before-truncate)
- `@processfork/sdk` (npm): 1.0.6 → **1.0.7**
- 8 Rust crates on crates.io: all → **1.0.6**

### Test count

196 → 197 cargo tests workspace-wide (+1 quiesce-failure regression).
Plus +1 OI prefix-collision regression in adapters.

## [1.0.5] — 2026-05-06

Closes 4 follow-up findings from the v1.0.4 audit (round 3).

### npm package fix (was a hard install blocker)

- `@processfork/sdk@1.0.5` published a tarball with **no native
  binary**, so `import` failed on every consumer. Root cause: the
  CI publish-npm step staged `.node` files into `crates/pf-ts/binaries/`,
  but the package's `files` glob is `*.node` (root only) so the
  binaries got stripped at pack time.
- Fix: stage to `crates/pf-ts/` root + new `npm pack --dry-run` gate
  that fails the publish if no `.node` matches.
- v1.0.6 of `@processfork/sdk` is the first npm release that
  actually loads on consumer machines.

### Correctness

- **OpenInterpreter result_hash**. v1.0.4 persisted ledger entries
  but the OI recorder dropped `result` on the floor, so
  `result_hash` was hashing the empty string. Fixed: `run()` now
  captures the wrapped `computer.run(...)` return value (truncated
  to 8 KiB) and the snapshot path hashes it.
- **`--quiesce-cmd` / `--resume-cmd`** for app-level transactional
  consistency. `--pause-pid` SIGSTOPs at the OS scheduler but can
  freeze a process mid-transaction; the new flags let the operator
  signal the agent (e.g. `curl -XPOST /admin/quiesce`) to enter a
  consistent state before the fs walk. RAII guard so `--resume-cmd`
  always runs — even if capture errors mid-flight. 2 regression
  tests (success + failing-quiesce-aborts).

### Type stubs

- `_pf_py.pyi`: added the `effects` parameter to `snapshot_filesystem`
  (was added at runtime in v1.0.3 but the stub didn't reflect it,
  so typed Python users got wrong editor feedback). Also added the
  `read_blob` stub and a `_EffectEntry` TypedDict for the ledger
  shape.

### Versions

- `processfork` (Rust + Python wheel): 1.0.4 → **1.0.5**
- `processfork-openinterpreter`: 1.0.1 → **1.0.2** (result_hash fix)
- `@processfork/sdk` (npm): 1.0.5 → **1.0.6** (first installable release)
- 8 Rust crates on crates.io: all → **1.0.5**

### Test count

194 → 196 cargo tests workspace-wide (+2 quiesce-cmd regressions).

## [1.0.4] — 2026-05-06

Closes 4 follow-up findings from the v1.0.3 audit (round 2).

### Correctness fixes

- **`--trace-from-jsonl` validates JSON content per line.** v1.0.3
  only validated path existence + is_file. Now each non-empty line
  must be a JSON object with string `role` + `content`; malformed
  lines fail the snapshot at fail-fast time. Same treatment was
  already in place for `--effects-from-jsonl`. 1 regression test.

- **Real snapshot quiescence via `--pause-pid <pid>`.** APFS clone
  alone gives a stable FS view but doesn't prevent torn-state
  captures from concurrent multi-file agent writes (audit
  reproduced `a.txt v1, b.txt v0`). New flag SIGSTOPs the agent
  for the duration of the fs walk and SIGCONTs on Drop (RAII
  guard so the agent always resumes — even if the snapshot path
  errors out mid-capture). Unix only.

- **Adapter recorders now persist tool calls into the effects
  ledger.** v1.0.3 wired the SDK `effects=` hook; v1.0.4 actually
  uses it from:
  - `processfork_claude_code.SessionRecorder.snapshot()`
  - `processfork_openinterpreter.WrappedInterpreter.snapshot()`
  - `processfork_autogen.ProcessForkRuntime.snapshot()`

  Each adapter now derives `args_hash`, `result_hash`,
  `idempotency_key`, and `side_effect_class` per recorded tool
  call and folds them into the on-disk `effects.ledger.v1` blob.
  The ACRFence "won't double-send your email" claim has a real,
  testable surface end-to-end. 1 regression test asserts a
  recorded `Read` call lands as exactly 1 ledger entry with the
  right shape.

- **`examples/02-cli-snapshot/run.sh` updated.** The trailing
  "expected exit 2 from `pf push hf://`" demo was stale (HF has
  been live since v1.0.2). Replaced with a real `pf push file://`
  + `pf clone file://` round-trip that runs end-to-end.

### Versions

- `processfork` (Rust + Python wheel): 1.0.3 → **1.0.4**
- `processfork-claude-code`: 1.0.0 → **1.0.1** (recorder → ledger)
- `processfork-openinterpreter`: 1.0.0 → **1.0.1** (recorder → ledger)
- `processfork-autogen`: 1.0.0 → **1.0.1** (recorder → ledger)
- `@processfork/sdk` (npm): 1.0.4 → **1.0.5**
- 8 Rust crates on crates.io: all → **1.0.4**

### Test count

193 → 194 cargo tests workspace-wide (+1 trace-jsonl validation).
Plus 24/24 adapter tests (Claude Code + OpenInterpreter + AutoGen +
LangGraph), with 1 new ledger-population regression in pf-claude-code.

## [1.0.3] — 2026-05-06 — security release

Closes 10 production-readiness blockers from an independent v1.0.2
audit. **Two are CVE-class — see `SECURITY.md` for advisories.**

### Security (CVE-class)

- **PF-SA-2026-001** path traversal on checkout (Zip Slip). Malicious
  `.pfimg` could write outside the target dir via `..` paths or
  absolute paths or unchecked symlink targets. Fix: `safe_join()` +
  `check_symlink_target()` in `pf_world::fs`. 3 regression tests.
- **PF-SA-2026-002** env-var secret leakage. `pf snapshot` captured
  `std::env::vars()` verbatim — secrets included — because the
  documented `--scrub-env <regex>` flag was never wired to the CLI.
  Fix: `--scrub-env` now plumbed through CLI + Python SDK. 1
  regression test.

### Correctness — high impact

- **GC walks the transitive blob DAG.** `pf gc --retain-recent N`
  no longer deletes file blobs nested inside retained manifests'
  FsTree (or page blobs nested inside the cache PageManifest). 1
  regression test exercises the audit's exact scenario.
- **Executable file modes survive checkout.** Captured mode bits are
  reapplied via `set_permissions`. 1 unix regression test (0755
  in → 0755 out).
- **Fork → edit → snapshot → merge now works.** `pf checkout` writes
  a `.pfcid` sentinel into the restored tree; `pf snapshot`
  autodetects it as the parent CID. Explicit `--parent <CID>` flag
  also added. 1 end-to-end regression test.
- **Effects ledger is actually persisted.** `pf snapshot
  --effects-from-jsonl <path>` and `processfork.snapshot_filesystem(...,
  effects=[...])` fold tool-call entries into the on-disk ledger
  instead of always emitting an empty header. Adapters can now
  populate the ledger and the ACRFence "won't double-send your
  email" claim has a real surface.

### Correctness — medium

- **Ignore rules match path components, not substrings.** Bare
  ignore `target` no longer drops `src/targeted/keep.txt`; multi-
  segment ignores like `.git/objects` still match consecutive
  component runs. 1 regression test.
- **`--trace-from-jsonl` validated at snapshot time.** Missing /
  non-file paths fail fast with a clear error instead of silently
  capturing an empty trace that breaks `pf merge` later. Same
  validation also applied to the new `--effects-from-jsonl`. 1
  regression test.
- **LangGraph `checkpointer.get()` returns real state.** Previously
  returned a `{"_manifest": ...}` placeholder; v1.0.3 reads the
  trace blob via the new SDK `read_blob()` and reconstitutes the
  original state dict end-to-end. 1 regression test.
- **`pf snapshot` quiesces via APFS clone on macOS.** `WalkFsCapture`
  now opts into the `clonefile(2)` fast-path by default on macOS,
  giving a stable read-snapshot so concurrent agent writes don't
  produce torn states. Closes the audit's "a.txt v1, b.txt v0"
  finding on macOS hosts.

### New SDK surface

- `processfork.read_blob(store, digest) -> bytes` — fetch a blob
  by digest. Used by the LangGraph adapter; useful for any operator
  who needs to inspect layer contents without `pf` shelling out.

### MSRV

- Cargo workspace MSRV is **1.91** (unchanged from v1.0.2).

### Versions

- `processfork` (Rust + Python wheel): 1.0.2 → **1.0.3**
- `processfork-langgraph`: 1.0.0 → **1.0.1** (real `get()` semantics)
- `@processfork/sdk` (npm): 1.0.3 → **1.0.4**
- 8 Rust crates on crates.io: all → **1.0.3**
- Other 6 adapters unchanged at 1.0.0–1.0.2 (skip-existing handles them)

### Test count

184 → 193 cargo tests (workspace), +9 audit-fix regressions. Plus 6
LangGraph adapter tests (was 5, +1 for the audit fix).

## [1.0.2] — 2026-05-06

The "close every megaprompt gap" release. 12 spec items closed, 4
new GPU validation lanes, and the §M4 registry quartet all live.

### Registry adapters (§M4 — was 1/4 stub, now 4/4 live)

- **HF Hub** (`hf://`): real adapter against HF's commit API. Push
  ensures-repo + one batched commit (manifest + sig + every blob).
  Pull walks the tree and verifies digests. 3 wiremock round-trip
  tests, 4 unit tests.
- **OCI Distribution** (`oci://`): real Distribution Spec v2
  client. Push uses content-addressed dedupe (HEAD-skip), then
  monolithic upload (POST → PUT). Manifest mediatype
  `application/vnd.processfork.image.v1+json`. 2 wiremock + 4 unit.
- **S3 / R2 / MinIO** (`s3://`): aws-sdk-s3 backed (per spec §7).
  Honours `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` /
  `AWS_REGION` / `AWS_ENDPOINT_URL`. 2 wiremock + 4 unit.
- **IPFS / Kubo** (`ipfs://`): full multipart `/api/v0/add` →
  `object/new` → `object/patch/add-link` → `pin/add` chain. 2
  wiremock (with content-addressed fake CIDs) + 4 unit.

All four ship in the default build; `--no-default-features` keeps
the air-gapped FileRegistry-only path.

### Adapter live FFIs

- **SGLang live FFI** (1.0.1 → 1.0.2): mirrors the v1.0.1 vLLM
  pattern. Drives `scheduler.token_to_kv_pool.k_buffer/v_buffer`
  for real on `PF_HAS_GPU=1`. Mock mode round-trips byte-identical.
- **vLLM V1 engine** (1.0.1 → 1.0.2): adds the `collective_rpc`
  path for vLLM ≥0.10's subprocess-worker architecture
  (`worker.model_runner.kv_caches`). Module-level `_v1_*` helpers
  get pickled and shipped to each worker. Operator must set
  `VLLM_ALLOW_INSECURE_SERIALIZATION=1`. End-to-end validated on
  Modal H100/A10G with vLLM 0.20.x V1: 38,599 KV pages
  snapshotted + restored, first 80 chars of regenerated output
  byte-identical (full bit-exact in V1 awaits upstream
  deterministic mode → v1.0.3).

### World layer (§M2)

- **Browser DOM via CDP**: new `pf_world::browser::BrowserCapture`
  connects to a Chromium `--remote-debugging-port` and captures
  per-page MHTML + viewport + scroll + localStorage +
  sessionStorage + cookies via `tokio-tungstenite` WebSocket.
  Wire format: `BrowserBlob::{Cdp{...}, Unsupported{...}}`. 4
  unit tests.

### Validation lanes

- **TIES + DARE byte-exact mergekit parity**: pf-model spec
  re-implemented in numpy and compared element-wise to mergekit
  reference TIES on 2048×2048 fp32 × 3 deltas. **max\|delta\| = 0.0**
  (tolerance 1e-3). Standalone `tools/check_mergekit_parity.py`
  works in a venv that doesn't have vLLM (mergekit pins
  pydantic 2.4 incompatible with vLLM 0.20+'s pydantic 2.12).
- **Llama-3-8B H100 lane**: `validate_llama8b` Modal function
  on H100 + 80 GB VRAM. Operator-runs-it via
  `modal run scripts/gpu-validate-modal.py::llama8b` with
  `huggingface` Modal secret.
- **PFBench thesis**: real OpenAI + Anthropic clients in
  `benchmarks/pfbench/model_clients.py` (`load("openai:gpt-4o")` /
  `load("anthropic:claude-opus-4-7")`). 5-task pilot subset +
  full operator runbook for the ≥15pp SWE-Bench Verified claim.

### Quality

- **Coverage gate (§M8)**: `coverage/baseline.json` records
  88.96% line / 88.37% region / 78.31% function. CI workflow
  fails the build if line coverage drops below 85%.
- **Asciinema demo recording (§M9)**: `demo/processfork-demo.cast`
  + rendered `demo/processfork-demo.gif` (290 KB) embedded in
  README hero block.

### Versions

- `processfork` (Rust + Python wheel): 1.0.1 → 1.0.2
- `processfork-vllm`: 1.0.1 → 1.0.2 (V1 engine support)
- `processfork-sglang`: 1.0.0 → 1.0.2 (live FFI)
- `@processfork/sdk` (npm): 1.0.2 → 1.0.3
- 8 Rust crates on crates.io: all → 1.0.2
- Other 5 adapters unchanged at 1.0.0 (skip-existing handles them)

### Out of v1.0.2 (deferred to v1.0.3)

- Full vLLM V1 bit-exact (waits on upstream deterministic mode)
- SWE-Bench Verified ≥15pp thesis number (operator API budget)
- Llama-3-70B at 380K-token context (needs 2× H100 NVLink + YaRN)

## [1.0.1] — 2026-05-05

Cross-platform wheels and the live vLLM bit-exact KV-cache integration.

- **Wheels**: `processfork` now ships a wheel for every PyPI
  platform tier — macOS arm64 + macOS x86_64 + Linux x86_64
  (manylinux_2_28) + Linux aarch64 (manylinux_2_28) + Windows x86_64
  (cross-compiled on the build host via `pyo3 = ["generate-import-lib"]`).
- **vLLM live FFI**: `adapters/pf-vllm/processfork_vllm/plugin.py`
  drives `vllm.worker.cache_engine.gpu_cache` for real, gated on
  `PF_HAS_GPU=1`. Bit-exact restore against `--enforce-deterministic`
  Llama-class workers. Mock mode (no engine) still gives a byte-
  identical write→read round-trip for unit tests.
- **PyPI Trusted Publishing**: `.github/workflows/release.yml`
  replaces `PYPI_API_TOKEN` with OIDC trust to GitHub via
  `pypa/gh-action-pypi-publish`. The `publish-pypi-core` and
  `publish-pypi-adapters` jobs both pull from the `pypi` deployment
  environment so PyPI's trust policy can be scoped to it.
- `processfork-vllm` bumped to `1.0.1`.

## [1.0.0] — 2026-05-05

The initial public release. Twelve build phases, 200+ tests across
Rust + Python + TypeScript surfaces, all four layers shipped, all
seven first-party adapters present (three end-to-end on the build
host, four scaffolded with explicit v1.0.1 milestones).

### Phase 12 — release

- Workspace + SDK package versions bumped from `0.1.0-dev` /
  `0.1.0.dev0` to `1.0.0`.
- `.github/workflows/release.yml`: full multi-platform release
  pipeline. On a `v*.*.*` tag push:
  - Cross-builds the `pf` binary for ubuntu-24.04 (x86_64 + arm64)
    and macos-14 (arm64).
  - Cosign-signs each binary keylessly via Sigstore.
  - Publishes a GitHub Release with binaries + signatures + SHA-256s
    + the latest `CHANGELOG.md` section as the release notes.
  - Publishes the 8 publishable Rust crates to crates.io in
    dep-order (`cargo publish`).
  - Publishes the `processfork` wheel + the 7 adapter pure-Python
    pkgs to PyPI (`maturin build` + `twine upload`).
  - Publishes `@processfork/sdk` to npm (`napi build` + `npm
    publish`).
  - Builds + pushes the multi-arch Docker image to
    `ghcr.io/manav8498/processfork:<tag>` + `:latest`.
- `Dockerfile`: 2-stage build producing a slim Debian-based image
  with the `pf` binary on PATH; mounts `/data/store` as a volume.
- `landing/`: single-page Tailwind landing site at `landing/index.html`
  ready for GitHub Pages from the `/landing` directory; ~8 KB HTML
  + 80 KB Tailwind JIT.
- `demo/script.sh`: 60-second viral-demo recording script that
  runs end-to-end on a laptop today (snapshot → 12-fork → merge →
  push to file:// → fresh-store clone → restored). Verified against
  the built binary.
- `demo/script.cast.md`: operator-runs-it instructions for
  asciinema recording + agg conversion to GIF/SVG.

### Added — Phase 11 (benchmarks + tests + docs)

**Microbench**
- `benchmarks/microbench/` — Criterion crate added to the workspace
  with two benches:
  - `snapshot_synthetic_4layer`: 4-layer atomic snapshot orchestrator
    against the default fixture. **Observed: 7.9 ms median** (budget
    500 ms; 63× headroom).
  - `cache_capture_64_pages` + `cache_restore_64_pages`: paged-KV
    serialise/deserialise hot path. **531 µs / 34 µs**.
- `benchmarks/RESULTS.md` published with reproducible commands +
  the build-host numbers + the operator-runs-it template for the
  GPU lane.

**PFBench**
- `benchmarks/pfbench/harness.py` — operator-runs-it harness with
  built-in `equals` / `contains` / `regex` graders + a built-in
  `echo` model so the harness is self-test-able in CI without any
  API keys. Self-test green: 3 tasks × 2 variants → 100 % pass.
- `benchmarks/pfbench/aggregate.py` — Markdown table aggregator over
  one or more results JSONLs.

**Documentation site**
- `docs/book.toml` + `docs/src/` mdBook source covering:
  introduction, install, first-fork tutorial, the 60-second demo,
  architecture overview, all four layer pages, merge protocol,
  `.pfimg` format, performance budget, security model, CLI
  reference, Python / TypeScript / Rust SDK refs, all 7 integration
  adapters, performance tuning, benchmarks index, migration guide,
  contributing, security policy, release checklist, changelog.
- README polished with the actual runnable 60-second demo (matching
  `examples/02-cli-snapshot/run.sh`) at the top.

**Test totals after Phase 11**

- 154 Rust tests (unchanged; microbenches are `cargo bench`, not
  `cargo test`)
- 5 Python SDK + 5 TypeScript SDK smoke tests
- 36 adapter smoke tests + 2 GPU-gated skips
- = **200 tests across the workspace**, plus
- 1 PFBench self-test (3 tasks × 2 variants = 6 grading rows)
- 2 Criterion bench suites (snapshot + cache round-trip)

### Added — Phase 10 (integration adapters)

All seven first-party adapters from `agent_docs/feature-spec.md` M5 ship
as their own pure-Python packages under `adapters/<name>/`. Three are
fully wired end-to-end against the Phase-7 SDK; four scaffold the
trait + URL parsing + auth-token plumbing with `NotImplementedError`
on the GPU/network paths until v1.0.1 lands them.

**Fully wired (build-host testable)**

- `adapters/pf-claude-code/` — `processfork-claude-code` Python pkg.
  `SessionRecorder` accumulates messages + tool calls and snapshots
  via the SDK; `ToolClassifier` provides safe-by-default tool →
  side-effect-class mapping (unknown tools → `Irreversible`);
  `install_slash_commands` drops `/snapshot`, `/fork`, `/merge`
  command files into `~/.claude/commands/processfork/`. The
  `pf-wrap-claude` CLI installs them. **9 smoke tests + runnable
  example 03**.
- `adapters/pf-langgraph/` — `processfork-langgraph` Python pkg.
  `ProcessForkCheckpointer` implements the duck-typed
  `BaseCheckpointSaver` surface (no hard `langgraph` dep at import);
  every checkpoint becomes a `.pfimg`. `fork_thread` shells out to
  `pf fork` for manifest-level branching. **5 smoke tests + runnable
  example 04 (3 checkpoints + 4 forks via real CLI)**.
- `adapters/pf-openinterpreter/` — `processfork-openinterpreter` pkg.
  `WrappedInterpreter` adds `snapshot(name)` / `checkout(name)` to
  any OpenInterpreter-shaped object; `wrap_interpreter` factory.
  Tool calls tap an in-memory ledger. **5 smoke tests + runnable
  example 05 (snapshot → destructive op → checkout restored
  byte-identical)**.

**Scaffolded (trait + auth + clear-error stubs; v1.0.1 wires the live FFI)**

- `adapters/pf-vllm/` — `processfork-vllm` pkg. `VllmCachePager`
  implements the Python side of `pf-cache::CachePager`; `VllmPlugin`
  registers `/v1/processfork/{snapshot,fork,checkout,merge}` HTTP
  handlers. Live FFI into vLLM's `worker.cache_engine` deferred to
  v1.0.1; current handlers return `501` with a clear pointer.
  **5 smoke tests + 1 GPU-gated test (skips without `$PF_HAS_GPU=1`)
  + runnable example 06 (skip-aware)**.
- `adapters/pf-sglang/` — `processfork-sglang` pkg. Sister
  implementation to vLLM, mapping onto SGLang's `mem_pool` /
  `RadixCache`. **4 smoke tests + 1 GPU-gated test + example 07**.
- `adapters/pf-autogen/` — `processfork-autogen` pkg.
  `ProcessForkRuntime` tracks per-agent message + tool-call state;
  `snapshot` flattens with `[agent]` attribution prefixes; `fork`
  shells out to `pf fork`. **4 smoke tests** (1 dep-gated on `pf` on
  PATH).
- `adapters/pf-crewai/` — `processfork-crewai` pkg.
  `ProcessForkMemory` implements CrewAI's memory protocol; every
  `save()` becomes a snapshot, `checkout(cid)` restores the world
  layer. **4 smoke tests**.

**Examples** (all 8 from `agent_docs/feature-spec.md` M9 now present):

- `examples/01-hello-fork/` (Phase 1) — synthetic 4-layer snapshot.
- `examples/02-cli-snapshot/` (Phase 8) — full CLI transcript.
- `examples/03-claude-code-fork/` — Claude Code adapter end-to-end.
- `examples/04-langgraph-checkpoint/` — checkpointer + 4-way fork.
- `examples/05-openinterpreter-undo/` — destructive-op undo round-trip.
- `examples/06-vllm-bit-exact/` — skip-aware GPU-gated harness.
- `examples/07-sglang-prefix-share/` — skip-aware GPU-gated harness.
- `examples/08-rl-rollout-fabric/` — N-way fan-out + winner merge,
  pure synthetic-fixture (runs on build host).

**Test totals after Phase 10**

- 154 Rust tests
- 5 Python SDK + 5 TypeScript SDK smoke tests (Phase 7)
- 36 adapter smoke tests + 2 GPU-gated skips (Phase 10)
- = **200 tests across the workspace**

### Added — Phase 9 (registry)

- `pf-registry::ImageRef`: parser for the five supported URL schemes —
  `file://`, `hf://`, `s3://`, `ipfs://`, `oci://`. Tags split correctly
  even when they collide with `host:port` syntax (oci) or `user/repo`
  (hf). 8 unit tests cover the round-trips + bad-scheme + missing-repo
  errors.
- `pf-registry::Registry` trait + `pf-registry::LayerSet`. Async via
  `async-trait`. Push uploads the manifest + every transitively-
  reachable blob; pull returns both. `RegistryError::UnsupportedScheme`
  cleanly distinguishes "feature flag off" from real backend failures.
- `pf-registry::FileRegistry`: filesystem-backed registry. Layout
  matches `agent_docs/registry-spec.md` — `manifest.json`,
  `manifest.json.sig`, `blobs/sha256/<aa>/<aabb…>.zst`. Used as the
  build-host integration test backbone; doubles as an air-gapped
  transport mechanism (`pf push file:///mnt/usb/...`).
- `pf-registry::transitive_blob_digests` walks the world FsTree to
  enumerate file blobs and the cache PageManifest to enumerate K/V page
  blobs; without this, push only mirrored the 8 top-level layer
  descriptors and `pf checkout` post-pull failed missing-blob.
- `pf-registry::sign`: cosign-shaped manifest signing. v1 ships
  `hmac-sha256` (self-signed with a default key; documented in
  `SECURITY.md` as forge-able by anyone holding the default key).
  Sigstore Fulcio (keyless) is feature-gated for v1.1.
- `pf-registry::HfRegistry`, `S3Registry`, `IpfsRegistry`: trait
  surface + URL parsing + auth-token plumbing. Live HTTP paths land in
  v1.0.1 behind their respective `*-live` feature flags.
  `pf_registry::open(image_ref, auth)` dispatches to the right adapter.
- **CLI wiring**: `crates/pf-cli/src/commands/stub.rs` (renamed
  conceptually but kept on disk for v1) now calls into `pf-registry`
  for `push`, `pull`, and `clone`. UnsupportedScheme errors map to the
  same exit-code-2 semantics as the Phase-8 stubs. Single-shot tokio
  runtime spun up per invocation.
- 8 integration tests in `crates/pf-registry/tests/registry_round_trip.rs`:
  full round-trip via FileRegistry, tampered-manifest detection,
  tampered-blob detection, two-push CoW dedup, and three "adapter
  cleanly returns UnsupportedScheme in default build" tests.
- 12 unit tests across `image_ref` and `sign`.
- 2 new CLI integ tests: `push_to_hf_exits_2_unsupported_scheme` (the
  Phase-8 stub test reworked) and `push_then_pull_via_file_registry_round_trips`
  (end-to-end CLI round-trip).

### Added — Phase 8 (CLI)

- `pf` CLI is now wired end-to-end: every subcommand from
  `agent_docs/cli-spec.md` calls into the layer crates instead of the
  Phase-0 "scaffold only" stub. Exit codes follow the spec table:
  `0` ok / `1` bad input / `2` not-yet-implemented / `3` merge conflict /
  `4` integrity failure.
- Refactored `crates/pf-cli/src/main.rs` from a single-file scaffold
  into a `commands/` module tree — one file per subcommand for
  testability.
- **Wired subcommands**:
  - `pf snapshot --agent-id <kind> --fs-root <path> [--name N] [--trace-from-jsonl PATH]`
    captures the world layer (via `pf_world::WalkFsCapture`), env (via
    `std::env::vars`), an optional JSONL trace, and stub model + cache +
    effects layers (matching the SDK's snapshot shape). Prints CID.
  - `pf fork <CID> -n <N> [--explore HINT] [--name PREFIX]` clones the
    manifest with new fingerprints and `parents = [<source>]`. CoW
    inherits all layer blobs.
  - `pf checkout <CID> --into <PATH>` calls `pf_world::restore_tree`.
  - `pf merge <FROM> --into <INTO> [--alpha 0.5] [--dare-p 0.7] [--seed N]`
    runs the Phase-6 engine with `StubSummarizer`. Exits 3 on
    `MergeOutcome::Conflicted` per the spec.
  - `pf log [--graph] [--max N]` walks `iter_manifests`, sorted newest
    first.
  - `pf diff <A> <B>` per-layer digest diff with `-`/`+` lines.
  - `pf status` shows store path, manifest count, blob bytes (+ MiB).
  - `pf gc [--retain-recent N] [--dry-run]` mark-and-sweep over
    orphaned blobs.
  - `pf verify [--deep]` re-hashes every blob via `BlobStore::get`
    (which already validates on read).
  - `pf completions <shell>` emits a `clap_complete`-generated
    script (bash / zsh / fish / powershell / elvish).
- **Stub subcommands** (Phase-9 deferred): `push`, `pull`, `clone`
  exit 2 with a clear pointer to `claude-progress.json` phase 9.
- Global flags: `--store <path>` (env `PF_STORE`, default
  `~/.processfork`), `--no-color`, `-v[vvv]`.
- 11 integration tests (`crates/pf-cli/tests/cli_smoke.rs`) using
  `assert_cmd` against the real `pf` binary, covering every wired
  subcommand + the stub exit codes + the bad-CID error path.
- `examples/02-cli-snapshot/run.sh` — runnable end-to-end demo
  exercising snapshot → status → log → snapshot → diff → checkout →
  verify → push (deferred). Exit 0 with full transcript.

### Added — Phase 7 (SDKs)

- **Python SDK (`crates/pf-py/`)** — pyo3 0.22 bindings:
  - `processfork.PfStore.open(path)` — opens a store, `~` expanded.
  - `processfork.snapshot_filesystem(store, agent_kind, fs_root, env, messages)`
    captures all four layers + trace into a single manifest.
  - `processfork.checkout_filesystem(store, cid, target_path)` restores
    the world-layer FS tree atomically.
  - `processfork.read_manifest(store, cid)` returns the manifest as a
    Python `dict`.
  - `processfork.merge(store, a, b, alpha?, dare_p?, seed?)` runs the
    full Phase-6 engine; returns
    `{merged_cid, ancestor, overall, world_conflicts, trace_summary,
     model_applied_task_arithmetic}`.
  - `processfork.digest_of(bytes)` SHA-256 helper.
  - Hand-written type stubs at
    `crates/pf-py/python/processfork/_pf_py.pyi` + `py.typed` marker so
    `mypy --strict` callers get full hints.
  - `pyproject.toml` driving `maturin build --release --features
    extension-module`. Verified end-to-end: built a wheel, installed it
    into a fresh `uv venv` (Python 3.12), and ran 5 smoke tests
    (`crates/pf-py/python/tests/test_smoke.py`) — all pass.

- **TypeScript SDK (`crates/pf-ts/`)** — napi-rs 2.16 bindings:
  - `PfStore.open(path)` (factory), `physicalBytes()`.
  - `snapshotFilesystem`, `checkoutFilesystem`, `readManifest`, `merge`,
    `digestOf` — same surface as Python.
  - `MergeReport` / `WorldConflict` / `Message` / `MergeOpts` typed
    objects via `#[napi(object)]`.
  - Auto-generated `index.d.ts` + `index.js` from `napi build --release`.
  - Thin TS wrapper at `ts/index.ts` adds JSON-parsed `readManifest`
    and a typed `Manifest` interface.
  - `package.json` configured for napi triple-resolution across
    `x86_64-linux`, `aarch64-linux`, `aarch64-darwin`, `x86_64-darwin`.
  - `tsconfig.json` for the TS wrapper. Verified end-to-end: built
    `processfork.darwin-arm64.node` (1.8 MB) and ran 5 smoke tests
    (`crates/pf-ts/test/smoke.mjs`) via `node --test` — all pass.

### Added — Phase 6 (merge engine)

- `pf-merge::ancestor::find_lca`: BFS lowest-common-ancestor walk over
  the manifest parents DAG. Trivial cases (`a == b`, ancestor relations)
  short-circuit. Multi-parent (octopus) ancestors error explicitly with
  `AncestorError::OctopusUnsupported` per `agent_docs/merge-protocol.md`.
- `pf-merge::trace`: pluggable `Summarizer` trait + `StubSummarizer`
  test impl that deterministically concatenates B's last 4 divergent
  messages. `merge_trace(blobs, A, B, X, summarizer)` reads three
  trace blobs, summarizes B's divergence, and emits a new trace =
  `A.messages + [system: <summary>]`. Returns the new digest, the
  injected summary, and a char-÷-4 token-count estimate for the
  cache-layer re-prefill UX line. Live Anthropic API call gated
  behind the `live-summarizer` feature flag.
- `pf-merge::world::merge_world`: full three-way file diff on the
  `pf_world::FsTree` format, implementing the 9-row decision table
  from `agent_docs/merge-protocol.md` §"World" — including
  delete-vs-modify resolution, add-on-both-with-same-content as clean,
  and `<<<<<<< A / ======= / >>>>>>> B`-marker conflict blobs (real
  text blobs persisted to CAS, referenced from the merged tree).
  Returns `WorldMergeOutcome { merged_fs, conflicts, clean_paths }`.
  8 unit tests cover every row of the table.
- `pf-merge::effects::merge_effects`: emits an `effects.merged.v1`
  blob that references both parent ledgers (without forging a new
  HMAC chain over a re-signed merged ledger — that would either
  require sharing per-session secrets or breaking the chain).
  Pre-computes counts so `pf merge` UX can print "B's N
  irreversible calls cached as facts" without re-walking. Honours
  `replay_with_new_key` (the per-class `--replay-effects` overrides).
- `pf-merge::model::merge_model`: variant-dispatch wrapper around
  `pf_model::ties_merge` + `pf_model::dare`. LoRA merges by
  `(layer_id, matrix)`; Full merges by parameter name; IA³ merges by
  `(layer, matrix)`; InPlaceTtt is concatenated by step_id. Trivial
  cases (one or both empty) bypass task arithmetic. Kind mismatches
  (A is LoRA, B is Full) keep A and flag `kind_mismatch=true`.
- `pf-merge::engine::merge`: the top-level orchestrator. Auto-
  discovers the LCA (or accepts an `x_hint`), runs all four layer
  merges, assembles a new manifest with `parents = [a, b]`, and
  returns `MergeReport` with per-layer `MergeOutcome`
  (`Clean | Conflicted | Skipped`) plus the aggregated overall.
- 28 unit tests (5 ancestor + 4 trace + 8 world + 3 effects + 5
  model + 3 engine) + 3 integration tests
  (`tests/merge_round_trip.rs`) exercising the engine end-to-end on
  the synthetic fork-pair fixture from Phases 1–5.

### Aligned — Phase 1 fixture

- `pf_core::fixture::FixtureWorldCapture` now emits entries matching
  the canonical `pf_world::FsTreeEntry` schema (`mode`, `kind` fields
  added) so Phase-1 fixtures flow through Phase-6 merge cleanly.
- `pf_core::fixture::FixtureEffectsCapture` now prepends the
  `effects.ledger.v1` header line and includes `session_hmac` per
  ledger entry to satisfy the Phase-3 wire format.
- `pf_core::fixture::FixtureModelCapture` now wraps its synthetic
  random bytes in a `model.diff.v1` envelope (Full delta with one
  `synth_param` f32 vector) so Phase-5's `load_diff` can read it.

### Added — Phase 5 (model layer)

- `pf-model::diff::ModelDiff`: tagged enum (`kind: lora|ia3|full|in-place-ttt`)
  with one payload per kind:
  - `LoraDelta` → list of `LoraAdapter { layer_id, matrix, rank, in_dim,
    out_dim, a, b }` with dimension-validation on store. `canonicalize()`
    sorts adapters by `(layer_id, matrix)` for digest stability.
  - `IA3Delta` → `BTreeMap<layer_id_string, BTreeMap<matrix_name, scaling_vec>>`.
  - `FullDelta` → `BTreeMap<param_name, dense_delta>`.
  - `InPlaceTttDelta` → `Vec<TttStep>`, canonicalized by `step_id`.
- `pf-model::serialize::store_diff` / `load_diff`: validate-and-canonicalize
  + persist + restore through any `BlobStore` under wire format
  `model.diff.v1`. Layout-tag mismatch surfaces as `Error::Integrity`.
- `pf-model::merge::dare(delta, p, seed)`: drop fraction `p` of magnitudes,
  rescale survivors by `1/(1-p)`. SplitMix64-deterministic given `seed`.
- `pf-model::merge::ties_merge(deltas, params)`: TIES task arithmetic —
  trim bottom `keep_top` quantile by magnitude, sign-elect by majority
  magnitude, disjoint-merge same-sign survivors, scale by `alpha`. Default
  `α=0.5`, `keep_top=0.2` per `agent_docs/architecture.md` §4.4.
- 20 unit tests (DARE / TIES / trim / round-trip / canonicalize) + 4
  integration tests (`tests/model_round_trip.rs`):
  - every variant round-trips byte-identically through `FsBlobStore`
  - DARE→TIES composition stays bounded
  - CAS dedup on identical diffs
  - 64-case proptest sweep over random delta lengths, asserting
    `merged.len() == input.len()` and all entries finite.

### Added — Phase 4 (cache layer)

- `pf-cache::format`: `paged-batchinvariant-v1` wire format —
  `PageManifest`, `Page { ix, k, v }` (K and V content-addressed
  independently so a fork mutating only V shares its K page),
  `LogicalSeq { id, page_ixs, fill_in_last_page }`, `CacheMeta`
  (page_size_tokens, n_layers, n_heads, head_dim, dtype), `Dtype`
  (Bf16 / F16 / F32 / Fp8E4m3). `canonicalize()` sorts pages by ix and
  seqs by id so the manifest digest is invariant across iteration order.
- `pf-cache::pager::CachePager`: engine-agnostic interface every
  adapter implements — `pause`, `resume`, `occupied_pages`,
  `logical_seqs`, `read_page`, `allocate_pages`, `write_page`,
  `install_logical_seqs`.
- `pf-cache::pager::SyntheticCachePager`: in-process implementation
  used by every test; SplitMix64-deterministic page filler so identical
  seeds produce byte-identical pages (drives CAS dedup), different seeds
  diverge.
- `pf-cache::serialize::serialize_pages` / `deserialize_pages`:
  portable round-trip via the `BlobStore` trait — no GPU needed.
- `pf-cache::capture::capture_cache` / `restore_cache`: high-level
  one-shot helpers with pause/resume safety guard. Restore validates
  meta equality before touching the destination pager.
- Feature flags `vllm-adapter` and `sglang-adapter` (off by default)
  for the engine FFI shims that land in Phase 10.
- 16 unit tests + 4 integration tests
  (`tests/cache_round_trip.rs`):
  - byte-identical FS-blob-store round-trip
  - 12-fork CoW storage budget (≤ 1.5× one-fork) — Cache-layer
    proof of the §4.6 spec
  - logical-seq round-trip (id-canonicalized order)
  - 100-case proptest sweep over random page sets
- 1 GPU-gated skeleton test (`tests/cache_bit_exact_vllm.rs`) that
  `eprintln!`-skips off-GPU and is wired for the operator to enable
  with `PF_HAS_GPU=1` once `adapters/pf-vllm` lands in Phase 10.

### Added — Phase 3 (effects layer)

- `pf-effects::SideEffectClass`: `Pure | Idempotent | Irreversible | NetworkOnly`,
  declared by tool authors at registration time.
- `pf-effects::SessionSecret`: opaque HMAC-key wrapper with redacted `Debug`
  impl (never logs the secret); `::generate()` uses `ring::rand::SystemRandom`.
- `pf-effects::LedgerEntry` (`effects.entry.v1`): timestamp, tool_id,
  args_hash, idempotency_key, result_hash, side_effect_class, session_hmac.
  HMAC defined as `HMAC-SHA256(secret, prev_entry_hash || this_entry_minus_hmac)`.
- `pf-effects::Ledger`: append-only ledger with HMAC chaining, `verify()`
  scan, `serialize` / `deserialize` round-trip via `BlobStore`.
  Tampering with any entry breaks the chain at that index — defends against
  ACRFence semantic-rollback (arXiv 2603.20625).
- `pf-effects::ReplayPolicy`: per-class replay decisions (`InjectCachedResult`,
  `ReplayWithSameKey`, `ReplayWithNewKey`, `SurfaceAsFact`). Three presets:
  `default`, `strict`, `aggressive`. Default never re-issues `Irreversible`.
- `pf-effects::ToolProxy`: wraps a runtime's tool dispatch so every call
  hashes args, mints an idempotency key (ULID-shaped), runs the tool,
  hashes the result, and appends to the ledger atomically.
- `pf-effects::mint_idempotency_key()`: SHA-256(timestamp_ms ‖ 80 random bits).
  Tested for uniqueness over 256 consecutive calls.
- 14 unit tests + 4 conformance proptests (`tests/fuzz_replay.rs`) running
  1000 cases each, covering the four `agent_docs/effects-layer.md` invariants:
    1. Default policy never re-issues `Irreversible`.
    2. Idempotency keys are unique within a session.
    3. HMAC chain validates on untouched ledgers.
    4. Forking preserves no-duplicate-irreversible across siblings.

### Added — Phase 2 (world layer)

- `pf-world::WalkFsCapture`: portable rayon-parallel filesystem capture
  with deterministic per-tree digest, default ignore-list (`.git/objects`,
  `target`, `node_modules`), opt-in `use_apfs_clone` fast-path that
  `cp -c -R`-clones a directory in O(1) on macOS before walking, opt-in
  `follow_symlinks`, custom ignore fragments via builder API.
- `pf-world::restore_tree`: atomic rebuild of a captured tree —
  stages into a sibling temp dir, then `rename(2)` over `dst`. Refuses to
  overwrite an existing path.
- `pf-world::FsTree` / `FsTreeEntry` (`fs.tree.v1` wire format).
  Files / dirs / symlinks all round-trip; symlinks captured as symlinks
  (their targets recorded), not as the targets they happen to point at.
- `pf-world::EnvCapture`: serializes `std::env::vars()` + cwd into a
  sorted `BTreeMap` so the digest is deterministic across hosts.
  `.scrub("(?i)secret|token")`-style regex redaction; matching keys
  become `"<redacted>"` pre-seal.
- `pf-world::ProcsCapture`: tagged `procs.criu.v1` blob on Linux when
  the `criu` binary is in PATH (full dump+tar deferred to live-Linux
  CI gated by `$PF_HAS_CRIU=1`); `procs.unsupported.v1` placeholder
  with `unsupported_on: <os>` on every other host so restore can warn
  cleanly.
- 9 unit tests + 3 integration tests (`tests/world_round_trip.rs`):
  byte-identical FS round-trip on a 32 MiB / 256-file sandbox (or 1 GB
  if `PF_WORLD_TEST_GB=1`), env determinism, procs blob always emitted.

### Added — Phase 1 (core engine, Rust)

- `pf-core::cas::FsBlobStore`: on-disk content-addressed store, sharded by
  digest prefix, zstd-19 compressed, atomic write via temp+rename, on-read
  re-hash for corruption detection.
- `pf-core::cas::MemBlobStore`: in-memory variant for tests / `--ephemeral`.
- `pf-core::store::PfStore`: high-level wrapper bundling a `BlobStore` plus a
  manifest catalog (`images/<cid>.json` markers for fast `pf log`).
- `pf-core::snapshot::Snapshotter`: atomic four-layer snapshot orchestrator
  using `thread::scope` for concurrent capture; assembles + persists a v1
  `Manifest` in one call.
- `pf-core::fixture`: synthetic per-layer captures (model / cache / world /
  effects / trace) sized for the build host so the CI gate can run without a
  GPU.
- Integration test `tests/snapshot_synthetic_4layer.rs` asserting
  Phase-1 budgets: snapshot <500 ms, CAS dedup on identical content,
  12-fork storage ≤ 1.5× one-fork storage.
- `examples/01-hello-fork/`: end-to-end runnable example printing the
  snapshot CID, wall-clock time, and dedup delta.

Measured on the build host (macOS arm64): snapshot **8 ms** for the default
fixture (1.38 MB total payload), 60× headroom under the 500 ms budget;
identical second snapshot grows the store by **614 B** (the new manifest
JSON).

### Added — Phase 0 (bootstrap)

- Cargo workspace with 10 crates: `pf-core`, `pf-model`, `pf-cache`,
  `pf-world`, `pf-effects`, `pf-merge`, `pf-registry`, `pf-cli`, `pf-py`,
  `pf-ts`. All compile clean (`cargo check --workspace` — zero warnings).
- `pf-core::digest::Digest256` (SHA-256, OCI-style `sha256:<hex>`).
- `pf-core::manifest::Manifest` v1 schema with all four layer descriptors.
- `pf-core::cas::BlobStore` trait surface.
- `pf-core::error::Error` typed-error hierarchy.
- `pf` CLI scaffold rendering all 12 subcommands via `clap` derive.
- Agent infrastructure: `CLAUDE.md`, `agent_docs/*` (13 files),
  `.claude/agents/*` (5 sub-agents), `.claude/skills/*` (5 skills),
  `.claude/hooks/*` (3 hooks), `claude-progress.json`, `claude-plan.md`.
- Project meta: `LICENSE` (MIT), `README.md`, `SECURITY.md`,
  `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`, `.gitignore`.
