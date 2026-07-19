//! Motis capture + ingest: poll the local Motis server for train trips near recently
//! logged GPS positions, append the raw segments to a `motis` SQLite log, then dedup
//! and decode them into a derived `train_segment` table in the `lookout` db.
