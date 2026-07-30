# Spike 4 — BLE out

Spike 3's GNSS core and screen, plus a BLE GATT service that notifies each new position to a
subscribed client. Completes the slice: time + lat/lon on the screen *and* over BLE.

> **`crux_core` is pinned to `=0.16.2` deliberately.** On 0.19 this spike reboots every
> 4s–7min once BLE is enabled; on 0.16.2 it runs indefinitely. The cause was never identified,
> only avoided — see [Known issue](#known-issue-intermittent-reboot-with-ble-enabled) before
> changing that pin.

```sh
just test      # run the core's tests on the laptop
just flash     # build, flash, and tail the serial console
```

## What to connect to

Advertises as **`lookout-spike4`**. In a BLE explorer (LightBlue on the laptop works):

| | |
|---|---|
| Service | `10000000-0000-4000-8000-000000000001` |
| Characteristic | `10000000-0000-4000-8000-000000000002` |
| Properties | READ + NOTIFY |
| Value | `50.86281,12.18576` — decimal degrees, UTF-8 |

Subscribe to the characteristic to get a notification per new position. Reading it without
subscribing returns the latest one, or `no fix yet` before the first.

The payload is deliberately UTF-8 text rather than packed binary, so a generic explorer shows
something legible without a custom decoder. A real client would want something tighter.

## Publishing is a core decision, not a shell one

The core emits a **second effect** alongside `Render`:

```rust
#[effect]
pub enum Effect {
    Render(RenderOperation),
    Broadcast(BroadcastOperation),
}
```

`BroadcastOperation` carries the finished payload, so the core owns both the wire format and
the judgement of when publishing is worthwhile — it only broadcasts when the position
actually *changes*, which matters because the receiver emits a dozen-plus sentences a second
and most repeat the position already sent. All of that is asserted in host tests; the shell
just calls `set_value(...).notify()` on whatever arrives.

This is the shape the predictor wants: predictions are effects, and deciding what to emit is
exactly the logic worth testing off-device.

## Known issue: intermittent reboot with BLE enabled

**Status: avoided by pinning `crux_core = "=0.16.2"`. Cause unidentified.**

On `crux_core` 0.19 the device reboots on its own between 4 seconds and 7 minutes after boot,
with or without a client connected. Spike 3 — the same core, display and GNSS code without
Bluetooth — runs indefinitely, so enabling BLE is what brings it out.

On 0.16.2 the same spike ran 30 minutes with a client connected *and* a real fix, so both the
`render` and `Broadcast` effect paths were exercised. The port between the two versions is a
single associated type (`type Capabilities = ()`), which later versions dropped — the
`#[effect]` API is otherwise identical, so the pin costs almost nothing today.

What that narrows it to: something introduced between 0.17 and 0.19. Bisecting those two would
locate it precisely, at one flash and a 15-minute soak each; not done, because the pin already
unblocks the work and we are not reporting it.

**The fault is always inside crux's per-effect machinery.** Two signatures, both reproducible:

```
crux_core::command::Command::new → Box::new_uninit → CommandContext::spawn
  → CommandContext::clone → crossbeam_channel::Sender::clone
LoadProhibited, EXCVADDR = 0x00000000, A2 = 0x00000000     ← null `&self`
```

```
Double exception, EXCVADDR = 0xffffffe0, backtrace an endless repeat of
posix_memalign / _DoubleExceptionVector, with crossbeam Receiver::try_recv on top
```

The first is a null pointer dereference in a context crux has just constructed; the second is
runaway recursion through the allocator. `App::update` returns a `Command` for every event, and
each one allocates channels, an `Arc` and a slab entry — so this path runs constantly.

**Ruled out by measurement, not argument:**

| Suspect | Evidence against |
|---|---|
| Stack overflow | `CONFIG_FREERTOS_WATCHPOINT_END_OF_STACK` never fired; main's high-water is a constant 22,812 bytes free of 49,152 |
| NimBLE host task stack | 6,492 of 8,192 free when callbacks run; raising it and removing all logging from callbacks changed nothing |
| Heap exhaustion or fragmentation | Free heap flat to ±8 bytes and largest block *identical* (110,592) across 7 minutes, right up to the crash |
| Heap buffer overrun | `CONFIG_HEAP_POISONING_COMPREHENSIVE` ran across crashes and never reported a damaged block |
| PSRAM in the general heap | Same crash with PSRAM disabled entirely |
| Allocation churn | Cutting events ~15x (only `RMC`/`GGA` reach the core) did not stop it |
| Model on the fragmented heap | Same crash with the core un-boxed, back on main's stack as in spike 3 |

Guesses that were confidently wrong along the way, recorded so they are not re-run: logging
from GATT callbacks, PSRAM, allocation volume, and the boxed model. Each looked plausible and
each was contradicted by the next crash. The version pin was the fifth hypothesis and the
first that held.

If the pin ever has to move — a later spike wanting newer crux — the untried options are
bisecting 0.17/0.18 to find the change, pre-`Command` crux 0.10 (a real port: custom
capabilities instead of `#[effect]`), or dropping crux from the device shell, which would
forfeit the shared-core argument that put it here.

## Notes

- **NimBLE, not Bluedroid.** `esp32-nimble` needs `CONFIG_BT_NIMBLE_ENABLED=y` *and*
  `CONFIG_BT_BLUEDROID_ENABLED=n` in `sdkconfig.defaults`; Bluedroid is the ESP-IDF default,
  and leaving it on builds the wrong host stack.
- **Never log from a GATT callback.** They run on the NimBLE **host** task, whose default
  stack is 4096, and Debug-formatting the connection descriptor through the ESP logger
  overflows it. The result is an occasional `Double exception` with a corrupted backtrace —
  and because the corruption lands wherever the stack happens to overrun into, one dump
  blamed `memcpy` reading rodata and reported `EXCCAUSE 2` (an instruction fetch from a data
  address), which looks nothing like a stack problem.

  Two things made this hard to pin down. Raising *main's* stack does nothing, because main is
  a different task. And it appeared to crash while idle, which seemed to rule the callbacks
  out — but the host or the BLE explorer retries connections in the background, so the
  callbacks were firing without anyone touching anything.

  The fix is both halves: `CONFIG_BT_NIMBLE_HOST_TASK_STACK_SIZE=8192`, and callbacks that do
  nothing but an atomic store, with the main loop doing the reporting. Measured afterwards,
  the callback task uses ~1.7KB of its 8192 — so the logging really was the bulk of it.
- **Measure stacks, don't estimate them.** `uxTaskGetStackHighWaterMark(null)` reports the
  calling task's unused bytes; the shell logs it for main at startup and for the NimBLE host
  task from the callback. Main uses ~26KB, which is why spike 3's 32768 was marginal at only
  ~6.5KB spare.
- **Main task stack raised again**, to 49152. Spike 3 left only ~6.5KB spare at 32768 and this
  adds the BLE handles on top. The startup log reports what was actually used — worth reading
  rather than guessing, since an overflow shows up as a corrupt pointer somewhere unrelated.
