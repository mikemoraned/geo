# Winter 2024 TODO's

## Linzer

* [x] update libs
    * [x] rustc version
    * [x] libs
        * [x] change all dependencies to be minor-only versions
        * [x] run `cargo update`
        * [x] run `cargo upgrade --compatible`
* [ ] add hamburg
    * [x] make Justfile more generic
    * [x] separate `data` into `config` and `output` dirs
    * [ ] outstanding problem with running `just --set area hamburg --set profile auto all` (will solve later)
    * [ ] refactor: make `routing` crate repeatable and stable
        * [ ] ...
* [ ] experiment: try different ways to summarise a shape in a normalised way which allows comparison across to similar shapes
    * [x] convert linzer web into a simple rust wasm app website
        * [x] create a basic Rust wasm monolithic module (not components) that says "hello world"
            - following https://dzfrias.dev/blog/rust-wasm-minimal-setup/#fn1
            * [x] install tools:
            ```
            cargo install wasm-pack
            ```
        * [x] deploy on netlify
            - build locally and just check in the wasm
            - deployed current branch `shape-experiment` to `https://linzer.netlify.app`
    * [x] show mapbox and georust working together:
        * [x] simple mapbox map in browser
            * [x] create a public mapbox key for use in netlify
                - restrict to use on https://linzer.netlify.app
                - called `geo-linzer-netlify`
            * [x] create a secret key for this app
                - will only be used locally
                - called `geo-linzer-local`
            * [x] create a `setting.js` module which defines `PUBLIC_MAPBOX_TOKEN` and maps to the secret key; this is not checked in
            * [x] define `PUBLIC_MAPBOX_TOKEN` in netlify
            * [x] update build so that takes a `setting.js.template` file and replaces `PUBLIC_MAPBOX_TOKEN` in it and saves in `setting.js`
        * [x] load a geojson source into map and show it
        * [x] load same geojson source from rust
        * [x] calculate centroids of each shape, using georust
        * [x] draw centroids in mapbox as a source
    * [x] usable UI for centroids
        * [x] ease of use: make each layer in mapbox toggleable on and off
        * [x] speed up / clean up: parse directly into geo-types, without using geojson intermediate step
        * [x] fix geometries: there appear to be a lot of 'degenerate' regions (very small). This really needs to be properly fixed upstream in the region generator, but for now we can probably filter these by size
        * [x] show spinner icon on layer buttons whilst waiting on layer to load
    * [x] show a set of 'rays' for each region
        - 'rays' are a set of linestrings for each region going from the centroid to each border-point
        * [x] refactor wasm into returning an `Annotated` object with a method for `centroids`
        * [x] add `rays` layer
    * [x] show a summary for each region
        - summary is the length of each ray as a fraction of max size of all rays for region
        * [-] visualise as a spiky shape per region in a CanvasSource
            * [x] add a canvas source covering area, displaying a test black rectangle covering entire area
            * [x] to confirm projection is working, draw centroids again on canvas as circles
                - this is sorta-maybe working but is fuzzy (as is a raster) and mapping seems still slightly-off
            * [-] show spikes as a series of arcs
        * [x] switch to layering D3 on top of the map, as in https://franksh.com/posts/d3-mapboxgl/, https://docs.os.uk/more-than-maps/tutorials/apis/d3-overlays-and-mapbox-gl
            * [x] remove previous canvas implementation
            * [x] load D3 and get it showing something minimal
            * [x] draw centroids using D3
            * [x] control display of centroids via top-control
        * [x] calculate summary
        * [x] show summary
    * [ ] show a normalised summary for each region
        - a normalised summary is one where it has a fixed number of degrees and the length per a degree is a single number
        * [ ] try:
            - degrees: 360
            - length: max length of all rays for each degree
    * [ ] ...

## Geo

* [ ] convert top-level geo.houseofmoran.io site into a Zola blog
    
# Geomob 2024

Things that I did for Geomob 2024 presentation

- [ ] minimal component parts:
    - [x] "biscuits" i.e. the parts of a city
        - ideas:
            - route-based
                1. Define a boundary
                2. Sample N locations within that boundary
                3. Find all routes between all pairs
                4. Restrict route lines by boundary
                5. Convert lines into pieces by converting to an image with lines and coalesce the pixels back into boundaries (re-purpose the garibaldi code)
                6. Project those boundaries back into geo shapes
            - road/way based
                1. Define a boundary
                2. Find all OS Ways (e.g. representing roads)
                3. Invert ...
                4. ...
        - impl:
            * [x] setup `houseofmoran-keks` property on stadiamaps
            * [x] get API key for `houseofmoran-keks`
            * [x] fetch route for a fixed pair of points and save as geojson
            * [x] vary routes by mode e.g. walking / cycling / driving
            * [x] fetch routes for N random pairs of points within a boundary and save as geojson
            * [x] make pairs have start/end points on the boundary (this ensures that paths always enclose an area)
            * [x] sample random pairs fairly across the area
                - e.g. sample random pairs of points over a grid
                - or https://www.jasondavies.com/poisson-disc/, https://docs.rs/fast_poisson/latest/fast_poisson/
            * [x] convert routes into pieces
                * [x] convert into black/white Luma image
                * [x] create a Justfile for repeatable recreation of everything
                * [x] find regions in image
                * [x] convert back into geojson
    - [x] layout:
        - [x] binpacking using https://lib.rs/crates/binpack2d
    - [ ] display:
        - [ ] convert to an SVG file with laid-out chunks in correct orientation

- things to add ahead of geomob presentation:
    - [ ] extend to cover a subset of the cities in the original http://www.armellecaron.fr/works/les-villes-rangees/
        - [x] make extraction of point sample grid a separate cli or sub-cli
        - [x] make sampling repeatable e.g. by taking an explicit seed
        - [x] make bounds of area cli args
        - [x] run for some sample cities
            - [x] edinburgh
            - [x] paris
            - [x] new york
            - [x] jerusalem
    - [x] regions:
        - [x] exclude boundary regions i.e. bits on the borders. these make the layouts typically very unbalanced
        - [x] exclude regions whose width, height, or area is > 20% of the respective width, height or area of whole bounding box
    - [x] make `geo.houseofmoran.io` a minimal static site with a blog post (or similar) pointing to slides
    - [x] rename `keks` to `linzer`
- things to fix later:
    - [ ] the random points created by a sample, using `Poisson2D`, are not always uniformly distributed across the rectangle (at least in the way I am using the library)
    - [ ] filter out water bodies as regions

- ideas:
    - turn the speed at which a route is travelled into a boundary property of the resulting shape e.g. if a route is travelled at a fast speed then shorten the length proportionally. this perhaps could be represented as the "springiness" of a bunch of line-segments. intent is to somehow represent the more transient nature of the experience of the area as a shortening or tension in the shape.