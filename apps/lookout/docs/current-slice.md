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

#### V1:

- [x] Set up marimo AI pairing (https://marimo.io/pair): configure marimo's AI assistant so we
      can pair with Claude while building the notebook.
      Note: marimo.io/pair ships as the `marimo-pair` Claude Code plugin (marketplace
      `marimo-team/marimo-pair`), not an in-notebook API-key assistant — Claude Code is the agent.
      Installed at user scope; it drives a live marimo kernel (started with `--no-token`) via a
      `marimo-pair` skill. No `ANTHROPIC_API_KEY` needed.
- [x] Turn the `notebooks/water_crossings` placeholder into a runnable marimo notebook: add
      `marimo`, `duckdb`, `geopandas`, `shapely`, `lonboard` to `pyproject.toml`; replace the
      stub `main.py` with a marimo notebook; confirm `uv run --project . marimo edit main.py` opens.
- [x] Resolve the four target states (Thüringen, Hesse, Baden-Württemberg,
      Rhineland-Palatinate) to Overture `division` region geometries to use as the query window.
- [x] Fetch a rail extract: query Overture `segment` for `subtype='rail'` (excluding
      `class='tram'`) within the four-state region, write to a local GeoParquet.
- [x] Fetch a water extract: query Overture `water` within the same region, write to a
      local GeoParquet.
- [x] Compute rail↔water intersections: `ST_Intersection` of rail lines with water geometries,
      producing the overlap line segments; retain the source rail segment `id` and the water
      body `id`.
- [x] Reduce each intersection to its centroid as a lat/lon point, keeping `rail_segment_id` and
      `water_id`; write this crossings dataset to GeoParquet.
- [x] Visualise for manual cross-check: overlay rail lines, water, and crossing points in the
      notebook via lonboard/geopandas. Fallback: export geoarrow and load into kepler.gl.
      Spot-check that crossings land where rail visibly meets water.

#### V2:

Built in `notebooks/water_crossings/v2.py` — a verbatim copy of `v1.py` plus the items
below. It reads the local Overture mirror (`/Volumes/PRO-G40/OvertureMaps/data/release/
<RELEASE>`) when present (much faster than S3, with S3 fallback), and `EXPORT_DIR` is now
derived from the notebook filename so outputs land in `data/water/v2/` (v1/v2 don't clobber).

Same as V1 but also:
- [x] try to elimate very short crossings i.e. where we wouldn't be able to see anything from a train because we went by too fast. We probably want to tune tune this, but we can start by eliminating any intersections between rail and water where the length, or diameter, is <= 5 metres. This means we probably need to do the conversion to the appropriated projected CRS so we can have the right units.
      Decision: overlap length is measured in a metric CRS (`PROJECTED_CRS = EPSG:25832`,
      UTM 32N). The "≤ 5 m" rule only meaningfully applies to **areal-water** crossings,
      whose overlap is a line (= the width of water the track spans). **Linear-watercourse**
      crossings (rail over a river/stream/canal centreline) are *points* with length 0, so
      rather than dropping them all we keep them by class:
      `SUBSTANTIAL_WATER_CLASSES = (river, canal, fairway, water)` — dropping stream / ditch
      / drain. Net effect: 12,801 raw crossings → **3,527 kept** (1,246 line overlaps > 5 m,
      2,281 point crossings of substantial classes). Threshold + class set are tunable constants.
- when visualising this:
    - [x] it'd be nice to identify areas by name, so let's add names of cities to the lonboard map
          Decision: cities = Overture `division` localities with `population ≥ 50,000`
          (`CITY_MIN_POPULATION`; 48 in-region — Frankfurt, Stuttgart, Mannheim, …).
          Caveat: lonboard 0.16 has **no TextLayer**, so a city's name shows on **hover**
          (marker tooltip), not as an always-on label.
    - [x] let's visualise the size of the overlap by:
        - making the actual centre of overlap a very small point (e.g. 3 pixels)
        - drawing a small open circle whose radius is proportional to the size of the overlap
          Done as two `ScatterplotLayer`s: a 3 px red centre dot, plus an open circle
          (`stroked`, `filled=False`) whose radius in **metres** = half the overlap length.
          Point crossings have length 0, so they show just the centre dot (no circle).
- [x] Toggle overlap classes: each crossing carries an `overlap_kind` (`line` | `point`),
      and `show_lines` / `show_points` `mo.ui.checkbox`es let the map hide/show LINESTRING
      (areal-water) and POINT (linear-watercourse) overlaps independently (map redraws reactively).

#### V3:

To be built in a **new `notebooks/water_crossings/v3.py`** — a verbatim copy of `v2.py`
plus the de-duplication below. Leave `v2.py` untouched.

I am seeing lots of cases where overlaps are very close to each other. This makes sense as each segment is separate but could be very short, so multiple segments all interact with the same water body. I'd like to minimise close-by overlaps to just one representative example.

Original ideas: (1) agglomerative clustering, traverse the tree keeping one per sub-tree
where all entries are within some distance; (2) k-means k=3 with a max-distance limit,
one per cluster; (3) hexbin, one per bin.

**Analysis (data-grounded on the four-state V2 output, 3,527 points):**
- Root cause is threefold: Overture splits the rail line into many short **segments** (each
  intersects the water separately); the same river is often stored as **both an areal polygon
  and a centreline** (different `water_id`s → a line-overlap *and* a point-overlap at the same
  spot); and **parallel/multi-track** rail doubles crossings.
- **Requirement (added):** parallel-track crossings must be **kept distinct** — we *want* both
  markers where an up/down line each crosses a river. This is in tension with distance-based
  clustering: parallel tracks are only ~4 m apart, the same scale as split-segment duplicates,
  so DBSCAN cannot separate the two by distance alone (see the scope caveat below).
- 81% of points have a neighbour ≤ 50 m, but only ~41% of nearest neighbours share the
  `water_id` → this is **inherently spatial**; a water-id grouping would miss ~59% of dups.
- Idea critique: (1) is fine but is just single-linkage cut at a distance (== DBSCAN
  `min_samples=1`), O(n²), doesn't scale to all-Germany; (2) **rejected** — k-means uses a
  *global* k and has no distance cutoff, wrong shape; (3) standard for *display* aggregation
  but grid-boundary artifacts, and this DuckDB build has **no H3** (only hex-encoding utils).

**Decisions:**
- **Method:** DBSCAN in a metric CRS (`EPSG:25832`) via `sklearn.cluster.DBSCAN(eps, min_samples=1)`
  — the standard equivalent of PostGIS `ST_ClusterDBSCAN`. `eps` exposed as a `mo.ui.slider`
  (default **25 m** — sweet spot: 3,527 → ~1,610 reps / 54% fewer; larger `eps` barely dedups
  more but starts single-linkage *chaining* where rail runs alongside water).
- **Scope:** purely spatial — merge crossings within `eps` regardless of `water_id` (collapses
  the poly+centreline-of-same-river case, the majority of dups). **Caveat:** this will also
  merge ~4 m-apart **parallel-track** crossings, which we want to keep — an accepted limitation
  of the DBSCAN route. If preserving parallel tracks matters more than simplicity, prefer the
  **V3b merge-first** approach, which keeps them naturally (contiguous segments line-merge per
  track; unconnected parallel tracks stay separate).
- **Representative:** keep the **largest-`overlap_m`** crossing per cluster, at its **real
  location** (not the centroid, so it stays on rail∩water), tagged with a `cluster_size`.
- **UI:** a `collapse near-duplicate crossings` checkbox + the `eps` slider; the map swaps the
  full crossing set for the representatives reactively. Raw crossings stay underneath.

Tasks:
- [ ] Copy `v2.py` → `v3.py`; add `scikit-learn` to `v3.py` deps (v2 stays clean).
- [ ] Add a DBSCAN dedup cell producing `reps_gdf` (`eps` from slider, `min_samples=1`,
      largest-`overlap_m` representative + `cluster_size`).
- [ ] Wire `collapse_nearby` checkbox + `eps` slider into the map; export `crossing_reps.parquet`
      to `data/water/v3/`.
- [ ] Spot-check: confirm dense blobs collapse to one sensible marker and no legit distinct
      crossings are wrongly merged (watch the chaining case at higher `eps`).

#### V3b

Same de-duplication problem, fixed at **source** instead of by clustering: `ST_LineMerge`/
`ST_Union` contiguous rail (and/or dissolve water per body) **before** intersecting, so a
single crossing yields one overlap component rather than one-per-segment.
- Removes the *split-segment* cause and **correctly preserves parallel-track crossings**
  (unconnected tracks don't line-merge) — which matches the requirement above, unlike V3.
- Does **not** by itself merge the poly+centreline duplicate (a river's areal outline and its
  centreline are separate features), so it may still want a light same-spot merge for that case
  only. Worth building to compare representative counts and locations against the V3 DBSCAN approach.
- [ ] Prototype the merge-first pipeline; compare kept-crossing counts and a few locations vs V3.

#### V4:

Same as V3 but also:
- [ ] Widen from the four-state region to all of Germany (division-restricted) and re-run, sanity
      checking counts and a few crossings.
- [ ] (optional) Map each crossing point to a %-distance along its rail segment
      (`ST_LineLocatePoint`) to express it as an Overture Connector, and enrich the segment via a
      ConnectorReference.
