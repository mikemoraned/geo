# Overture Maps, and what an extract takes

What the upstream source publishes, and what an extract keeps of it. An extract lands in the store
[medallion.md](medallion.md) describes, and its rules on immutability and layout hold for this
source as for any other.

## A release is immutable, and only the recent ones are reachable

Overture publishes monthly, names each release `YYYY-MM-DD.N`, and serves the recent ones from a
public bucket. Old releases age out of it, so the pinned default needs bumping; a run can name
another. Asking a location for a release it lacks reports the releases it holds, since only a
mirror answers for an aged-out release.

A release never changes, so an extract is re-fetchable: read again over the same window, it answers
with the same rows. The manifest therefore records the release, and nothing about how it was
read.

## The layout is `theme=…/type=…`, in either location

Overture lays a release out as GeoParquet, one directory per theme and type, and that partition is
the unit everything else follows: a query registers one, an extract reads one and writes one back.
A local mirror of the bucket's `release/` prefix holds the identical files under the identical
layout, so it is the same release by a shorter path. The location is not the data: either answers
with the same extract, and the manifest records the release rather than where a run read it.

The bucket takes anonymous, unsigned requests. A read names either location as a directory with a
trailing slash rather than a glob, since a `/*` glob fails the reader's `.parquet` extension
check.

## A GERS id names an entity, and a label only describes it

Overture gives each feature in its reference map an id from the Global Entity Reference System,
records in a registry when that id was first seen, last seen and last changed, and reports in a
per-release changelog what moved. An id therefore survives a release where a label does not: two
releases can disagree about a name, a code or a class and still agree which entity is which. Any
reference to upstream data that has to outlive a release is therefore a GERS id.

The commitment covers the reference map, divisions and transportation among them, rather than
everything a release holds. Overture derives a base-theme id from the feature itself: such an id
holds while the feature does, changes when it changes, and leaves no record that the feature was
once the same. A derivation keyed on one renames its own rows across a release even where the
geography stood still. Every id is a 36-character UUID, and in the releases read so far the
registry's ids are version 4 and the derived ones version 3.

## Every row carries its own envelope

Overture writes a `bbox` struct on each row — `xmin`, `ymin`, `xmax`, `ymax` — beside the
geometry itself, and the
[schema](https://docs.overturemaps.org/schema/reference/base/water/) calls it an optional
bounding box for the feature. A predicate over those four bounds prunes row groups before
a reader decodes any geometry, so a window costs four comparisons rather than a spatial
operation.

A window keeps every row whose envelope touches it, rather than only the rows it contains: a river
running off the edge still crosses a railway inside.

## The country's own areas are the window

The release holds each country as two `division_area` rows of subtype `country` — its land and its
territorial waters — which carry the `division_id` of the country division itself. That id selects
both rows and nothing else, where the country code selects whatever rows carry the code. The code
describes a row; the id names the entity, so a read asks for the division id.

The window is the bounding box of both areas together, and so the release's own rather than a
function of whatever fixes a device has recorded. Narrowing to an area of interest belongs to the
derivations that read the extract. Reaching as far as the territorial waters do, the window can run
much wider than the land: for a country whose outlying rock carries waters but no land, the box
reaches some five degrees of longitude west of its coast.

An extract keeps both areas. The wider window reaches into the open sea, so the extract holds water
rows that lie beyond any railway, and it is larger for them. It exists to find the water a railway
meets, and a coastal crossing sits in the waters rather than on the land, so a window drawn round
the land alone would drop the rows the derivation looks for.

Those same areas answer which country a point is in, by a point-in-polygon test against the
outlines rather than against a box. A point is therefore placed by the same boundaries everything
else is derived against, and a fix over the territorial waters places as readily as one over
land — which is what a railway crossing an estuary needs. A test holds it: a division with both
areas places a point over the waters.

## Rail brings its connectors, and nothing else does

Segments are roads, railways and the like, joined at connectors. An extract keeps rail segments,
then only the connectors those segments name: the window holds every road junction in the country
as well, and a rail segment's own reference tells the two apart. An extract leaves out street
`tram` lines — not the transport in question — and their connectors with them.

## One extraction takes every theme, and records it last

An extraction reads the themes together — crossings join rail to water and clip both against the
country boundary the window came from — so all of them go under one extract id. Were they split
across extractions, a join could span two releases.

An extraction writes its manifest row after the rows it describes, because that row is the claim
that the extract is complete. Taking a recorded extract again writes no new row: the one being
read is already the record, and it names the instant the extraction was first taken.

Extracts are large — around 1.5 GB for one country — and re-derivable from the manifest, so a
store commonly holds the manifest and none of the rows. Filling one in is the ordinary operation;
taking a new extract the rarer one.
