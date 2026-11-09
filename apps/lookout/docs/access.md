# Access

Who can record telemetry, how a device gains and loses that right, and which risks the design
accepts. The [server's README](../crates/server/README.md) describes the mechanics: the
ceremonies, the records, and the session. [The browser](web.md) describes what the pages show.

## Who can record

Recording means opening `/record` and writing samples over its websocket. Both need a signed-in
session. Every other page writes nothing, and stays open to anyone.

A session is signed in while the passkey that opened it is allowed. Every allowed passkey belongs
to one account, `recording`, and grants the same access. No passkey has more rights than another,
and none can allow or revoke a passkey.

The admin is whoever holds the redis URL. Allowing and revoking a passkey are writes to redis, made
from the command line. No page can make them.

## How a device gains access

1. The device registers a passkey on `/signin/`. The server keeps the passkey as pending, under a
   short code, and the page shows the code.
2. The device's owner passes the code to the admin.
3. The admin allows the code within an hour, before the pending passkey expires.
4. The device signs in with the passkey, which leaves a session.

The admin learns from a code only that some device registered it. Nothing on the code says whose
device it is, so the admin allows a code only when it comes from the owner of the device.

A passkey synced through a password manager reaches every device that shares the vault. Each of
those devices can then sign in without registering again.

A server that keeps its records in memory skips the admin for a passkey registered for
`localhost`, and allows it at once. No admin can reach that server's memory. A hosted server's
origin is not `localhost`, so a hosted server in memory allows no passkey.

## How access ends

- **Revoking a passkey** ends every session it opened. The next request from such a session is
  refused, and an open websocket closes within a minute.
- **Signing out** ends that one session.
- **Disuse** ends a session 30 days after its last request.

A device whose session ended keeps its passkey. If the passkey is still allowed, the device signs
in again. If it was revoked, the device registers again, and the admin allows the new code.

## Risks the design accepts

- **Anyone can register.** A stranger's registration holds a record on the server for at most an
  hour, and a stranger's unfinished ceremony for at most 10 minutes. No limit applies to how many
  a caller starts.
- **The redis URL is the root of trust.** Whoever holds it can allow any pending passkey, and can
  already push samples onto the queue directly.
- **A signed-in device records until its access ends.** A lost phone with a session records for
  up to 30 days after its last use, unless the admin revokes its passkey first.
- **A revoked passkey's open socket lasts up to a minute.** The server rechecks an open socket's
  session once a minute, not on every sample.
- **Synced passkeys give no clone detection.** They report a signature counter of 0, which
  disables the counter check. Each device that shares the vault holds the same passkey, and
  revoking it ends access on all of them.
- **A server in memory trusts its network.** It listens on every interface, and a client on the
  same network can claim a `localhost` origin. That client's passkey is allowed at once. A server
  in memory logs samples and stores none, so such a client writes nothing.
