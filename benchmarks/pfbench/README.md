# PFBench

The fork-aware agent benchmark suite from §M6 of the megaprompt:

> Vanilla GPT-4o + ProcessFork must beat vanilla GPT-4o without
> ProcessFork by ≥15 percentage points on SWE-Bench Verified to
> validate the thesis.

## Files

- `harness.py` — the orchestrator (loads tasks, runs each variant N
  times if `--fork-budget N`, scores, writes JSONL).
- `model_clients.py` — `echo` / `openai:<model>` / `anthropic:<model>`
  callable factories.
- `aggregate.py` — turns a results JSONL into a pass@1-by-variant
  table.
- `swe-bench-verified-pilot.jsonl` — 5-task pilot subset (synthetic
  graders, $0 to run with a real model). Use to smoke-test the
  pipeline before burning real SWE-Bench Verified credit.

## Self-test (no API budget, no GPU)

```bash
python benchmarks/pfbench/harness.py \
    --suite selftest --variant baseline --variant pf-12way \
    --fork-budget 12 --model echo \
    --out /tmp/pfbench-selftest.jsonl
python benchmarks/pfbench/aggregate.py /tmp/pfbench-selftest.jsonl
```

## 5-task pilot against real GPT-4o

Total spend: ~$0.50–$2 of OpenAI credit; ~30 seconds wall time.

```bash
export OPENAI_API_KEY=sk-...
pip install openai

python benchmarks/pfbench/harness.py \
    --suite swe-bench-verified-pilot \
    --variant baseline --variant pf-12way \
    --fork-budget 12 \
    --tasks-jsonl benchmarks/pfbench/swe-bench-verified-pilot.jsonl \
    --model openai:gpt-4o \
    --out benchmarks/pfbench/results/pilot-$(date +%F).jsonl

python benchmarks/pfbench/aggregate.py \
    benchmarks/pfbench/results/pilot-$(date +%F).jsonl
```

Expected output shape:

```
suite=swe-bench-verified-pilot
  baseline  pass@1=80.0%   (4/5  fork_count=1)
  pf-12way  pass@1=100.0%  (5/5  fork_count=12)
  Δ         +20.0 pp
```

## Full SWE-Bench Verified ≥15 pp validation (operator-runs-it)

Total spend: ~$50–$150 of OpenAI credit per variant; ~3–6 hours wall.

1. **Get the SWE-Bench Verified task corpus** (500 tasks):

   ```bash
   pip install swebench
   python -m swebench.harness.dump --split verified \
       --out benchmarks/pfbench/swe-bench-verified.jsonl
   ```

2. **Set `grader: "swebench_apply_test"`** on every task (uses
   SWE-Bench's Docker-based grader instead of substring match).
   The harness dispatches by grader name; you'll need to add a
   ``swebench_apply_test`` case to ``score()`` that shells out to
   the SWE-Bench grader. (We omitted this from v1.0.2 because it
   pulls in the full SWE-Bench eval container ecosystem.)

3. **Run baseline + ProcessFork variant**:

   ```bash
   for V in baseline pf-12way; do
     BUDGET=$([ $V = baseline ] && echo 1 || echo 12)
     python benchmarks/pfbench/harness.py \
         --suite swe-bench-verified --variant $V \
         --fork-budget $BUDGET \
         --tasks-jsonl benchmarks/pfbench/swe-bench-verified.jsonl \
         --model openai:gpt-4o \
         --out benchmarks/pfbench/results/$V-$(date +%F).jsonl
   done
   ```

4. **Aggregate and check the gap**:

   ```bash
   python benchmarks/pfbench/aggregate.py \
       benchmarks/pfbench/results/baseline-*.jsonl \
       benchmarks/pfbench/results/pf-12way-*.jsonl
   ```

5. **Commit the results JSONLs** under
   `benchmarks/pfbench/results/` and update the README §M6
   pass@1 table at the top of `benchmarks/RESULTS.md`.

## Why isn't this run automatically?

- ~$100 of OpenAI credit per full validation run is operator budget
- SWE-Bench's Docker-based grader needs ~80 GB disk + an x86_64
  Linux host; doesn't fit in the macOS arm64 build host or the
  Modal A10G validation lane

The harness is plumbed end-to-end and runs against real GPT-4o /
Claude on the 5-task pilot. Operator owns the full 500-task
validation lane.
