import "/lookout-predictor.js";

const GEOLOCATION = { enableHighAccuracy: true, maximumAge: 0, timeout: 30_000 };
const TICK_INTERVAL_MS = 1000;

const predictor = document.querySelector("lookout-predictor");
const said = (message) => {
  document.querySelector("#said").textContent = message;
};

await predictor.ready;
predictor.dispatch("Reset");

setInterval(() => predictor.dispatch({ Tick: new Date().toISOString() }), TICK_INTERVAL_MS);

if (!navigator.geolocation) {
  said("no geolocation in this browser");
} else {
  navigator.geolocation.watchPosition(
    ({ coords, timestamp }) => {
      said("");
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
