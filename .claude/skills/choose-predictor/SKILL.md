---
name: choose-predictor
description: Report lookout's evaluation runs in gold as a dated doc, and choose the apps' predictor and its settings with the user. Use after every run of `just evaluate`, and whenever the user asks which predictor the apps run.
user-invocable: true
---

This skill reports every evaluation run in lookout's gold, and opens a discussion with the user
that ends in a choice of predictor and settings for the apps. The report is a dated doc in
`docs/evaluations/`, so the review sits in a diff beside the runs it weighed. Run it from
`apps/lookout`.

## Steps

1. **Read every run.** Run `duckdb -json < ../../.claude/skills/choose-predictor/runs.sql`. Each
   row is one run's settings joined to its summary. If gold holds no run, say so and stop.

2. **Find the runs no report covers.** A report names each run it covers by its `run_id`. Search
   `docs/evaluations/` for each id. If every run is covered, say so and stop.

3. **Find what the apps run now.** Read which predictor `platform-core`'s model builds, and the
   settings it passes, from the code. Mark that predictor's runs in the report.

4. **Group the comparable runs.** Two runs are comparable when they share `evaluation_version`,
   `country`, `realtime_at_least`, `sessions`, `pick_seed`, and every scoring
   setting: `horizon_ms`, `last_moment_ms`, `ramp_ms`, `last_moment_score`, `floor`, and
   `warm_up_ms`. A run outside every group of two or more goes in a list of its own.
   The oracle reads ground truth, which the apps cannot, so it can never be chosen. Its runs are a
   reference, showing the best score the data and the rules allow, and the report says so.

5. **Write the report** as `docs/evaluations/<today>-choose-predictor.md`. If today's report
   exists, add the new runs to it rather than starting another. Write it through the two writing skills, as
   `CLAUDE.md` requires. It holds these sections, in order:
   - **Runs:** one table per group, one row per run. Each row gives the predictor and its
     settings, the float type, and precision, recall, and false alarms per hour, each with its
     interval as `value [low, high]`. It also gives the lead time's p10, median, and p90, and the
     counts of passes, notifications, false alarms, repeats, and misses. The group's shared
     settings and its run ids head the table.
   - **Comparison:** for each pair of predictors in a group, which measures differ. Two
     predictors differ on a measure only where their intervals do not overlap. Where intervals
     overlap, the runs cannot tell the predictors apart.
   - **Trade-offs:** what the numbers leave open, such as recall against false alarms per hour,
     and the predictor and settings you recommend.
   - **Choice:** left empty until step 6 ends.

6. **Discuss the choice with the user.** Present the trade-offs and your recommendation, and ask
   which predictor and settings the apps run. Record the answer under **Choice**, with the
   reasons the user gave. A choice to keep the current predictor counts as a choice.

7. **Report** the doc's path, and whether the choice changes what the apps run. A change is a
   code change to `platform-core`, made as its own task rather than by this skill.
