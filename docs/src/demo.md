# The 60-second demo

The original elevator pitch from the kickoff prompt — translated into a
real, runnable script. Source: [`demo/script.cast.md`](https://github.com/processfork/processfork/blob/main/demo/script.cast.md).

> 0:00 — A developer is 4 hours into a Claude Code session refactoring a
> monorepo. KV cache is 380K tokens. Postgres running. Playwright open.
> Half-built feature branch. Three failed TS builds.
>
> 0:10 — They type `pf snapshot`. **87 ms.** A 1.2 GB `.pfimg` file
> appears.
>
> 0:25 — `pf fork main -n 12 --explore "fix the type error"`. Twelve
> parallel terminal panes flash open. Each is a *fully live agent* —
> same KV cache, same browser session, same DB, same git index —
> diverging in real time. Eight crash; four solve the bug differently.
>
> 0:40 — `pf merge winner-3 -> main`. Git-style diff: world-layer file
> changes, effect-layer ("agent ran `pnpm test` 47 times — replaying as
> cached"), and a structured "what branch-3 learned" patch injected into
> the cache. Original session resumes — same prompt depth, no
> re-prefill, with the fix applied.
>
> 0:55 — `pf push hf://user/refactor-session-2026-05-05`. Cut to a
> teammate on a different laptop, different model build, typing
> `pf checkout hf://user/refactor-session-2026-05-05`. Same agent boots
> in 4 seconds. Bit-exact. Picks up mid-thought.

## What's runnable today on the build host (no GPU)

Run `bash examples/02-cli-snapshot/run.sh` — full transcript of
snapshot → status → log → diff → checkout → verify → push (deferred
exit 2). 8 ms snapshot p99 against the synthetic fixture.

## What needs operator setup

The Hopper-class GPU bit-exact replay (frame 1's "87 ms" against
Llama-3-70B) lives behind `$PF_HAS_GPU=1` plus the v1.0.1 vLLM live
adapter. The `pf push hf://…` round-trip needs `$HF_TOKEN`.

See [Performance](./performance.md) for the full budget table and
[Migrating to v1.0](./migration.md) for the operator-side setup.
