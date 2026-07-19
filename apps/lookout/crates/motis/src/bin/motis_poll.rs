//! `motis_poll`: polls recent GPS positions off the redis queue, queries the local
//! Motis server for train trips within a buffered bounding box around them, and appends
//! the returned segments to a raw, duplication-allowed `motis` SQLite log.

fn main() {}
