# Setup

Creating:

```
fly apps create --name houseofmoran-geo
```

Certificate:

1. Create a CNAME mapping in DNS from `geo-api.houseofmoran.io` -> `houseofmoran-geo.fly.dev`
2. `fly certs add --app houseofmoran-geo geo-api.houseofmoran.io`

# Deploy

For deploying straight to prod from local code/config:

```
fly deploy --app houseofmoran-geo --config fly.toml
```
