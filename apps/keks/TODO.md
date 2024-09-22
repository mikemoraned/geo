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
    - [ ] layout:
        - ideas:
            - binpacking
                - maybe re-use library from speculaas-biscuits
    - [ ] display:
        - ideas:
            - simple transition:
             - project pieces onto map as an overlay
             - show layout as an image
             - transition via transparencye.nv


- ideas:
    - turn the speed at which a route is travelled into a boundary property of the resulting shape e.g. if a route is travelled at a fast speed then shorten the length proportionally. this perhaps could be represented as the "springiness" of a bunch of line-segments. intent is to somehow represent the more transient nature of the experience of the area as a shortening or tension in the shape.