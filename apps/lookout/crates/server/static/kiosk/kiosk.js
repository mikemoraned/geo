import "/lookout-predictor.js";
import { readJourneys } from "/kiosk/journeys.js";

const JOURNEYS = "/map.pmtiles";
const WATCH_EACH_SESSION_MS = 60_000;

function introduce(count) {
  const each = WATCH_EACH_SESSION_MS % 60_000 === 0
    ? plural(WATCH_EACH_SESSION_MS / 60_000, "minute")
    : plural(Math.round(WATCH_EACH_SESSION_MS / 1_000), "second");
  const journeys = plural(count, "journey");
  document.querySelector("#intro").textContent =
    `${journeys[0].toUpperCase()}${journeys.slice(1)}, replayed. ` +
    `Each takes ${each} to replay, and ` +
    `${count === 1 ? "it runs" : "they run in turn"} for as long as this is left open.`;
}

function plural(count, thing) {
  if (count === 1) return `a ${thing}`;
  return `${count} ${thing}s`;
}

const predictor = document.querySelector("lookout-predictor");
const said = (message) => {
  document.querySelector("#said").textContent = message;
};

await predictor.ready;

try {
  const sessions = (await readJourneys(JOURNEYS)).filter((session) => session.samples.length > 0);
  if (sessions.length === 0) {
    said(`${JOURNEYS} holds no journeys to replay`);
  } else {
    introduce(sessions.length);
    replay(sessions);
  }
} catch (error) {
  said(`could not read ${JOURNEYS}: ${error.message}`);
}

function replay(sessions) {
  let session = 0;
  let sent = -1;
  let started;

  const begin = () => {
    sent = -1;
    predictor.dispatch("Reset");
    said(`${sessions[session].crossings} crossings on this journey`);
  };

  const frame = (now) => {
    started ??= now;
    const through = (now - started) / WATCH_EACH_SESSION_MS;

    if (through >= 1) {
      session = (session + 1) % sessions.length;
      started = now;
      begin();
    } else {
      const samples = sessions[session].samples;
      const at = Math.min(Math.floor(through * samples.length), samples.length - 1);
      if (at > sent) {
        sent = at;
        predictor.dispatch({ Position: samples[at] });
      }
    }

    requestAnimationFrame(frame);
  };

  begin();
  requestAnimationFrame(frame);
}
