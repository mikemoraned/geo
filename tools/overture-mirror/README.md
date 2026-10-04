# overture-mirror

Recipes that copy Overture Maps releases from the public bucket into a local mirror. Any project
in this repo that reads Overture reads the mirror when its drive is mounted. The mirror itself
lives on an external drive, so the repo records how the mirror is made and not what it holds.

## Where the mirror is

`mirror.just` holds the mirror's path, and nothing else. This Justfile and each app's Justfile
import it. If the drive moves, edit that one line.

## Recipes

- `just releases` lists the releases the bucket serves, and marks each one that a later
  release supersedes.
- `just sync <release>` copies one release into the mirror, and resumes from the files already
  there.
- `just verify <release> [mirror]` checks a mirrored release against the bucket, and copies
  nothing. The mirror defaults to the path in `mirror.just`.
- `just test` runs the tests.

The recipes run a Rust binary that reads the bucket anonymously through `object_store`, so the
machine needs no AWS CLI and no credentials.

`sync` and `verify` refuse a release the bucket does not serve, and a mirror path that is not a
directory. `sync` also refuses a superseded release.

## A mirror holds whole releases

`sync` copies every theme of a release, not only the themes a project reads today. The mirror
exists to keep a release readable after it ages out of the bucket. Once the release ages out, a
theme left out is unreachable. A whole release runs to hundreds of GB: 2026-09-23.1 is 619 GB.

## A sync resumes from what the mirror holds

A copy of a whole release takes hours, and a sync that stops part-way resumes on the next run.
Each file has a signature: its size, its first 1 KiB, and its last 1 KiB. A local file whose
signature matches the bucket's copy counts as complete, and `sync` copies every other file again.
The bucket's half of a signature takes two ranged reads, made only where a local file of the
right size exists.

A file is written under a `.part` suffix and renamed once complete. An interrupted copy
therefore leaves a `.part` file, never a truncated file under the real name.

## `verify` applies the same check without copying

`verify` compares each file of a release with the bucket's copy by signature. It lists each file
missing locally, differing from the bucket, or present locally alone, such as a leftover `.part`
file. If it lists any file, it exits with a failure.

## A superseded release is never mirrored

Overture names a release `YYYY-MM-DD.N`. A later `.N` of the same date replaces the earlier
release, because Overture found a fault in it. The mirror holds only the latest `.N` of each
date. Data already read from a superseded release stays usable, provided it records the release
it came from.
