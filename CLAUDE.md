# ProcessFork — agent rules

This is the build-agent contract. Keep it short. Subsystem detail lives in
`agent_docs/` and is loaded on demand.

## Toolchain

- Rust **edition 2024**, MSRV **1.85** (the spec said 1.83; edition 2024
  requires 1.85 — recorded in `claude-progress.json/assumptions[]`).
- Python **3.11+**, formatted with `ruff` and type-checked with `mypy --strict`.
- TypeScript with `biome` for lint + format.
- Run `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` before
  every commit. The `.claude/hooks/post-edit.sh` hook does this automatically
  for files you've just touched.

## Style bar

- No `unwrap()` / `expect()` outside `#[cfg(test)]` and `examples/`.
- Every public symbol gets a doc-comment. Doc examples must compile
  (`cargo test --doc`).
- Errors are typed (`thiserror` in libs, `anyhow` only in binaries).
- `tracing` for logs — never `println!` outside CLI user-facing output.
- SPDX header (`// SPDX-License-Identifier: MIT`) on every source file.

## Verification discipline

- A feature is **not done** until an end-to-end example in `examples/` runs
  green against real binaries. Unit tests alone are insufficient.
- Before claiming a phase complete, invoke the gate sub-agents in this order:
  `qa-reviewer` → `security-reviewer` → `perf-reviewer`. All three must PASS.
- Never edit `claude-progress.json` to advance `current_phase` until the gate
  passes.

## State files (read these first every new session)

1. `claude-progress.json` — machine-readable phase state, blockers,
   assumptions. Update on every phase boundary.
2. `claude-plan.md` — your scratchpad and handoff to your future self.
   Keep < 200 lines.
3. `agent_docs/<current-phase>.md` — full spec for what you're building.

## Sessions and context budget

- This is a multi-session build. You may not see the kickoff prompt again.
- At 60% context, write a progress note. At 70%, commit and consider compact.
  At 85%, finish the current logical unit and stop adding new work.
- Always leave the repo in a state where a fresh window can resume from the
  three state files above + `git status`.

## Out of scope (do not build, even if it seems natural)

Hosted SaaS, web dashboard, Windows native, distributed multi-host fork,
custom inference engine, custom model-merge algorithm, telemetry of any
kind. See the §2 scope fences in the original kickoff prompt.
