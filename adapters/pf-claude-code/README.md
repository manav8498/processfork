# processfork-claude-code

ProcessFork integration for [Claude Code](https://claude.com/claude-code).
Adds three slash-commands inside any Claude Code session:

- `/snapshot [name]` — captures the live session into a `.pfimg` and prints
  the CID.
- `/fork [N] [hint]` — forks N divergent branches.
- `/merge <branch> [-> main]` — three-way merge with conflict surfacing.

## Install

```bash
pip install processfork-claude-code
```

## Use

```bash
pf-wrap-claude --store ~/.processfork
```

This drops three command files into `~/.claude/commands/processfork/`,
configures `PF_AGENT_KIND=claude-code` in the env, and registers a hook
script that taps the effect ledger on every tool call. Restart your
Claude Code session and the commands appear under `/`.

## How it works

The wrapper is a thin Python layer over the ProcessFork SDK
(`processfork`). The slash-command files invoke `processfork.snapshot`,
`processfork.fork`, and `processfork.merge` against your local store.

Tool-call interception goes through Claude Code's `PreToolUse` /
`PostToolUse` hooks; each invocation is appended to the effect ledger
with the side-effect class declared in `~/.processfork/tool-classes.toml`
(unknown tools default to `irreversible` — safe by default).

See `agent_docs/integration-claude-code.md` in the main repo for the
full spec.
