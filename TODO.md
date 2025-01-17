- [x] set up initial structure:

  - [x] `apps`:
    - [x] `keks`:
      - [x] `web`
        - [x] minimal svelte app
        - [x] netlify deploy
          - https://houseofmoran-keks.netlify.app
        - [x] available at `keks.houseofmoran.io`
        - [x] plausible analytics setup
      - [x] `backend` cargo workspace
        - [x] minimal axum service
        - [x] deploy to fly.io
        - [x] available at `keks-api.houseofmoran.io`
          - [x] prove by doing a call back to root of API from `keks.houseofmoran.io`
  - [x] `backend` cargo workspace

    - shared across all projects, intended to be for common parts
    - [x] minimal axum service
    - [x] deploy to fly.io
      - https://houseofmoran-geo.fly.dev/
    - [x] available at `geo-api.houseofmoran.io`
      - [x] prove by doing a call back to root of API from `keks.houseofmoran.io`

  - [x] `web`

    - [x] simple vanilla html website
    - [x] netlify deploy
      - https://houseofmoran-geo.netlify.app
    - [x] available at `geo.houseofmoran.io`
    - [x] just contains a link to `keks.houseofmoran.io`
    - [x] plausible analytics setup

- [x] prod checks
  - check that each website is uk, using updown.io:
    - [x] keks.houseofmoran.io
    - [x] keks-api.houseofmoran.io
    - [x] geo.houseofmoran.io
    - [x] geo-api.houseofmoran.io
