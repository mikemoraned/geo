# Current Slice: minimal version of water crossings

### Target

We want to be able to find where rivers / lakes / other visible water bodies cross a train line. This doesn't have to be absolutely perfect i.e. it's ok if we mis-identify some water classes and/or have some other geo anomilies. The water crossing should be visualisable so that we can manually cross-check it makes sense.

It's ok if we only get this for a single country (Germany) or state (Thuringen).

### Straw Man

We should be able to do something like:
1. Use OvertureMaps [water](https://docs.overturemaps.org/schema/reference/base/water/) dataset, restricted to the German country [division](https://docs.overturemaps.org/schema/reference/divisions/division/)
2. Similarly, use OvertureMaps [segments](https://docs.overturemaps.org/schema/reference/transportation/segment/) also restricted to Germany, and to rail segments only.
3. Find the intersections between the water and rail segments. The output of this should be another set of line segments.
4. We only want the centroids of these line segments as lat/lon points; we want to retain the original segment this came from, and ideally also the id of the water body it overlapped with
5. (optional) Map these lat/lon points back to the line segment and represent them as a %-age distance along the line segment:
    * this would make the crossings a [Connector](https://docs.overturemaps.org/schema/reference/transportation/connector/)  
    * we can enrich our copy of segments with these additional connectors via a [ConnectorReference](https://docs.overturemaps.org/schema/reference/transportation/types/connector_reference/)

For this slice, we should do all this in notebooks, and in particular we should use https://marimo.io/pair with Claude. However, it's fine to be inspired and/or reuse info from existing things like crates/transport/src/overture.rs.

We can visualise outputs either inline in the notebook via geopandas / lonboard. If that doesn't work, we can export as geoarrow and load into kepler.gl.

### Approach notes

- **Engine:** use DuckDB's `spatial` extension from the marimo notebook to read Overture
  GeoParquet directly off S3 and do the intersection in SQL — the minimal-Python
  equivalent of what `crates/transport/src/overture.rs` does with SedonaDB (anonymous
  `us-west-2` read, bbox-pruned). Keep Python to glue + viz only.
- **Datasets:** rail = `theme=transportation` / `type=segment`, `subtype='rail'`, excluding
  `class='tram'` (mirror `EXCLUDED_CLASSES`). Water = `theme=base` / `type=water`.
- **Region:** target four states — **Thüringen, Hesse, Baden-Württemberg,
  Rhineland-Palatinate** — chosen so the extract covers Frankfurt, Mannheim, Sinsheim,
  Speyer and Koblenz (which straddle Hesse / Baden-Württemberg / Rhineland-Palatinate).
  Restrict via Overture `division` (region-level) rather than a single coarse bbox, since
  the states aren't a tidy rectangle. Widen the same pipeline to all of Germany once it
  works. Pin the Overture release (currently `2026-06-17.0`, per `DEFAULT_RELEASE`) so
  extracts are reproducible.

### Tasks

- [x] Set up marimo AI pairing (https://marimo.io/pair): configure marimo's AI assistant so we
      can pair with Claude while building the notebook.
      Note: marimo.io/pair ships as the `marimo-pair` Claude Code plugin (marketplace
      `marimo-team/marimo-pair`), not an in-notebook API-key assistant — Claude Code is the agent.
      Installed at user scope; it drives a live marimo kernel (started with `--no-token`) via a
      `marimo-pair` skill. No `ANTHROPIC_API_KEY` needed.
- [ ] Turn the `notebooks/water_crossings` placeholder into a runnable marimo notebook: add
      `marimo`, `duckdb`, `geopandas`, `shapely`, `lonboard` to `pyproject.toml`; replace the
      stub `main.py` with a marimo notebook; confirm `uv run --project . marimo edit main.py` opens.
- [ ] Resolve the four target states (Thüringen, Hesse, Baden-Württemberg,
      Rhineland-Palatinate) to Overture `division` region geometries to use as the query window.
- [ ] Fetch a rail extract: query Overture `segment` for `subtype='rail'` (excluding
      `class='tram'`) within the four-state region, write to a local GeoParquet.
- [ ] Fetch a water extract: query Overture `water` within the same region, write to a
      local GeoParquet.
- [ ] Compute rail↔water intersections: `ST_Intersection` of rail lines with water geometries,
      producing the overlap line segments; retain the source rail segment `id` and the water
      body `id`.
- [ ] Reduce each intersection to its centroid as a lat/lon point, keeping `rail_segment_id` and
      `water_id`; write this crossings dataset to GeoParquet.
- [ ] Visualise for manual cross-check: overlay rail lines, water, and crossing points in the
      notebook via lonboard/geopandas. Fallback: export geoarrow and load into kepler.gl.
      Spot-check that crossings land where rail visibly meets water.
- [ ] Widen from the four-state region to all of Germany (division-restricted) and re-run, sanity
      checking counts and a few crossings.
- [ ] (optional) Map each crossing point to a %-distance along its rail segment
      (`ST_LineLocatePoint`) to express it as an Overture Connector, and enrich the segment via a
      ConnectorReference.
