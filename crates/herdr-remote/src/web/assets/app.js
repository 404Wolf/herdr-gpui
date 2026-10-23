// Herdr Remote: the app shell's script. It routes between server-rendered
// fragments, keeps a signed session with the computer, applies the
// server's patch stream, and sends commands. No inline script and no
// eval: the page's CSP forbids both, and Alpine runs as its CSP build.
"use strict";

const DEVICE = "herdr.device"; // { id, origins, fingerprint, computer }
const TOKEN = "herdr.session"; // { token, expires, origin }
const SIGN_PREFIX = "herdr-remote-session\0";

const app = {
  device: readJson(localStorage, DEVICE),
  session: readJson(sessionStorage, TOKEN),
  origin: null,
  stream: null,
  // The navigation in progress; a newer one aborts it.
  routing: null,
  retry: 0,
};

function readJson(storage, key) {
  try {
    return JSON.parse(storage.getItem(key) || "null");
  } catch {
    return null;
  }
}

function writeJson(storage, key, value) {
  try {
    if (value === null) storage.removeItem(key);
    else storage.setItem(key, JSON.stringify(value));
  } catch {
    // Private browsing: the session lives in memory only.
  }
}

// ---- bytes ----

function b64url(bytes) {
  let text = "";
  for (const byte of new Uint8Array(bytes)) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromB64url(text) {
  const padded = text.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(text.length / 4) * 4, "=");
  return Uint8Array.from(atob(padded), (char) => char.charCodeAt(0));
}

// ---- the device key, kept in IndexedDB and never extractable ----

function keyStore(mode, action) {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open("herdr-remote", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("keys");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const tx = open.result.transaction("keys", mode);
      const request = action(tx.objectStore("keys"));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    };
  });
}

const loadKey = () => keyStore("readonly", (store) => store.get("device"));
const saveKey = (pair) => keyStore("readwrite", (store) => store.put(pair, "device"));
const dropKey = () => keyStore("readwrite", (store) => store.delete("device"));

// ---- talking to the computer ----

class HttpError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function call(path, { method = "GET", body, raw, auth = true, origin, signal } = {}) {
  const headers = {};
  if (body !== undefined) headers["Content-Type"] = "application/json";
  if (raw !== undefined) headers["Content-Type"] = raw.type || "application/octet-stream";
  if (auth && app.session) headers.Authorization = `Bearer ${app.session.token}`;
  const response = await fetch((origin || app.origin || "") + path, {
    method,
    headers,
    body: raw !== undefined ? raw : body === undefined ? undefined : JSON.stringify(body),
    cache: "no-store",
    signal,
  });
  if (!response.ok) {
    let message = response.statusText;
    try {
      message = (await response.json()).error || message;
    } catch {
      // Not JSON.
    }
    throw new HttpError(response.status, message);
  }
  return response;
}

async function api(path, options = {}) {
  try {
    return await call(path, options);
  } catch (error) {
    if (error.status !== 401 || options.auth === false) throw error;
    app.session = null;
    await signIn();
    return call(path, options);
  }
}

/// The first of the computer's addresses that answers, this page's first.
async function reach() {
  const origins = [location.origin, ...(app.device?.origins || [])].filter(
    (origin, index, all) => all.indexOf(origin) === index,
  );
  const tries = origins.map(async (origin) => {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 2500);
    try {
      const response = await fetch(`${origin}/api/ping`, { signal: controller.signal, cache: "no-store" });
      if (!response.ok) throw new Error("not ok");
      return origin;
    } finally {
      clearTimeout(timer);
    }
  });
  return Promise.any(tries);
}

async function signIn() {
  if (app.session && app.session.expires > Date.now() + 60_000 && app.session.origin === app.origin) return;
  const pair = await loadKey();
  if (!pair || !app.device) throw new HttpError(403, "This phone is not paired.");
  const challenge = await (
    await call("/api/session/challenge", { method: "POST", body: { device: app.device.id }, auth: false })
  ).json();
  const message = new Uint8Array([...new TextEncoder().encode(SIGN_PREFIX), ...fromB64url(challenge.nonce)]);
  const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, pair.privateKey, message);
  const session = await (
    await call("/api/session", {
      method: "POST",
      body: { challenge: challenge.challenge, signature: b64url(signature) },
      auth: false,
    })
  ).json();
  app.session = { token: session.token, expires: session.expires_ms, origin: app.origin };
  writeJson(sessionStorage, TOKEN, app.session);
}

// ---- views ----

const view = () => document.getElementById("view");

function show(template) {
  stopStream();
  view().replaceChildren(document.getElementById(template).content.cloneNode(true));
}

function banner(text) {
  const element = document.getElementById("banner");
  element.textContent = text || "";
  element.hidden = !text;
}

function flash(text) {
  banner(text);
  setTimeout(() => banner(""), 4000);
}

/// The fragment and stream a path shows.
function routeOf(path, search) {
  const chat = path.match(/^\/chat\/([^/]+)$/);
  if (chat) return { fragment: `/f/chat/${chat[1]}`, stream: `view=chat&pane=${chat[1]}` };
  const routes = {
    "/": { fragment: "/f/home", stream: "view=home" },
    "/board": { fragment: "/f/board", stream: "view=board" },
    "/new": { fragment: "/f/new" },
    "/new-chat": { fragment: `/f/new-chat${search}` },
    "/settings": { fragment: "/f/settings" },
  };
  return routes[path] || routes["/"];
}

async function route() {
  const target = routeOf(location.pathname, location.search);
  stopStream();
  // Only the newest navigation may fill the view or open a stream: an
  // older page answering late must not replace the one the URL names.
  app.routing?.abort();
  const routing = new AbortController();
  app.routing = routing;
  try {
    const html = await (await api(target.fragment, { signal: routing.signal })).text();
    if (app.routing !== routing) return;
    view().innerHTML = html;
    window.scrollTo(0, location.pathname.startsWith("/chat/") ? document.body.scrollHeight : 0);
    if (target.stream) startStream(target.stream);
  } catch (error) {
    if (app.routing !== routing || routing.signal.aborted) return;
    if (error.status === 404) return navigate("/");
    if (error.status === 403) return forget();
    flash(error.message);
  }
}

function navigate(path) {
  if (path !== location.pathname + location.search) history.pushState(null, "", path);
  route();
}

// ---- the patch stream: server-sent events read with fetch, so the
// bearer token rides in a header rather than the URL ----

function stopStream() {
  app.stream?.abort();
  app.stream = null;
}

async function startStream(query) {
  const controller = new AbortController();
  app.stream = controller;
  try {
    const response = await call(`/api/stream?${query}`, { signal: controller.signal });
    if (app.stream !== controller) return response.body?.cancel();
    app.retry = 0;
    banner("");
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = "";
    controller.signal.addEventListener("abort", () => reader.cancel());
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += value;
      let end;
      while ((end = buffer.indexOf("\n\n")) >= 0) {
        handleEvent(buffer.slice(0, end));
        buffer = buffer.slice(end + 2);
      }
    }
  } catch (error) {
    if (controller.signal.aborted) return;
    if (error.status === 401) app.session = null;
  }
  if (controller.signal.aborted || app.stream !== controller) return;
  // The stream ended: reconnect, reloading the view to catch up.
  banner("Reconnecting…");
  app.retry = Math.min(app.retry + 1, 6);
  setTimeout(() => app.stream === controller && start(), 500 * 2 ** app.retry);
}

function handleEvent(block) {
  let event = "message";
  let data = "";
  for (const line of block.split("\n")) {
    if (line.startsWith("event:")) event = line.slice(6).trim();
    else if (line.startsWith("data:")) data += line.slice(5).trim();
  }
  if (event === "reload") return route();
  if (event !== "patch" || !data) return;
  const patch = JSON.parse(data);
  if (patch.badge !== undefined) navigator.setAppBadge?.(patch.badge).catch(() => {});
  for (const [id, html] of Object.entries(patch.regions || {})) {
    const element = document.getElementById(id);
    if (!element) continue;
    // Never replace a composer holding a draft, pictures, or a send.
    const draft = element.querySelector?.("textarea");
    if (draft && (draft.value || document.activeElement === draft)) continue;
    if (element.matches?.("[data-busy]") || element.querySelector?.("[data-busy]")) continue;
    const atBottom = window.innerHeight + window.scrollY >= document.body.scrollHeight - 80;
    element.outerHTML = html;
    if (atBottom && id === "chat-turn") window.scrollTo(0, document.body.scrollHeight);
  }
}

// ---- commands and forms ----

/// Sends a command; true when the chat took it.
async function command(pane, body) {
  try {
    const answer = await (await api(`/api/chat/${pane}/command`, { method: "POST", body })).json();
    if (!answer.rejected) return true;
    flash(answer.rejected);
  } catch (error) {
    flash(error.message);
  }
  return false;
}

document.addEventListener("click", (event) => {
  const link = event.target.closest("a[data-nav]");
  if (link && link.origin === location.origin) {
    event.preventDefault();
    return navigate(link.pathname + link.search);
  }
  const button = event.target.closest("[data-command]");
  if (button) {
    event.preventDefault();
    button.disabled = true;
    command(button.dataset.pane, JSON.parse(button.dataset.command)).finally(() => (button.disabled = false));
  }
  if (event.target.closest("[data-retry]")) start();
});

document.addEventListener("change", (event) => {
  const target = event.target;
  const pane = target.dataset.pane;
  if (target.dataset.option !== undefined) {
    command(pane, { command: "set_option", id: target.dataset.option, value: { type: "choice", id: target.value } });
  } else if (target.dataset.mode !== undefined) {
    command(pane, { command: "set_mode", id: target.value });
  } else if (target.dataset.autoAccept !== undefined) {
    if (target.checked && !confirm("Answer every question with its allow option from now on?")) {
      target.checked = false;
      return;
    }
    command(pane, { command: "set_auto_accept", on: target.checked });
  }
});

document.addEventListener("submit", async (event) => {
  const form = event.target;
  if (form.matches("[data-pair]")) {
    event.preventDefault();
    return pair(form);
  }
  if (!form.dataset.form) return;
  event.preventDefault();
  const body = Object.fromEntries(new FormData(form).entries());
  const error = form.querySelector(".form-error");
  try {
    const answer = await (await api(form.dataset.form, { method: "POST", body })).json();
    navigate(answer.location || "/");
  } catch (failure) {
    if (error) error.textContent = failure.message;
  }
});

window.addEventListener("popstate", route);

// ---- pairing ----

async function pair(form) {
  const params = new URLSearchParams(location.hash.slice(1));
  const error = form.querySelector("[data-error]");
  try {
    const pairKey = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
    const publicKey = await crypto.subtle.exportKey("raw", pairKey.publicKey);
    const claim = await (
      await call("/api/pair/claim", {
        method: "POST",
        auth: false,
        body: { token: params.get("t"), name: form.elements.name.value.trim(), public_key: b64url(publicKey) },
      })
    ).json();
    show("t-approve");
    view().querySelector("[data-code]").textContent = claim.code.replace(/(\d{3})(\d{3})/, "$1 $2");
    for (;;) {
      await new Promise((resolve) => setTimeout(resolve, 1500));
      const state = await (await call(`/api/pair/claim/${claim.claim}`, { auth: false })).json();
      if (state.state === "approved") {
        await saveKey(pairKey);
        app.device = { id: state.device, origins: state.origins, fingerprint: state.fingerprint, computer: state.computer };
        writeJson(localStorage, DEVICE, app.device);
        history.replaceState(null, "", "/");
        if (!matchMedia("(display-mode: standalone)").matches && /iPhone|iPad/.test(navigator.userAgent)) {
          return show("t-install");
        }
        return start();
      }
      if (state.state !== "claimed") {
        view().querySelector("[data-state]").textContent =
          state.state === "denied" ? "Your computer declined. Scan a new code to try again." : "The code expired. Scan a new one.";
        return;
      }
    }
  } catch (failure) {
    error.textContent = failure.message;
  }
}

async function forget() {
  try {
    await (await navigator.serviceWorker?.ready)?.pushManager.getSubscription().then((sub) => sub?.unsubscribe());
  } catch {
    // No push subscription.
  }
  await dropKey().catch(() => {});
  app.device = null;
  app.session = null;
  writeJson(localStorage, DEVICE, null);
  writeJson(sessionStorage, TOKEN, null);
  show("t-unpaired");
}

// ---- push ----

async function pushSubscription() {
  const registration = await navigator.serviceWorker?.ready;
  return registration?.pushManager.getSubscription();
}

async function subscribePush() {
  const registration = await navigator.serviceWorker.ready;
  const { key } = await (await api("/api/push/key")).json();
  const subscription = await registration.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: fromB64url(key),
  });
  await api("/api/push", { method: "POST", body: subscription.toJSON() });
}

async function unsubscribePush() {
  const subscription = await pushSubscription();
  await subscription?.unsubscribe();
  await api("/api/push", { method: "DELETE" });
}

// ---- Alpine components (CSP build: no inline expressions) ----

document.addEventListener("alpine:init", () => {
  const Alpine = window.Alpine;
  Alpine.data("fold", () => ({
    open: true,
    toggle() {
      this.open = !this.open;
    },
    get openClass() {
      return this.open ? "" : "-rotate-90";
    },
  }));
  Alpine.data("history", () => ({
    loaded: false,
    get folded() {
      return !this.loaded;
    },
    async load() {
      const pane = this.$el.closest("[data-pane]").dataset.pane;
      this.$refs.body.innerHTML = await (await api(`/f/chat/${pane}/history`)).text();
      this.loaded = true;
    },
  }));
  Alpine.data("composer", () => ({
    files: [],
    uploading: 0,
    sending: false,
    // Marks the composer while it holds pictures or work in flight, so a
    // live patch leaves it alone.
    mark() {
      const busy = this.files.length > 0 || this.uploading > 0 || this.sending;
      if (busy) this.$root.dataset.busy = "";
      else delete this.$root.dataset.busy;
    },
    pick() {
      this.$refs.file.click();
    },
    async attach(event) {
      const pane = this.$el.closest("[data-pane]").dataset.pane;
      const picked = [...event.target.files].slice(0, 4 - this.files.length);
      event.target.value = "";
      this.uploading += picked.length;
      this.mark();
      for (const file of picked) {
        try {
          const { id } = await (await api(`/api/chat/${pane}/upload`, { method: "POST", raw: file })).json();
          this.files.push({ id, name: file.name || "Picture" });
        } catch (error) {
          flash(error.message);
        }
        this.uploading -= 1;
        this.chips();
      }
    },
    chips() {
      const chips = this.$refs.chips;
      chips.replaceChildren(
        ...this.files.map((file, index) => {
          const chip = document.createElement("button");
          chip.type = "button";
          chip.className = "chip";
          chip.textContent = `${file.name} ✕`;
          chip.addEventListener("click", () => {
            this.files.splice(index, 1);
            this.chips();
          });
          return chip;
        }),
      );
      this.mark();
    },
    grow() {
      const text = this.$refs.text;
      text.style.height = "auto";
      text.style.height = `${text.scrollHeight}px`;
    },
    key(event) {
      // A hardware keyboard sends with Enter; on touch Enter is a newline.
      if (event.key === "Enter" && !event.shiftKey && matchMedia("(pointer: fine)").matches) {
        this.send(event);
      }
    },
    slash(event) {
      this.$refs.text.value = `/${event.currentTarget.dataset.slash} `;
      this.$refs.text.focus();
    },
    // The draft stays until the chat takes it, so a dropped connection
    // loses nothing; the user sends again.
    async send(event) {
      event.preventDefault();
      const text = this.$refs.text.value.trim();
      if (!text || this.sending || this.uploading > 0) return;
      const attachments = this.files.map((file) => ({ path: file.id }));
      this.sending = true;
      this.mark();
      const taken = await command(this.$root.closest("[data-pane]").dataset.pane, { command: "prompt", text, attachments });
      this.sending = false;
      if (taken) {
        this.$refs.text.value = "";
        this.files = [];
        this.chips();
        this.grow();
      }
      this.mark();
    },
  }));
  Alpine.data("settings", () => ({
    push: false,
    pushHint: "",
    async init() {
      if (!("PushManager" in window)) {
        this.pushHint = /iPhone|iPad/.test(navigator.userAgent)
          ? "Add Herdr to your Home Screen and open it from there to get notifications."
          : "This browser cannot receive notifications.";
        return;
      }
      this.push = Boolean(await pushSubscription());
      this.pushHint = this.push ? "Questions and finished turns arrive here." : "";
    },
    // Alpine's CSP build cannot assign through x-model, so the switch's
    // own state says what the user asked for.
    async togglePush(event) {
      const input = event.target;
      const wanted = input.checked;
      try {
        if (wanted) await subscribePush();
        else await unsubscribePush();
        this.push = wanted;
        this.pushHint = wanted ? "Questions and finished turns arrive here." : "";
      } catch (error) {
        this.push = !wanted;
        input.checked = !wanted;
        this.pushHint = error.message;
      }
    },
    forget() {
      if (confirm("Forget this computer? You will need to pair again.")) forget();
    },
  }));
});

// ---- start ----

async function start() {
  if (location.pathname === "/pair" && location.hash.includes("t=")) {
    show("t-pair");
    const fingerprint = new URLSearchParams(location.hash.slice(1)).get("fp") || "";
    view().querySelector("[data-fingerprint]").textContent = fingerprint.replace(/(.{2})(?=.)/g, "$1:");
    return;
  }
  if (!app.device) return show("t-unpaired");
  show("t-loading");
  try {
    app.origin = await reach();
    await signIn();
    route();
  } catch (error) {
    if (error.status === 403 || error.status === 404) return forget();
    show("t-offline");
  }
}

if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => {});
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && app.device && !app.stream) start();
});
navigator.serviceWorker?.addEventListener("message", (event) => {
  if (event.data?.navigate) navigate(event.data.navigate);
});
start();
