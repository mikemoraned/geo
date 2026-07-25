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
  — the standard equivalent of PostGIS `ST_ClusterDBSCAN` (== connected components within `eps`).
  **Precompute** representatives at a discrete set of radii
  `EPS_VALUES = (5,10,15,20,25,30,40,50,75,100,150,200)`, union into one `reps_all` frame with an
  `eps` column; a `mo.ui.slider(steps=EPS_VALUES)` just *filters* to a level (no clustering on
  drag). Reductions from 3,527 points: 2,090 @5 m, 1,547 @25, **1,415 @100 (default)**, 1,380 @150,
  1,336 @200. Single-linkage *chaining* sets in early where rail runs alongside water — largest
  cluster is 12 @5 m but 64 by `eps` ≥ 15 m — so a smaller `eps` keeps near/parallel crossings
  more distinct. **But** the Mannheim spot-check (below) shows a single real crossing's overlaps
  can spread ~100 m, so correct merging needs the *large* end of the range — hence the default is
  **100 m**.
  (NB: an earlier hand-rolled union-find gave ~1,610/largest-28 @25 m; that was buggy —
  DBSCAN == scipy connected_components == 1,547/largest-64, verified in-kernel.)
- **Scope:** purely spatial — merge crossings within `eps` regardless of `water_id` (collapses
  the poly+centreline-of-same-river case, the majority of dups). **Caveat:** this will also
  merge ~4 m-apart **parallel-track** crossings, which we want to keep — an accepted limitation
  of the DBSCAN route. If preserving parallel tracks matters more than simplicity, prefer the
  **V3b merge-first** approach, which keeps them naturally (contiguous segments line-merge per
  track; unconnected parallel tracks stay separate).
- **Representative:** keep the **largest-`overlap_m`** crossing per cluster, at its **real
  location** (not the centroid, so it stays on rail∩water), tagged with a `cluster_size`.
- **UI:** a `collapse near-duplicate crossings` checkbox + the `eps` slider; the map swaps the
  full crossing set for the selected-level representatives reactively. Raw crossings stay underneath.

Tasks:
- [x] Copy `v2.py` → `v3.py`; add `scikit-learn` to `v3.py` deps (v2 stays clean).
- [x] Add a DBSCAN dedup cell — precomputes `reps_all` across `EPS_VALUES` (`min_samples=1`,
      largest-`overlap_m` representative + `cluster_size`, tagged with `eps`).
- [x] Wire `collapse_nearby` checkbox + `eps` slider (filters `reps_all`) into the map; export
      `crossing_reps.parquet` (all `eps` levels) to `data/water/v3/`.
- [x] Spot-check on the map: confirm dense blobs collapse to one sensible marker and no legit
      distinct crossings are wrongly merged.
      Finding: a real **bridge in Mannheim** only merges into a single representative at
      **`eps` = 100 m** — the overlap areas of one physical crossing spread that far (many split
      segments + poly-outline-and-centreline). So in practice the *larger* end of the range is
      what's needed for correct merging; the earlier "try 5–10 m" intuition undercounts real
      crossings. Slider now defaults to **100 m** (accepting that parallel tracks merge at that
      radius); `EPS_VALUES` extended to 150 & 200 m in case some crossings spread even wider.

#### V3b

To be built in a **new `notebooks/water_crossings/v3b.py`** — a verbatim copy of `v2.py`
(export dir auto-resolves to `data/water/v3b/`). Leave `v2.py` untouched.

Same de-duplication problem as V3, fixed at **source** instead of by clustering: line-merge
contiguous rail **before** intersecting, so one physical crossing yields one overlap component
rather than one-per-segment. Preferred over V3 because it **preserves parallel-track crossings**
(unconnected tracks don't line-merge) and needs no arbitrary `eps`.

> **⛔ Stage 1 finding (measured 2026-07-24, four-state extract) — merge-first does NOT dedup; V3b parked.**
> - Rail line-merge collapses 44,096 → 38,793 contiguous lines (`ST_LineMerge(ST_Collect(list(geometry)))`;
>   `ST_Union_Agg` is the *wrong* tool — it nodes at junctions and *fragments* to 70,230).
> - Crossings recomputed from the merged rail: **3,523 vs V2's 3,527** — essentially unchanged. So
>   the near-duplicate overlaps are **not** caused by split rail segments (the V3b premise is refuted).
> - The duplication is **water-side + spatial**: unioning the crossed water (the V4 idea) drops
>   crossings 3,527 → ~2,553 — point crossings 2,281 → 1,324 as centrelines get absorbed into
>   polygons; the remainder DBSCAN collapses is separate-but-nearby water bodies + parallel tracks.
> - Consequences: (1) Stage 2 (re-attach) would build on a no-op; (2) **V3 already delivers deduped
>   + segment-tied output** — its DBSCAN representatives are real crossing rows, so they keep
>   `rail_segment_id`/`water_id` (the V2 schema, deduped); (3) **V4 water-union is the real lever**
>   for the poly+centreline case. Rail-merge is parked as a tested dead-end *for dedup*.
> - `v3b.py` remains a clean v2 clone (no Stage-1 cells committed).

The original two-stage plan is kept below for the record.

**Two stages:**

*Stage 1 — dedup by merging rail.* Build maximal contiguous rail lines, then intersect with
water and reduce as in V2 (5 m filter, substantial classes, `ST_Dump` → centroids).
- Merge with `ST_LineMerge(ST_Collect_Agg(geometry))` (collect, no noding) rather than
  `ST_Union_Agg` if available — `ST_Union` nodes at every rail–rail junction, which is costly on
  ~44 k segments and unnecessary (we only want to join segments sharing endpoints).
- Still does **not** merge the poly-outline + centreline duplicate of the same river (that's two
  *water* features, untouched by a rail merge). Left as-is in V3b (keep the line/point toggle) —
  **deferred to V4**, which unions all water into a single area so the centreline disappears.

*Stage 2 — re-attach each deduped crossing to a specific original segment* (to restore the V2
schema: segments-with-crossing-points, and to enable the Connector idea in V4).

Critique of the proposed "for each crossing, find **all** segments within N m and map onto
**each**": mapping to *all* nearby segments re-introduces the duplication we just removed, and a
metric N-search can grab the wrong parallel track. Refinements:
- Attach each crossing to its **single owning** segment, not all within N. Prefer a **topological**
  match — the deduped overlap line lies *on* the original segments by construction, so
  `ST_Intersects` / `ST_DWithin(point, seg, small_tol)` picks the owner; break ties by nearest
  (`min ST_Distance`). Use N only as a **small tolerance** (~2–5 m) to absorb the fact that the
  centroid of a *curved / areal* overlap sits slightly off the polyline — not as a wide search.
- Then project onto that segment: `frac = ST_LineLocatePoint(seg, pt)` and
  `snapped = ST_LineInterpolatePoint(seg, frac)`. This yields an on-segment point **and** the
  %-distance — i.e. V4's Overture Connector falls straight out of this step.
- Output schema mirrors V2 `crossing_points` (`rail_segment_id`, `water_id`, `overlap_m`,
  `overlap_kind`, `lat`/`lon`) plus `frac`, now deduped and segment-tied.

**Edge cases:**
- **Long bridges** (e.g. a Rhine crossing spanning several segments over ~100 m+): the physical
  crossing legitimately touches multiple segments. **Decided:** attach the representative to the
  **one** segment under its centroid (nearest-owning) — no per-segment breakdown; this suits the
  downstream use.
- Parallel tracks closer than `tol` could cross-attach; mitigate with small `tol`, or by
  remembering which merged line a crossing came from and restricting candidate segments to that
  line's constituents (needs carrying constituent ids through the merge).

Alternatives considered: (a) dedup via the **connector graph** — group crossings on the same
maximal degree-2 rail path + water body, keeping ids natively (no geometry merge) but needs graph
construction, more code; (b) full `ST_Union_Agg` noding — simpler call, but costly and splits at
junctions. Line-merge + topological re-attach is the recommended standard route.

Tasks:
- [x] Copy `v2.py` → `v3b.py`.
- [x] Stage 1: line-merge contiguous rail, re-run the intersect/filter/dump; compare counts.
      **Result: no meaningful dedup (3,527 → 3,523) — see finding above. V3b parked.**
- [-] Stage 2: re-attach each crossing to its single owning segment — moot without a working
      Stage 1; the `ST_LineLocatePoint` → `frac` (Connector) idea moves to V5 if still wanted.

#### V4:

Built in a **new `notebooks/water_crossings/v4.py`** — a verbatim copy of `v2.py` plus the
water-dedup below (export dir → `data/water/v4/`). Leave `v2.py` untouched.

Same as V2 but also:
* [x] we account for some water bodies being represented both as a centre-line and an area by doing a union of all water bodies into a single area, which should have the effect of the centre-line disappearing.
      Implemented as a **targeted drop** (chosen over a literal full union, to preserve the V2
      schema — `water_id` / `water_class` / `water_subtype` stay intact): in `crossing_points`,
      drop each **point** crossing whose location lies inside an **areal water polygon**
      (`ST_Contains`, bbox-pruned). Rationale: a river stored as both polygon and centreline gives
      a redundant point there — the rail also crosses the polygon as a line, which already
      represents it. Net: **3,527 → 2,568** (point crossings 2,281 → 1,322 — remaining points are
      centreline-only rivers/canals with no polygon; line crossings unchanged at 1,246).
      Equivalent to unioning water polygons and removing centrelines they cover, but per-body
      identity is kept. (Adjacent distinct polygons are not merged — a minor effect here.)

#### V5:

Built in **`notebooks/water_crossings/v5.py`** — cloned from `v4.py` (keeps V4's centreline-drop;
export dir → `data/water/v5/`). Leave earlier notebooks untouched.

**Goal:** one crossing per **(physical track, water body)** — e.g. the Mannheim rail bridge =
4 tracks × 1 river = **4** crossings — while (a) preserving parallel tracks and (b) *not*
collapsing a **horseshoe** water body that loops back to cross the **same** track in two
genuinely separate places.

**Why the simpler collapses fall short** (measured on the four-state V4 output):
- `ST_Dump` per part → **2,568** (Mannheim = 12: the river polygon has **6 island holes**, so each
  track's span splits into 3 channels — *not* split segments).
- One per **(rail_segment, water_id)** → **2,164** (Mannheim = 4 ✓) — but a horseshoe river
  re-crossing the *same* segment far away would wrongly collapse to one mid-point.
- One per **(connector-component, water_id)** → **1,811** — also merges *split bridges* (a track
  cut into several connected segments over the water; ~31% of crossing segments connect to another:
  1,524 → 1,054 components) and still keeps parallel tracks (unconnected → separate components).
  Same horseshoe blind spot.
- V3 DBSCAN @100 m → 1,415, but it **over-merges parallel tracks** (Mannheim = 1 ✗).

**Approach — connector-component grouping + a within-group distance merge:**
1. Add `connectors` to the rail extract; build adjacency among **crossing** segments that share a
   connector id → a `component_id` = a maximal run of connected crossing segments = the *physical
   track* at that crossing. (Robust via Overture connector ids — geometry line-merge under-performed
   here, see V3b.)
2. Reduce: link kept parts that share a **(component_id, water_id)** and are within distance `D`
   (projected metres); keep one representative per resulting group (largest-`overlap_m` part → keeps
   a real `rail_segment_id`; total spanned length + part count as extra columns).
3. Both steps are **connected-components** problems, handed to **scipy** (`csgraph.connected_components`
   for the graphs, `spatial.cKDTree.query_pairs` for the distance edges) — no hand-rolled union-find.
4. Because the spatial merge is **scoped to one (component, water_id)**, `D` is safe to make
   generous (~100–200 m): it only distinguishes *islands / bridge spread* (close → one) from a
   *horseshoe re-crossing* (far → two). Parallel tracks (different component) and different water
   bodies are already separated by the scoping — so **none of V3's chaining / parallel-track
   problems apply**. Expose `D` as a slider.

Cases handled: islands (C1) ✓, parallel tracks kept (C2) ✓, split bridges (C3) ✓, poly+centreline
(C4, via V4 drop) ✓, horseshoe re-crossing kept-as-two (C5) ✓.

**Edge cases:**
- Parallel tracks that interconnect at a junction *right at* the water share a connector → one
  component → would merge. Rare at open-water bridges; spot-check.
- `D` choice: large enough to span the widest island-braided bridge, small enough to separate the
  tightest horseshoe. Tunable; the scoping makes it forgiving.

Tasks:
- [x] Clone `v4.py` → `v5.py`.
- [x] Add `connectors` to the rail extract; build `component_id` via scipy connected-components
      over shared connector ids (restricted to crossing segments).
- [x] Refactor: replace the hand-rolled union-find + O(k²) distance loops with scipy
      (`csgraph.connected_components` + `spatial.cKDTree.query_pairs`); behaviour unchanged
      (2,568 → 2,115; Mannheim = 4).
- [x] Split the reduction into three cells — (1) segment→physical-track components, (2) attach to
      kept parts, (3) cluster→representatives — each rendering a lonboard map coloured by
      physical-track component so each step is visible. Colours come from matplotlib's categorical
      `tab20` (cycled), not a hand-rolled hash; `component_labels` is a thin scipy wrapper.
- [x] Reduce to one crossing per `(component_id, water_id)` sub-clustered by distance `D`; keep the
      largest-`overlap_m` representative (real `rail_segment_id`, `component_id`, `water_id`, plus
      `merged_parts` and `total_overlap_m`). Emit the V2-shaped schema; export `crossing_reps.parquet`
      to `data/water/v5/`.
- [x] `D` slider (`merge_dist`, 25–500 m, default 100) + `collapse_v5` checkbox in the map.
      Result at 100 m: **2,568 → 2,115**; **Mannheim river = 4** (4 components, each with its 3
      island-channels merged, `merged_parts=[3,3,3,3]`). Larger `D` trends toward the full
      (component,water) collapse (1,811); smaller `D` keeps more horseshoe-style re-crossings apart.
      (Left as follow-up: add the Mannheim case to the test dataset and confirm each rep sits on an
      in-bbox segment.)

### Test Dataset Extraction

(Cross-cutting, not tied to one version — captured now, build later.)

Build a small **library of test cases**, each specified by a **bounding box** plus expected
assertions, so we can validate a pipeline version's dedup behaviour against hand-verified truth.

- **First case — Mannheim bridge area:** expect **4** crossings, and **each** crossing should lie
  on a rail segment within that bbox. (This is the *desired* answer: it passes for V5, fails for
  V2/V4 which report 12 — so the suite doubles as a target for the dedup work.)
- **Interactive capture:** find the bbox interactively (e.g. read the lonboard map viewport, or a
  small marimo UI to draw/enter a rectangle), then **append** it to a checked-in dataset.
- **Storage: GeoJSON** (`notebooks/water_crossings/test_cases.geojson`) — a `FeatureCollection`
  where each test case's **bbox is a Feature** (a `Polygon` rectangle) and the **assertions ride as
  custom `properties`** on that Feature: `name`, `expected_crossings`, optional per-crossing
  expectations, free-text `notes`. `properties` is GeoJSON's standard extension point, so the file
  stays valid and is directly viewable in any GeoJSON tool / lonboard. If we want to assert
  *locations*, expected crossings can be added as extra `Point` Features tagged with the case `name`.
- **Assertion shape:** run the pipeline restricted to the bbox and check `count == expected_crossings`
  and that every crossing's `rail_segment_id` resolves to a segment intersecting the bbox. Keep
  assertions version-aware (the "correct" count is the V5 target; earlier versions may differ).

Tasks:
- [x] Storage format decided: **GeoJSON** — bbox as a `Polygon` Feature, assertions as custom
      `properties` (with optional `Point` Features for expected crossing locations).
- [x] Seed the first case: `test_cases.geojson` with **Mannheim bridge = 4** (bbox derived from the
      four crossing segments' extent). `notes` kept short — no slice/implementation detail.
- [x] A small runner extracted to a local module `crossing_checks.py` (`load_cases` / `run_cases`),
      imported into `v5.py`. Checks each case against the V5 output — reps inside the bbox count ==
      `expected_crossings`, and each such rep's segment intersects the bbox. Mannheim passes
      (found 4, count_ok, segments_ok).
- [x] Interactive bbox-capture in `v5.py`: a `mo.ui.anywidget`-wrapped lonboard map showing rail
      (grey) + V5 crossings (red) + city markers (hover for names), so you can see what a case
      covers. **Capture = the map's visible area**: pan/zoom so the target fills the view, and the
      bbox is derived from the synced `view_state` (centre + zoom, deck.gl web-mercator: `512*2**zoom`
      px span the globe; nominal 1000×560 px map). Name / expected inputs, a Refresh-from-view button
      showing the live in-box rep count, and an Append button that writes the case via
      `crossing_checks.add_case`.
      History: first tried lonboard's on-map box-select (`selected_bounds`) — undiscoverable in this
      build; then a centre + `box_size_m` slider with a second **preview map** — that extra
      re-rendering WebGL map is what blew up Safari's memory (not the rail on a single map). Settled
      on visible-area + one map (rail kept). If memory is still tight, the three V5 step maps + main
      viz each redraw the full 44k-segment rail — those could drop to the ~1.5k crossing segments.
- [x] Extract the capture into a reusable **`bbox_capture.py`** module (`make_capture` builds the
      widgets, `controls` lays them out, `visible_bounds` maps view_state→bbox, `result` previews +
      appends via `crossing_checks.add_case`) so any notebook can drop it in. `v5.py`'s two capture
      cells are now thin wrappers. Constraint baked in: the UI elements must be **top-level notebook
      globals** — marimo UI reactivity does *not* fire for elements held as object attributes
      (verified in-kernel), so the module *returns* them for the notebook to bind, rather than
      hiding them in a widget object.

#### V6:

Same as V5 but also:
- [x] Widen from the four-state region to all of Germany (division-restricted) and re-run, sanity
      checking counts and a few crossings.
      Built in `notebooks/water_crossings/v6.py` (clone of `v5.py`; region cell uses the Germany
      **country** division as the single clip boundary — no per-region/state list). All-Germany
      counts: rail 180,329, water 966,385, crossings 43,565,
      crossing_points 8,859 (5,132 line / 3,727 point), V5 reps @100 m 7,362. Test cases pass
      (Mannheim = 4, horseshoe = 4). Exports in `data/water/v6/`.

#### V7:

I've spotted an issue in the dataset in the area around Hamburg: we are finding water crossings when the route is underwater 😀 . Looking at the [explorer](https://explore.overturemaps.org/?feature=transportation.segment.e3ce6201-156c-4765-b6b7-869885ae6c4d#18.04/53.554312/9.99713) for this region, and I can see that it has an attribute "rail_flags" which has values like `between: [0.192236095, 0.265829864] values: [is_tunnel]`. We need to process these to exclude water crossings that are in a tunnel.

This actually nicely fits with our previously optional plan to map a crossing back to it's fractional position along a segment. We can use this alongside the `between` value to exclude any water crossings where it has the `is_tunnel` property.

- [x] Add test-case for Hamburg asserting that no crossings should be found in the area under water (`Hamburg under water`).
- [ ] Map each crossing point to a %-distance along its rail segment
      (`ST_LineLocatePoint`) to express it as an Overture Connector, and enrich the segment via a
      ConnectorReference.
- [ ] Filter out any water crossings where `is_tunnel` property is present.
- [ ] Look for any other properties like `is_tunnel` which indicate that we shouldn't include it as a water crossing (see schema for `RailFlag` on OvertureMaps [website](https://docs.overturemaps.org/schema/reference/transportation/types/rail_flag/))

