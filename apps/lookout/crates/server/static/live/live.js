// Feeds the predictor from this browser: where it says we are, and what time it is here.
import "/lookout-predictor.js";

const GEOLOCATION = { enableHighAccuracy: true, maximumAge: 0, timeout: 30_000 };
// Fixes arrive seconds apart, and a countdown should shorten in between.
const TICK_INTERVAL_MS = 1000;

const predictor = document.querySelector("lookout-predictor");
const said = (message) => {
  document.querySelector("#said").textContent = message;
};

await predictor.ready;
// Knowing nothing, which draws the request for crossings.
predictor.dispatch("Reset");

setInterval(() => predictor.dispatch({ Tick: new Date().toISOString() }), TICK_INTERVAL_MS);

if (!navigator.geolocation) {
  said("no geolocation in this browser");
} else {
  navigator.geolocation.watchPosition(
    ({ coords, timestamp }) => {
      said("");
      // The fix's own timestamp, not the time it arrived: it says where we were when it was
      // taken, and an arrival counted from it is counted from the right instant.
      predictor.dispatch({
        Position: {
          t: new Date(timestamp).toISOString(),
          gps: {
            lat: coords.latitude,
            lon: coords.longitude,
            alt: coords.altitude,
            acc: coords.accuracy,
            speed: coords.speed,
            heading: coords.heading,
          },
        },
      });
    },
    (failed) => said(`no position: ${failed.message}`),
    GEOLOCATION,
  );
}
