const statusEl = document.getElementById("status");
const codeEl = document.getElementById("code");
const buttons = [document.getElementById("signin"), document.getElementById("register")];

class Refusal extends Error {
  constructor(status, reason) {
    super(reason);
    this.status = status;
  }
}

function show(text) {
  statusEl.textContent = text;
}

function showCode(code) {
  codeEl.textContent = code ?? "";
  codeEl.hidden = code === undefined;
}

async function post(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Refusal(response.status, await response.text());
  }
  return response;
}

async function signIn() {
  const { ceremony, options } = await (await post("/auth/signin/start")).json();
  const credential = await navigator.credentials.get({
    publicKey: PublicKeyCredential.parseRequestOptionsFromJSON(options.publicKey),
  });
  await post("/auth/signin/finish", { ceremony, credential: credential.toJSON() });
  location.assign("/record/");
}

async function register() {
  const { ceremony, options } = await (await post("/auth/register/start")).json();
  const credential = await navigator.credentials.create({
    publicKey: PublicKeyCredential.parseCreationOptionsFromJSON(options.publicKey),
  });
  const finished = await post("/auth/register/finish", { ceremony, credential: credential.toJSON() });
  const registered = await finished.json();
  if (registered.status === "allowed") {
    show("Registered and allowed. Sign in to continue.");
  } else {
    show("Registered. Give this code to the admin. Once they allow it, this passkey can sign in. The code expires in an hour.");
    showCode(registered.code);
  }
}

function explain(error) {
  if (error instanceof Refusal && error.status === 403) {
    return "This passkey is not allowed. Register it, and ask the admin to allow it.";
  }
  if (error.name === "NotAllowedError") {
    return "The passkey prompt was cancelled or timed out.";
  }
  return `Failed: ${error.message}`;
}

function whileWaiting(action) {
  return async () => {
    buttons.forEach((button) => (button.disabled = true));
    show("Waiting for the passkey…");
    showCode(undefined);
    try {
      await action();
    } catch (error) {
      show(explain(error));
    } finally {
      buttons.forEach((button) => (button.disabled = false));
    }
  };
}

if (typeof window.PublicKeyCredential?.parseCreationOptionsFromJSON === "function") {
  document.getElementById("signin").addEventListener("click", whileWaiting(signIn));
  document.getElementById("register").addEventListener("click", whileWaiting(register));
} else {
  buttons.forEach((button) => (button.disabled = true));
  show("This browser cannot read passkey options, so it cannot sign in here.");
}
