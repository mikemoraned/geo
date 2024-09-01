- [ ] set up initial structure:

  - [ ] `apps`:
    - [ ] `keks`:
      - [x] `web`
        - [x] minimal svelte app
        - [x] netlify deploy
          - https://houseofmoran-keks.netlify.app
        - [x] available at `keks.houseofmoran.io`
        - [x] plausible analytics setup
      - [ ] `backend` cargo workspace
        - [ ] minimal axum service
        - [ ] deploy to fly.io
        - [ ] available at `keks-api.houseofmoran.io`
          - [ ] prove by doing a call back to root of API from `keks.houseofmoran.io`
  - [ ] `backend` cargo workspace

    - shared across all projects, intended to be for common parts
    - [ ] minimal axum service
    - [ ] deploy to fly.io
    - [ ] available at `geo-api.houseofmoran.io`
      - [ ] prove by doing a call back to root of API from `keks.houseofmoran.io`

  - [x] `web`

    - [x] simple vanilla html website
    - [x] netlify deploy
      - https://houseofmoran-geo.netlify.app
    - [x] available at `geo.houseofmoran.io`
    - [x] just contains a link to `keks.houseofmoran.io`
    - [x] plausible analytics setup
