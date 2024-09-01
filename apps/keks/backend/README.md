# Setup

Creating:

```
fly apps create --name houseofmoran-keks
```

Certificate:

1. Create a CNAME mapping in DNS from `keks-api.houseofmoran.io` -> `houseofmoran-keks.fly.dev`
2. `fly certs add --app houseofmoran-keks keks-api.houseofmoran.io`

# Deploy

For deploying straight to prod from local code/config:

```
fly deploy --app houseofmoran-keks --config fly.toml
```
