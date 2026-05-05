# 60-second viral demo — recording script

Operator-runs-it. Records to `demo/script.cast` (asciinema format).

## Prereqs

- `pf` binary built and on PATH (`cargo build --release -p pf-cli`).
- `asciinema` installed (`brew install asciinema` or
  `pip install asciinema`).
- A clean shell with `PS1='$ '`.
- Optional: `agg` for converting cast → svg/gif.

## Record

```bash
asciinema rec demo/script.cast \
    --title "ProcessFork — fork() for AI agents" \
    --command "bash demo/script.sh"
```

Stop recording with `Ctrl-D`.

## Convert to GIF / SVG

```bash
agg demo/script.cast demo/script.gif --speed 1.4
# or
agg demo/script.cast demo/script.svg --speed 1.4 --format svg
```

## Embed

The README expects `demo/script.gif` to exist for the
[60-second demo](../docs/src/demo.md) embed.

## Frame budget (matches the kickoff prompt's elevator pitch)

| t (sec) | frame                                                              |
|---------|--------------------------------------------------------------------|
| 0:00–0:10 | Setup: developer 4h into a Claude Code session                  |
| 0:10–0:25 | `pf snapshot` — 87 ms (build host: 8 ms on the synthetic fixture) |
| 0:25–0:40 | `pf fork main -n 12 --explore "fix the type error"`             |
| 0:40–0:55 | `pf merge winner-3 -> main` — typed three-way merge UX          |
| 0:55–1:00 | `pf push hf://…` + teammate `pf checkout` on a different host   |

The actual recording uses the `file://` registry round-trip we can
run on a laptop today — see `demo/script.sh`.
