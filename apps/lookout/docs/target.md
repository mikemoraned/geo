# `Lookout` Motivation

I travel on trains a lot and it's not unusual that something goes by that I'd have liked to have taken a picture of, but I didn't see it coming in time. This can be all sorts of things, but is commonly things like rivers and bridges. The purpose of this app is to remind me when there is something interesting coming up.

# Straw-man

There are lots of things that count as interesting, but initially we will focus on rivers and bridges. So, initially we will find "points of interest" which are where a train line crosses a river or larger piece of water, and notify when the poi is 1 minute away, based on current rates of travel.

This plan may change based on what we discover, but for now this is the planned approach.

## Architecture

We follow the [ports and adapters pattern](https://8thlight.com/insights/a-color-coded-guide-to-ports-and-adapters), and in particular the different types of things we have to interface with are managed by this pattern:
- sensors e.g. GPS and accelerometers
- geo datasets
- ui's, on different platforms, including actions such as notifying the user
- persistence of derivations of state

## Approach

This largely breaks down into doing this live:
1. Using the assumption that they are currently on a train, identify which trainline they are currently on, based on how fast they are travelling and where they are
    - this will involve taking in absolute position sensors (i.e. GPS) and also relative ones like accelerometers
    - we can deal with uncertainty in position by clamping to the nearest trainline
    - we can use some more advanced modelling based on past position etc
2. Find next poi of interest that is on that line
3. Predict time of arrival at that poi and alert if this is less than a minute

This means that ahead of time we have to build a dataset that supports the lookup:
1. Get the train network for an area from [OvertureMaps](https://docs.overturemaps.org/guides/transportation/)
2. distill this down into a series of segments, or whatever supports the lookup

We can additionally improve accuracy by using accelerometers from devices that are coupled i.e. my laptop, my phone and my ipad. This is where a local device comms library comes in as it allows this accelerometer data to be shared across the devices. We may also want to consider using an [M5](https://m5stack.com) device purely as a dumb accelerometer i.e. it need not be running the whole stack, and just providing sensor data.

# Constraints, Trade-offs and Technology Choices

- Use the [crux](https://redbadger.github.io/crux/) library for ports and adapters
- Use the [iroh](https://docs.iroh.computer/quickstart) library for multi-device comms
- sensor data is persisted in sqlite
- sensor data is visualised in https://rerun.io 
- all code inside the centre of the architecture should be in Rust i.e. all business logic is in Rust
- for front-end on web we should follow a single-page-app pattern and use typescript + https://www.solidjs.com

# Learnings 

## Constraints

- **The dev laptop cannot self-source motion data.** The dev machine is an Apple Silicon
  (M3) MacBook Air (`Mac15,12`); Apple Silicon dropped the Sudden Motion Sensor, so there
  is no built-in accelerometer the OS or browser can read — `DeviceMotionEvent` never
  fires in desktop Safari/Chrome, and there is no SMC motion sensor. A "browser + server
  both on the laptop, localhost only" setup can validate the websocket transport and
  persistence plumbing, but **cannot** produce real accelerometer data. Any slice needing
  real motion must use an external source:
    - **AirPods** (Pro / 3rd-gen / Max) via macOS `CMHeadphoneMotionManager` — real
      accel+gyro, wireless, no cert; a small native (Swift) helper forwards it. (Head
      motion, not device motion.)
    - **A game controller with an IMU** (PS5 DualSense, DS4, Switch Joy-Con/Pro) — real
      3-axis accel+gyro, but the browser Gamepad API does *not* expose the IMU, so it
      needs a native HID reader → websocket.
    - **iPhone/iPad running the page** — the real target, gives proper `DeviceMotionEvent`
      data, but on a LAN IP (not `localhost`) the sensor API needs a secure context, so it
      requires HTTPS via a cert (`mkcert`) or a tunnel (`cloudflared`/`ngrok`).
    - An **M5 / dedicated sensor board** remains a future option.
