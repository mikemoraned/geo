// Feeds the predictor from recorded journeys, one after another, for as long as the page is
// open. Sends no time: a replayed fix carries the instant it was recorded at, which is the
// clock everything in the view is measured against.
import "/lookout-predictor.js";

const SESSIONS = "/sessions.json";
// How long each session takes to watch, whatever it took to record. A recording runs for
// hours and nobody stands in front of a screen for that.
const SESSION_MS = 60_000;

// What is being replayed and for how long is decided by the file and by `SESSION_MS`, so the
// page says what it is doing rather than what it was written expecting.
function introduce(count) {
  const each = SESSION_MS % 60_000 === 0
    ? plural(SESSION_MS / 60_000, "minute")
    : plural(Math.round(SESSION_MS / 1_000), "second");
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

const response = await fetch(SESSIONS);
if (!response.ok) {
  said(`could not load ${SESSIONS}: ${response.status} ${response.statusText}`);
} else {
  const sessions = (await response.json()).filter((session) => session.samples.length > 0);
  if (sessions.length === 0) {
    said(`${SESSIONS} holds no sessions to replay`);
  } else {
    introduce(sessions.length);
    replay(sessions);
  }
}

// Which sample to send is worked out from the clock rather than counted off by a timer: each
// frame asks how far through the minute it is and sends the sample that far through the
// session. A browser that cannot keep up skips samples instead of falling behind, and a fix
// not sent costs nothing — the one that is sent carries its own instant, so the speed between
// them is still measured over the interval that really separated them.
//
// A journey is sent as it was recorded, timestamps and all. Each one begins by telling the
// core to start again knowing nothing, which is what lets a journey from the morning follow
// one from that evening.
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
    const through = (now - started) / SESSION_MS;

    if (through >= 1) {
      // Round again from the first, since a kiosk is left running and a screen that stops is
      // one that looks broken.
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
