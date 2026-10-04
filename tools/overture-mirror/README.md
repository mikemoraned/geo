# overture-mirror

Recipes that copy Overture Maps releases from the public bucket into a local mirror, or from one
mirror to another. Any project
in this repo that reads Overture reads the mirror when its drive is mounted. The mirror itself
lives on an external drive, so the repo records how the mirror is made and not what it holds.

## Where the mirror is

`mirror.just` holds the mirror's path, and nothing else. This Justfile and each app's Justfile
import it. If the drive moves, edit that one line.

## Recipes

- `just releases [source]` lists the releases a source holds, and marks each one that a later
  release supersedes.
- `just sync <release> [source] [mirror]` copies one release from a source into a mirror, and
  resumes from the files already there.
- `just verify <release> [source] [mirror]` checks a mirrored release against a source, and
  copies nothing.
- `just test` runs the tests.

A source is either a bucket, written `s3://<bucket>/<prefix>`, or a mirror's directory. A
destination is always a mirror's directory. The source defaults to the Overture bucket, and the
mirror to the path in `mirror.just`.

The recipes run a Rust binary that reads the bucket anonymously through `object_store`, so the
machine needs no AWS CLI and no credentials.

`sync` and `verify` refuse a release the source does not hold, a mirror path that is not a
directory, and a mirror that is the source's own directory. `sync` also refuses a superseded
release.

`sync` and `verify` ignore files named in a denylist, wherever they sit. It holds `.DS_Store`,
which macOS writes into directories opened in Finder. Such a file is never copied, and never
reported as present locally alone.

## A mirror holds whole releases

`sync` copies every theme of a release, not only the themes a project reads today. The mirror
exists to keep a release readable after it ages out of the bucket. Once the release ages out, a
theme left out is unreachable. A whole release runs to hundreds of GiB: 2026-09-23.1 is 577 GiB.

## Progress counts the bytes each command moves

`sync` and `verify` draw one progress bar, with a spinner, the bytes done out of the total, the
rate, and the time left. The total counts the bytes the command reads from the source:

- `sync` counts the size of each file it copies. A file already complete leaves the total, so
  the time left covers only the copying still to do.
- `verify` counts the bytes it reads from the source to build each signature, at most 2 KiB a
  file. A file it reads nothing for, because it is missing locally or its size differs, leaves
  the total.

## A sync resumes from what the mirror holds

A copy of a whole release takes hours, and a sync that stops part-way resumes on the next run.
Each file has a signature: its size, its first 1 KiB, and its last 1 KiB. A local file whose
signature matches the bucket's copy counts as complete, and `sync` copies every other file again.
The bucket's half of a signature takes two ranged reads, made only where a local file of the
right size exists.

A request to the bucket that fails is retried for up to 30 minutes. The first wait is 1 second,
and each wait doubles, up to 60 seconds. A failure that outlasts the retries stops the sync, and
the next run resumes from what it copied.

A file is written under a `.part` suffix and renamed once complete. An interrupted copy
therefore leaves a `.part` file, never a truncated file under the real name.

## `verify` applies the same check without copying

`verify` compares each file of a release with the bucket's copy by signature. It lists each file
missing locally, differing from the bucket, or present locally alone, such as a leftover `.part`
file. If it lists any file, it exits with a failure.

## One mirror backs up another

A second mirror, such as one on a NAS, takes its releases from the first rather than from the
bucket. Each command names the first mirror as its source:

```
just sync 2026-09-23.1 /Volumes/portable/release /Volumes/nas/release
just verify 2026-09-23.1 /Volumes/portable/release /Volumes/nas/release
```

A source mirror's directories that are not releases are skipped, since a NAS or an operating
system can add its own. A `.part` file in a source is an unfinished copy, and is skipped too.

## A superseded release is never mirrored

Overture names a release `YYYY-MM-DD.N`. A later `.N` of the same date replaces the earlier
release, because Overture found a fault in it. The mirror holds only the latest `.N` of each
date. Data already read from a superseded release stays usable, provided it records the release
it came from.
