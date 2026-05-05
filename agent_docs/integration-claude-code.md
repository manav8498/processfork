# Integration: Claude Code

`pf wrap claude` boots Claude Code with three injected slash-commands:
`/snapshot`, `/fork`, `/merge`.

## Mechanism

Claude Code (the CLI) supports user-defined slash-commands via files in
`~/.claude/commands/`. The `pf wrap claude` shim:

1. Drops three command files into `~/.claude/commands/processfork/`.
2. Sets `PF_AGENT_KIND=claude-code` and `PF_SESSION_ID=$(uuid)` in the env.
3. Starts Claude Code with `claude --hook PreToolUse=$PF_BIN/effect-tap.sh`.
4. The pre-tool-use hook intercepts every tool call into the effect ledger.

## Commands

`/snapshot [name]` — captures the current Claude Code session into a `.pfimg`
and prints the CID.

`/fork [N] [hint]` — forks N branches; each opens in its own terminal pane
via tmux/iTerm scripting (operator's terminal of choice configured in
`~/.processfork/config.toml`).

`/merge <branch> -> [main]` — three-way merge as in the CLI.

## Effect tap

Claude Code's PreToolUse / PostToolUse hooks invoke a small shell script
that:

1. Reads the tool name and JSON args from stdin.
2. Looks up the side-effect class from `~/.processfork/tool-classes.toml`.
3. Appends a ledger entry to `$PF_STORE/sessions/<session>/ledger.jsonl`.

Unknown tools default to `Irreversible` (safe-by-default). Operators can
classify tools by editing `tool-classes.toml`:

```toml
[tools]
Read = "pure"
Write = "irreversible"
Bash = "irreversible"
Grep = "pure"
Glob = "pure"
WebFetch = "network-only"
```

## Example

`examples/07-claude-code-fork/` runs the full snapshot → fork → merge loop
against a real Claude Code session, and asserts the merged session resumes
mid-thought.
