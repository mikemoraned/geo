SELECT
    run.* EXCLUDE (fingerprints),
    summary.sessions AS sessions_replayed,
    summary.* EXCLUDE (run_date, run_id, sessions)
FROM read_parquet('data/medallion/gold/evaluation_run_v3/**/*.parquet', hive_partitioning = true) AS run
JOIN read_parquet('data/medallion/gold/evaluation_summary_v1/**/*.parquet', hive_partitioning = true) AS summary
    USING (run_id)
ORDER BY run.started_at;
