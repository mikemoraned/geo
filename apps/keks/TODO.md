- [ ] minimal component parts:
    - [ ] "biscuits" i.e. the parts of a city
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
            * [ ] fetch routes for N random pairs of points within a boundary and save as geojson
            * [ ] sample random pairs fairly
                - e.g. sample random pairs of points over a grid
            * [ ] ...
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