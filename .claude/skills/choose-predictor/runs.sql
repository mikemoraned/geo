SELECT
    run.* EXCLUDE (fingerprints),
    summary.sessions AS sessions_replayed,
    summary.* EXCLUDE (run_date, run_id, sessions),
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
    skew.offset_p10_ms AS skew_offset_p10_ms,
    skew.offset_median_ms AS skew_offset_median_ms,
    skew.offset_p90_ms AS skew_offset_p90_ms
FROM read_parquet('data/medallion/gold/evaluation_run_v4/**/*.parquet', hive_partitioning = true) AS run
JOIN read_parquet('data/medallion/gold/evaluation_summary_v1/**/*.parquet', hive_partitioning = true) AS summary
    USING (run_id)
LEFT JOIN read_parquet('data/medallion/gold/evaluation_skew_v1/**/*.parquet', hive_partitioning = true) AS skew
    USING (run_id)
ORDER BY run.started_at;
