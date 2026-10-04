# overture-mirror

Recipes that copy Overture Maps releases from the public bucket into a local mirror. Any project
in this repo that reads Overture reads the mirror when its drive is mounted. The mirror itself
lives on an external drive, so the repo records how the mirror is made and not what it holds.

## Where the mirror is

`mirror.just` holds the mirror's path, and nothing else. This Justfile and each app's Justfile
import it. If the drive moves, edit that one line.

## Recipes

- `just prerequisites` installs the AWS CLI.
- `just releases` lists the releases the bucket serves, and marks each one that a later
  release supersedes.
- `just sync <release>` copies one release into the mirror.
- `just test` runs the tests.

The recipes run a Rust binary that drives the AWS CLI. It reads the bucket's listing as JSON, and
leaves the copy to `aws s3 sync`.

`sync` refuses a release the bucket does not serve, a superseded release, and a mirror path that
is not a directory. The safehouse sandbox hides the external drive, so run `sync` outside it.

## A mirror holds whole releases

`sync` copies every theme of a release, not only the themes a current extract reads. The mirror
exists to keep a release readable after it ages out of the bucket. Once the release ages out, a
theme left out is unreachable. 2026-09-23.1 is 619 GB whole, and the three themes lookout reads
come to 131 GB of it.

## A superseded release is never mirrored

Overture names a release `YYYY-MM-DD.N`. A later `.N` of the same date replaces the earlier
release, because Overture found a fault in it. The mirror holds only the latest `.N` of each
date. An extract already taken from a superseded release stays valid, since its manifest names
the release it came from.
