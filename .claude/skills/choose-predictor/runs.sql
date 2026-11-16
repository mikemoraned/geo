SELECT
    run.* EXCLUDE (fingerprints) REPLACE (
        to_milliseconds(run.horizon) AS horizon,
        to_milliseconds(run.last_moment) AS last_moment,
        to_milliseconds(run.ramp) AS ramp,
        to_milliseconds(run.warm_up) AS warm_up
    ),
    summary.sessions AS sessions_replayed,
    summary.* EXCLUDE (run_date, run_id, sessions) REPLACE (
        to_milliseconds(summary.scored_duration) AS scored_duration,
        to_milliseconds(summary.lead_p10) AS lead_p10,
        to_milliseconds(summary.lead_median) AS lead_median,
        to_milliseconds(summary.lead_p90) AS lead_p90
    ),
    skew.passes AS skew_passes,
    skew.seen_passes AS skew_seen_passes,
    skew.visits AS skew_visits,
    skew.unmatched_visits AS skew_unmatched_visits,
    skew.seen AS skew_seen,
    skew.seen_low AS skew_seen_low,
    skew.seen_high AS skew_seen_high,
    skew.unmatched AS skew_unmatched,
    skew.unmatched_low AS skew_unmatched_low,
    skew.unmatched_high AS skew_unmatched_high,
    to_milliseconds(skew.offset_p10) AS skew_offset_p10,
    to_milliseconds(skew.offset_median) AS skew_offset_median,
    to_milliseconds(skew.offset_p90) AS skew_offset_p90
FROM read_parquet('data/medallion/gold/evaluation_run_v6/**/*.parquet', hive_partitioning = true) AS run
JOIN read_parquet('data/medallion/gold/evaluation_summary_v2/**/*.parquet', hive_partitioning = true) AS summary
    USING (run_id)
LEFT JOIN read_parquet('data/medallion/gold/evaluation_skew_v2/**/*.parquet', hive_partitioning = true) AS skew
    USING (run_id)
ORDER BY run.started_at;
