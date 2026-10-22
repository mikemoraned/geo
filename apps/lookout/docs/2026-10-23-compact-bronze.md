# Storing the Motis capture compactly in bronze

*Assessment of 2026-10-23. It draws on the `motis_segment_v2` captures of 11–22 October 2026, the
repository's object store, and rewrites of 20–22 October in other layouts. DuckDB 1.5.6 wrote each
rewrite, and a scratch git repository packed it. Nothing in the store has changed. The store's own
writer, parquet-rs 57.3.1, wrote none of the layouts measured. Whatever survives a change to the
capture belongs in [medallion.md](medallion.md) and [motis.md](motis.md), and the rest goes stale.*

How the Motis capture can take less space in git, with bronze and gold still versioned and silver
still derived. It is for whoever next changes how the capture is written.

A poll file is the Parquet file one poll writes. Sizes are summed file lengths, in megabytes of
10⁶ bytes. A size in git is the size of the pack that `git gc` writes.

## The capture is nearly all of versioned bronze

`motis_segment_v2` holds 1,530 MB of poll files. Every other versioned bronze dataset together
holds 22 MB. The capture polls Germany every 10 minutes, and a full day writes 283–322 MB.

The writer compresses nothing. `writer_at`, in `crates/medallion/src/write.rs`, passes no writer
properties, and parquet-rs 57.3.1 defaults to `Compression::UNCOMPRESSED`. `writer_at` writes
silver too, so a change to its properties reaches both layers.

## Each polyline is stored about 80 times

The `polyline` column takes 2.33 MB of a 2.77 MB poll file, the poll of 3,276 legs at 10:02 UTC
on 20 October. Across the capture dataset, 1,919,542 rows hold 1,334.5 MB of polyline text, 87% of
the dataset. Only 24,790 polylines are distinct, 16.7 MB in all.

Two kinds of repetition multiply. A leg appears in every poll whose window it overlaps, and many
trips share the polyline between two stops. On 20 October, 403,868 rows held 259,446 distinct legs
and 22,164 distinct polylines. Each day also repeats the polylines of the days before it, so few
are new after the first full day.

| Days | Rows | Polyline text | Polylines first seen | Their text |
| --- | --- | --- | --- | --- |
| 11–17 October | 9,433 | 15.1 MB | 3,428 | 2.05 MB |
| 18 October | 326,271 | 249.8 MB | 16,187 | 11.23 MB |
| 19 October | 410,804 | 278.3 MB | 4,136 | 2.61 MB |
| 20 October | 403,868 | 272.2 MB | 761 | 0.43 MB |
| 21 October | 398,474 | 268.3 MB | 161 | 0.19 MB |
| 22 October | 370,692 | 250.8 MB | 117 | 0.14 MB |

The rows for 22 October come from 123 polls, two of them uncommitted. A full day holds 144.

## Git packs the poll files to a fifth of their size

Git compresses each object with zlib. When it packs objects, it also stores each poll file as a
delta against a similar one. The committed poll files take 725.7 MB of the object store now, for
1,529.9 MB of file content. Packed, they would take 315.7 MB.

Each day's stored size therefore depends on whether `git gc` has packed it yet.

| Day | Poll files | File content | Stored now | Stored as |
| --- | --- | --- | --- | --- |
| 18 October | 144 | 282.8 MB | 64.5 MB | packed |
| 19 October | 143 | 321.8 MB | 102.5 MB | a third loose |
| 20 October | 143 | 313.8 MB | 191.3 MB | loose |
| 21 October | 142 | 309.4 MB | 188.5 MB | loose |
| 22 October | 121 | 288.5 MB | 175.6 MB | loose |

The object store holds 1.6 GB in all, 0.6 GB of it loose. GitHub recommends keeping a repository
under 1 GB, and strongly recommends under 5 GB.

## Sizes of each layout

Each layout holds 20–22 October, about 408 polls and 1.17 million rows. A file holding several
polls sorts its rows by stop pair, polyline, trip, and capture time. Every compressed file uses
zstd at level 3. An uncompressed layout shows its size in git. A compressed layout shows its size
as written, since `git gc` shrank compressed files by 4% at most.

Size a day, by how many polls a file holds and where the polyline lives:

| One file per | Polyline in every row | Polyline stored once |
| --- | --- | --- |
| Poll, uncompressed, as now | 58–70 MB | 37 MB |
| Poll, zstd | 160 MB | 26 MB |
| Hour | 57 MB | 14 MB |
| 6 hours | 18 MB | 7.9 MB |
| 12 hours | 12 MB | 6.3 MB |
| Day | 8.3 MB | 5.4 MB |

- The poll files as they are now take 58.0 MB a day in git over the three days, and 70.1 MB on
  20 October alone. The one fully packed day in the repository, 18 October, takes 64.5 MB.
- The zstd poll files with the polyline in every row cover 20 October alone.
- Sizes under Polyline stored once leave out the polyline table. It holds 10.4 MB for every
  polyline the three days saw, and grows by 0.1–0.4 MB of polyline text a day.
- A file per day left uncompressed takes 6.2 MB a day in git, against 8.2 MB for a zstd file per
  day. Git finds deltas between consecutive days' files. Its checkout holds 23.9 MB a day, against
  8.3 MB.

## What the sizes show

**Compressing poll files more than doubles their size in git.** Git finds no deltas between zstd files. On
20 October, zstd poll files pack to 158.9 MB, against 70.1 MB for the poll files as they are.
Compression pays off only together with storing polylines once or merging polls.

**Storing each polyline once, with zstd, cuts poll files by more than half.** A polyline table would hold each
polyline verbatim, keyed by a UUID v5 of its text, as a source's id is. A capture row would carry
that id in place of the polyline. The measurements used a 16-byte MD5, the width of a UUID. The
poller would keep the 24,790 known ids in memory, as it keeps trip details for a run. Writing the
polyline table before the capture rows means a crash leaves an unused polyline, never a missing
one. Two pollers writing the same polyline add a duplicate row, and readers already collapse
duplicates.

**Merging polls into fewer files removes most of the remaining repetition, with no schema
change.** Once repeats sit next to each other, Parquet's dictionary encoding, run-length encoding,
and zstd remove them. Sort order decides how close they sit. In a file per day sorted by stop pair,
each coordinate column takes 0.09–0.14 MB. Sorted by trip, each takes 0.73–0.75 MB.

**A polyline table makes the merge period matter less.** A file with the polyline in every row
restates each polyline it holds, so hourly files take 57 MB a day. With a polyline table, hourly
files take 14 MB a day. Merging at each commit of the capture so far, every 10–24 hours, comes to
5.4–7 MB a day.

**Storing only changed rows gains little.** Over 20–22 October, 35% of rows repeat another capture
of the same leg: the same trip, stops, times, and polyline. A sorted file already stores such rows
cheaply. Storing only changes would also alter what a row means, from one leg in one poll to one
leg over a span of polls.

## What bronze's rules allow

**Polls merge before they reach git.** Bronze never rewrites a file, and git keeps every committed
blob. Merging committed poll files would therefore add the merged file and free nothing. The poller
would instead write polls to landing, which git ignores, and a drain would write one bronze file
per run. [medallion.md](medallion.md) already describes landing as the capture log a polling
process appends to, drained into bronze. Its partitioning for a third-party service's samples names
one file per poll, which would become one file per drain.

**A drain adds no new risk of loss.** Until a drain runs, polls exist only on the local disk, as
uncommitted poll files do now. A drain would read its file back and check the rows before deleting
landing files, so a failed drain leaves no gap.

**The polyline stays as received.** A polyline table keeps each polyline verbatim, as medallion.md
requires of a compact third-party geo format.

**Readers need no change for merged files.** `motis_ingest` registers the capture dataset as one
table, and picks each leg's newest capture by `captured_at`. A merged file with today's columns
reads as the poll files it replaces do. A polyline table needs a join in that query. Its window,
`ROW_NUMBER() OVER (PARTITION BY … ORDER BY captured_at DESC)`, now carries each row's polyline.
With a polyline table, it would carry a 16-byte id, and the join would add polylines after the
window. The effect on ingest's 12.7 GB memory peak is unmeasured.

## History

What is already committed stays in the clone. A new version of the capture dataset, migrated as
`motis_source` became `motis_source_v2`, shrinks each checkout but not the clone. Each worktree on
a branch holding the poll files checks out its own 1,530 MB.

Shrinking the clone needs a history rewrite, such as `git filter-repo` removing the poll files. It
rewrites every commit since the first poll file, and needs a force-push and a reset of every other
clone and worktree. As long as the new version holds every column of the old, the rewrite loses no
data.

The six-day capture of Germany runs until about 25 October. Each day committed as poll files adds
about 60 MB to the packed history for good. A day held back until a compact form exists never
enters history, but meanwhile exists only on the local disk.

## Recommended order

1. Decide how the rest of the six-day capture enters git.
2. Write polls to landing, and drain them into one sorted, zstd-compressed bronze file per run.
   Draining once a day adds 8.3 MB a day, with no schema change.
3. Add a polyline table, as a new version of the capture dataset. Draining once a day then adds
   5.4 MB a day, and frequent drains stay cheap.

If the repository's size becomes a problem, rewrite history with `git filter-repo`.

## Open questions

- **Do the sizes hold for parquet-rs?** The store writes with parquet-rs 57.3.1, whose defaults
  differ from DuckDB's. Its dictionary page limit is 1 MiB, which a day's 13.7 MB of distinct
  polylines exceeds. Its row groups hold up to 1,048,576 rows, against DuckDB's 122,880. No layout
  measured has been written with parquet-rs.
- **Does a polyline table lower ingest's memory peak?** No run has measured it.

## Method

Each measurement can be repeated with DuckDB 1.5.6, git, and the store's files.

- Polyline counts come from DuckDB queries over
  `read_parquet('…/bronze/motis_segment_v2/*/*.parquet', hive_partitioning = true)`.
- Each layout is a DuckDB `COPY` of the poll files. This statement writes a file per day with the
  polyline in every row:

  ```sql
  COPY (
    SELECT *
    FROM read_parquet(
      ['…/polled_date=2026-10-20/*.parquet',
       '…/polled_date=2026-10-21/*.parquet',
       '…/polled_date=2026-10-22/*.parquet'],
      hive_partitioning = true)
    ORDER BY polled_date, from_stop_id, to_stop_id, polyline, trip_id, captured_at
  ) TO 'day_inline_zstd' (FORMAT parquet, COMPRESSION zstd, PARTITION_BY (polled_date));
  ```

- A polyline id is `unhex(md5(polyline))`, the width of a UUID. A merge period partitions on a
  bucket of the capture time, such as `time_bucket(INTERVAL '6 hours', captured_at)`.
- A size in git comes from a scratch repository:

  ```sh
  git init -q scratch && cp -R day_inline_zstd scratch/
  git -C scratch add -A && git -C scratch commit -qm layout
  git -C scratch gc -q --prune=now
  stat -f %z scratch/.git/objects/pack/*.pack
  ```

- The committed poll files come from the repository. The `cat-file` pipeline gives each blob's
  stored size. The `pack-objects` pipeline gives their packed size, without writing to the
  repository:

  ```sh
  git rev-list --objects --all -- <dataset path> \
    | git cat-file --batch-check='%(objecttype) %(objectsize) %(objectsize:disk) %(rest)'
  git rev-list --objects --all -- <dataset path> | git pack-objects --stdout | wc -c
  ```
- On APFS, `du` overstates files DuckDB writes by up to a third, so every size sums file lengths.

## References

- [About large files on GitHub](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)
  — the repository sizes GitHub recommends
- parquet-rs 57.3.1, `parquet/src/file/properties.rs` — the writer's defaults
- [medallion.md](medallion.md) on bronze's rules and the landing layer, and [motis.md](motis.md) on
  the capture
