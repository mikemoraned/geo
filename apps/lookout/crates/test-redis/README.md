# test-redis

Starts a redis container for a test, and waits until redis answers. Tests that need redis take
this crate as a dev-dependency, so they share one container setup.

`TestRedis::start` runs the `redis:7-alpine` image. The redis module of `testcontainers-modules`
defaults to [5.0][module], and [`GETDEL`][getdel], which the sign-in stores use, needs 6.2.

Docker maps the container's port before redis accepts connections, so a first connection can be
refused. `start` therefore connects and sends `PING` every 100 ms, for up to 30 seconds, and
returns once redis replies.

Dropping a `TestRedis` stops its container. A test keeps it in scope until the test finishes.

[module]: https://docs.rs/testcontainers-modules/0.15.0/src/testcontainers_modules/redis/standalone.rs.html
[getdel]: https://redis.io/docs/latest/commands/getdel/
