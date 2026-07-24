"""Splice pfaedle's shapes into the original GTFS feed, byte-preserving trips.txt.

pfaedle's full-feed rewrite breaks motis GTFS-RT trip resolution (realtime -> ~0%). So does a
plain csv round-trip of trips.txt: it reformats DELFI's quoting, and motis resolves RT trips
against the raw feed but not the reformatted one, even though the values are identical. So we
keep the original feed byte-for-byte and change only:
  - shapes.txt   <- pfaedle's (curved rail track)
  - trips.txt    <- original bytes, with ONLY the shape_id field rewritten on the rows pfaedle
                    re-shaped. shape_id sits before a fixed number of trailing columns, none
                    of which are quoted or contain commas, so a right-split isolates it while
                    leaving every other byte of the line (quoting included) untouched.
Every other table is copied verbatim, so RT resolution is unaffected.

Usage: main.py <original.zip> <pfaedle-output-dir> <out.zip>
"""

import csv
import io
import shutil
import sys
import zipfile
from pathlib import Path

original, pfaedle_dir, output = (Path(a) for a in sys.argv[1:4])
csv.field_size_limit(1 << 30)

with open(pfaedle_dir / "trips.txt", newline="") as f:
    new_shape_id = {row["trip_id"]: row["shape_id"] for row in csv.DictReader(f)}


def rewrite_trips_shape_id(src_file, dst_file):
    text = io.TextIOWrapper(src_file, "utf-8", newline="")
    header = text.readline()
    columns = next(csv.reader([header]))
    trip_col, shape_col = columns.index("trip_id"), columns.index("shape_id")
    trailing = len(columns) - 1 - shape_col  # columns after shape_id (unquoted, comma-free)
    dst_file.write(header.encode("utf-8"))
    changed = 0
    for line in text:
        fields = next(csv.reader([line]))
        wanted = new_shape_id.get(fields[trip_col])
        if wanted is None or wanted == fields[shape_col]:
            dst_file.write(line.encode("utf-8"))
            continue
        stripped = line.rstrip("\r\n")
        ending = line[len(stripped):]
        parts = stripped.rsplit(",", trailing + 1)  # -> [prefix, shape_id, *trailing]
        assert parts[1] == fields[shape_col], (parts[1], fields[shape_col])
        parts[1] = wanted
        dst_file.write((",".join(parts) + ending).encode("utf-8"))
        changed += 1
    return changed


with zipfile.ZipFile(original) as src, \
     zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as out:
    changed = 0
    for name in src.namelist():
        if name.endswith("shapes.txt"):
            continue  # replaced by pfaedle's, added below
        with src.open(name) as f, out.open(name, "w", force_zip64=True) as w:
            if name.endswith("trips.txt"):
                changed = rewrite_trips_shape_id(f, w)
            else:
                shutil.copyfileobj(f, w)
    with open(pfaedle_dir / "shapes.txt", "rb") as f, \
         out.open("shapes.txt", "w", force_zip64=True) as w:
        shutil.copyfileobj(f, w)

print(f"spliced shapes into {output}: rewrote shape_id on {changed} rows")
