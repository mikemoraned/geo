# Overture Maps, and what an extract takes

What the upstream source publishes, and what an extract keeps of it. An extract lands in the
store [medallion.md](medallion.md) describes, whose rules on immutability and layout hold for
this source as for any other.

## A release is immutable, and only the recent ones are reachable

Overture publishes monthly, names each release `YYYY-MM-DD.N`, and serves the recent ones from a
public bucket. Old releases age out of it, so the pinned default needs bumping as they do; a run
can name another.

A release never changes, and that is what makes an extract re-fetchable: read again over the same
window, it answers with the same rows. So the manifest records the release, and nothing about the
reading of it.

## The layout is `theme=…/type=…`, in either location

Overture lays a release out as GeoParquet, one directory per theme and type, and that partition is
the unit everything else follows: a query registers one, an extract reads one and writes one back.
A local mirror of the bucket's `release/` prefix holds the identical files under the identical
layout, so it is the same release by a shorter path. The location is not the data: either answers
with the same extract, and provenance records the release rather than where it was read.

The bucket takes anonymous, unsigned requests. Either location is named as a directory with a
trailing slash rather than a glob, since a `/*` glob fails the reader's `.parquet` extension check.

## Every row carries its own envelope

Overture writes a `bbox` struct on each row. A predicate over it prunes row groups before any
geometry is decoded, so a window is four comparisons on `bbox` rather than a spatial operation.

A window keeps every row whose envelope touches it, rather than only the rows it contains: a river
running off the edge still crosses a railway inside.

## The country's own outline is the window

The release holds each country's boundary, as `division_area` rows of subtype `country`, and an
extract is restricted to the bounding box of one. The window is therefore the release's own, not a
function of whatever GPS fixes have been observed so far, and narrowing to an area of interest
belongs to the derivations that read the extract.

Those same boundaries answer which country a point is in, by a point-in-polygon test against the
outlines rather than against a box. A point is therefore placed by the boundaries everything else
is derived against.

## Rail brings its connectors, and nothing else does

Segments are roads, railways and the like, joined at connectors. An extract keeps rail segments,
then only the connectors those segments name: the window holds every road junction in the country
as well, and a rail segment's own reference is what tells the two apart. Street `tram` lines are
left out as not the transport in question, and their connectors go with them.

## One extraction takes every theme, and records it last

The themes are read together — crossings join rail to water and clip both against the country
boundary the window came from — so all of them go under one extract id. Split across extractions,
a join could span two releases.

An extraction writes its manifest row after the rows it describes, because that row is the claim
that the extract is complete. Taking a recorded extract again writes no new row: the one being
read is already the record, and it names the instant the extraction was first taken.

Extracts are large — around 1.5 GB for one country — and re-derivable from the manifest, so a
store commonly holds the manifest and none of the rows. Filling one in is the ordinary operation;
taking a new extract is the rarer one.
