// The browser side of the probe and flasher (journal/D-031).
//
// Everything that decides what goes on the wire lives in the WebAssembly
// core (`oc-web`): encoding requests, splitting the port's bytes into
// messages, the pre-flash gate, the HalfKay reports. This file only drives
// the browser's asynchronous APIs (Web Serial, WebHID) and the page.

import init, * as oc from "./oc_web.js";

const $ = (id) => document.getElementById(id);

/** How long a request waits for its answer. */
const ANSWER_TIMEOUT_MS = 1500;
/** Lines of stream output kept on the page. */
const STREAM_LINES = 200;
/** How long to wait for the bootloader to appear after a restart. */
const LOADER_WAIT_MS = 10000;
/** Pixel scale of the screen canvas. */
const SCREEN_SCALE = 4;
/** How long `CENTER` reads an input at first, seconds of the module's clock. */
const CENTER_SECONDS = 5;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** A request answered by a list that a done closes. */
const UNTIL_DONE = Infinity;

/** Frees the WebAssembly messages a request kept. */
function release(items) {
  for (const item of items) item.message?.free();
}

// --- Probe session ----------------------------------------------------------

/** The open session, or null. */
let session = null;

class Session {
  constructor(port) {
    this.port = port;
    this.reader = new oc.ProbeReader();
    this.pending = [];
    /** The readings a `CENTER` collects from the stream, or null. */
    this.listening = null;
    this.closed = false;
    this.writer = port.writable.getWriter();
    this.portReader = port.readable.getReader();
    this.done = this.readLoop();
  }

  async readLoop() {
    try {
      for (;;) {
        const { value, done } = await this.portReader.read();
        if (done) break;
        for (const message of this.reader.push(value)) this.dispatch(message);
      }
    } catch (error) {
      if (!this.closed) {
        setProbeStatus(`connection lost (unplugged or restarted?): ${error.message ?? error}`);
      }
    } finally {
      this.failPending(new Error("the port closed"));
      if (!this.closed) closeSession();
    }
  }

  /**
   * Hands a message to the stream view, to a `CENTER` listening, or to the
   * oldest waiting request. A request that keeps its messages gets the
   * WebAssembly objects too, as `item.message`, and frees them itself
   * (`release`).
   */
  dispatch(message) {
    const kind = message.kind;
    if (kind === oc.MessageKind.Sample) {
      const listening = this.listening;
      if (listening) {
        // Every tick, a thousand a second: kept, not printed.
        const full = listening.readings.absorb(message);
        message.free();
        if (full) {
          this.stopListening();
          listening.resolve(listening.readings);
        }
        return;
      }
      const text = message.text;
      message.free();
      appendStream(text);
      return;
    }
    const item = { kind, text: message.text, page: message.page, bytes: message.bytes };
    const head = this.pending[0];
    if (!head) { // an answer nobody waits for
      message.free();
      return;
    }
    if (kind === oc.MessageKind.Error) {
      message.free();
      this.pending.shift();
      clearTimeout(head.timer);
      release(head.messages);
      head.reject(new Error(item.text));
      return;
    }
    if (head.keep) item.message = message; else message.free();
    head.messages.push(item);
    const complete = head.count === UNTIL_DONE
      ? kind === oc.MessageKind.Done
      : head.messages.length === head.count;
    if (complete) {
      this.pending.shift();
      clearTimeout(head.timer);
      head.resolve(head.messages);
    }
  }

  failPending(error) {
    for (const waiting of this.pending.splice(0)) {
      clearTimeout(waiting.timer);
      release(waiting.messages);
      waiting.reject(error);
    }
    const listening = this.listening;
    this.stopListening();
    listening?.reject(error);
  }

  /**
   * Hands the stream's samples to `readings` (an `oc.Readings`) instead of
   * the stream view, until they span the `seconds` it was made for: resolves
   * with it then, rejects if the samples stop coming.
   */
  listen(readings, seconds) {
    this.stopListening();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.stopListening();
        reject(new Error("the stream stopped before the time was up"));
      }, seconds * 1000 + 2 * ANSWER_TIMEOUT_MS);
      this.listening = { readings, resolve, reject, timer };
    });
  }

  /** Gives the stream back to its view, leaving a listen unsettled. */
  stopListening() {
    if (this.listening) clearTimeout(this.listening.timer);
    this.listening = null;
  }

  /**
   * Sends one frame and waits for `count` answers, or for every answer up
   * to the closing done with `UNTIL_DONE`; `keep` hands back the
   * WebAssembly messages as well.
   */
  ask(frame, count = 1, keep = false) {
    return new Promise((resolve, reject) => {
      const waiting = { count, keep, messages: [], resolve, reject };
      waiting.timer = setTimeout(() => {
        const at = this.pending.indexOf(waiting);
        if (at >= 0) this.pending.splice(at, 1);
        reject(new Error("no answer from the module"));
      }, ANSWER_TIMEOUT_MS);
      this.pending.push(waiting);
      this.writer.write(frame).catch((error) => {
        clearTimeout(waiting.timer);
        reject(error);
      });
    });
  }

  async close() {
    this.closed = true;
    try { await this.portReader.cancel(); } catch { /* already gone */ }
    await this.done;
    try { this.portReader.releaseLock(); } catch { /* already released */ }
    try { this.writer.releaseLock(); } catch { /* already released */ }
    try { await this.port.close(); } catch { /* already gone */ }
  }
}

async function connect() {
  let port;
  try {
    port = await navigator.serial.requestPort({
      filters: [{ usbVendorId: oc.probe_vendor_id(), usbProductId: oc.probe_product_id() }],
    });
  } catch {
    return; // the chooser was dismissed
  }
  try {
    // Any rate but 134, which restarts the module into its bootloader.
    await port.open({ baudRate: 115200 });
  } catch (error) {
    setProbeStatus(`cannot open it (another program may be using it): ${error.message}`);
    return;
  }
  session = new Session(port);
  try {
    const [hello] = await session.ask(oc.request_hello());
    setProbeStatus(`connected: ${hello.text}`);
    setProbeEnabled(true);
  } catch (error) {
    setProbeStatus(`wrong one (${error.message}): click Connect and pick the other`);
    await closeSession();
  }
}

async function closeSession() {
  const closing = session;
  session = null;
  setProbeEnabled(false);
  appView?.free();
  appView = null;
  stopScreenLive();
  if (closing) await closing.close();
}

async function run(action) {
  if (!session) return;
  try {
    await action(session);
  } catch (error) {
    setProbeStatus(`error: ${error.message}`);
  }
}

/**
 * Runs a request started from `button`, saying in the `status` element that
 * it is under way, then how it ended: an answer can take up to
 * `ANSWER_TIMEOUT_MS`, and one that changes nothing on the page (a clear, a
 * profile that reads the same) would otherwise look ignored.
 */
async function request(button, status, pending, action) {
  if (!session) return;
  const label = $(status);
  button.disabled = true;
  label.textContent = pending;
  try {
    label.textContent = `${await action(session)} at ${new Date().toLocaleTimeString()}`;
  } catch (error) {
    label.textContent = `error: ${error.message}`;
  } finally {
    button.disabled = !session;
  }
}

// --- Probe views ------------------------------------------------------------

async function captureScreen(probe) {
  const pages = await probe.ask(oc.request_screen(), 8);
  const screen = new Uint8Array(1024);
  for (const page of pages) screen.set(page.bytes, page.page * 128);
  const rgba = oc.screen_rgba(screen, SCREEN_SCALE);
  const canvas = $("screen");
  const image = new ImageData(new Uint8ClampedArray(rgba.buffer), canvas.width, canvas.height);
  canvas.getContext("2d").putImageData(image, 0, 0);
}

let screenLive = false;

async function screenLoop() {
  while (screenLive && session) {
    await run(captureScreen);
    await sleep(100);
  }
}

function stopScreenLive() {
  screenLive = false;
  $("screen-live").checked = false;
}

function appendStream(line) {
  const out = $("stream-out");
  const lines = out.textContent ? out.textContent.split("\n") : [];
  lines.push(line);
  out.textContent = lines.slice(-STREAM_LINES).join("\n");
  out.scrollTop = out.scrollHeight;
}

function setProbeStatus(text) {
  $("probe-status").textContent = text;
}

function setProbeEnabled(connected) {
  for (const element of document.querySelectorAll(".needs-probe")) element.disabled = !connected;
  $("connect").disabled = connected;
  $("disconnect").disabled = !connected;
}

// --- Applets (journal/D-043) ------------------------------------------------

/** The applet on show, or null. */
let appView = null;
/** Each parameter's row: its editor and its status cell. */
let paramRows = [];

/** Each applet's name, by its index. */
let appNames = new Map();
/** The instances running, or null before the first listing. */
let instances = null;
/** The position of the instance on show, or null for an app by its kind. */
let shownInstance = null;

/**
 * Lists the applets in the chooser and the instances running, and shows the
 * one on the module's screen (journal/D-046).
 */
async function listApps(probe) {
  const items = await probe.ask(oc.request_list_apps(), UNTIL_DONE, true);
  const list = new oc.AppList();
  for (const item of items) list.absorb(item.message);
  release(items);
  const chooser = $("apps-choice");
  chooser.replaceChildren();
  appNames = new Map();
  for (let at = 0; at < list.len; at += 1) {
    const running = list.running(at);
    appNames.set(list.index(at), list.name(at));
    chooser.add(new Option(`${list.name(at)}${running ? " (running)" : ""}`, list.index(at)));
  }
  const count = list.len;
  list.free();
  await listInstances(probe);
  let focused = 0;
  for (let at = 0; at < instances.len; at += 1) if (instances.focused(at)) focused = at;
  chooser.value = String(instances.app(focused));
  await showInstance(probe, focused);
  let running = 0;
  for (let at = 0; at < instances.len; at += 1) if (!instances.is_empty_slot(at)) running += 1;
  return `${count} apps, ${running} of ${instances.len} slots running`;
}

/** Lists the slots and draws a button for each. */
async function listInstances(probe) {
  const items = await probe.ask(oc.Instances.list_request(), UNTIL_DONE, true);
  const list = new oc.Instances();
  for (const item of items) list.absorb(item.message);
  release(items);
  instances?.free();
  instances = list;
  drawInstances();
}

/** The slot shown on the module, -1 before the slots are listed. */
function activeSlot() {
  if (!instances) return -1;
  for (let at = 0; at < instances.len; at += 1) if (instances.focused(at)) return at;
  return -1;
}

function drawInstances() {
  const row = $("apps-instances");
  row.replaceChildren();
  const active = activeSlot();
  $("apps-select").textContent = active < 0 ? "Run on the module" : `Run in slot @${active}`;
  for (let at = 0; at < instances.len; at += 1) {
    const show = document.createElement("button");
    const empty = instances.is_empty_slot(at);
    show.className = "needs-probe slot";
    show.classList.toggle("active", at === active);
    show.classList.toggle("empty", empty);
    show.setAttribute("aria-pressed", String(at === active));
    show.title = at === active ? "shown on the module" : "show it on the module";
    show.textContent = empty ? `@${at} empty` : `@${at} ${appNames.get(instances.app(at)) ?? "?"}`;
    show.addEventListener("click", (event) => request(
      event.currentTarget, "apps-status", "showing…", async (probe) => {
        await probe.ask(oc.Instances.focus_request(at));
        await listInstances(probe);
        await showInstance(probe, at);
        return `@${at} shown`;
      },
    ));
    row.append(show);
    if (!instances.is_empty_slot(at)) {
      const remove = document.createElement("button");
      remove.className = "needs-probe";
      remove.textContent = "✕";
      remove.title = `empty @${at}`;
      remove.addEventListener("click", (event) => request(
        event.currentTarget, "apps-status", "emptying…", async (probe) => {
          await probe.ask(oc.Instances.remove_request(at));
          return listApps(probe);
        },
      ));
      row.append(remove);
    }
  }
}

/** Describes the instance at `position` and draws it, its routing too. */
async function showInstance(probe, position) {
  shownInstance = position;
  await describe(probe, oc.AppView.of_instance(position, instances.app(position)));
}

/** Describes applet `app`, by its kind, and draws it. */
async function showApp(probe, app) {
  shownInstance = null;
  await describe(probe, new oc.AppView(app));
}

/** Fills `view` and draws its parameters, values, actions and routing. */
async function describe(probe, view) {
  const items = await probe.ask(view.describe_request(), UNTIL_DONE, true);
  for (const item of items) view.absorb(item.message);
  release(items);
  appView?.free();
  appView = view;
  drawParams();
  drawObservables();
  drawActions();
  drawRouting();
}

const OUTPUT_JACKS = ["A", "B", "C", "D"];

/** Where the shown instance's ports go, each a list to change it. */
function drawRouting() {
  const body = $("apps-routing");
  body.replaceChildren();
  if (shownInstance === null || !instances) return;
  const at = shownInstance;
  const line = (name, arrow, options, value, send) => {
    const tr = document.createElement("tr");
    tr.append(cell(name), cell(arrow));
    const select = document.createElement("select");
    select.className = "needs-probe";
    options.forEach(([label, option]) => select.add(new Option(label, option)));
    select.value = String(value);
    select.addEventListener("change", () => run(async (probe) => {
      try {
        await probe.ask(send(Number(select.value)));
      } catch (error) {
        $("apps-status").textContent = `error: ${error.message}`;
      }
      await listInstances(probe);
      drawRouting();
    }));
    const td = document.createElement("td");
    td.append(select);
    tr.append(td);
    body.append(tr);
  };
  const outputs = [...OUTPUT_JACKS.map((jack, index) => [jack, index]), ["OFF", -1]];
  for (let port = 0; port < appView.output_count; port += 1) {
    line(appView.output_name(port), "→", outputs, instances.output(at, port),
      (output) => oc.Instances.route_request(at, port, output));
  }
  for (let port = 0; port < appView.input_port_count; port += 1) {
    const kind = appView.input_port_trigger(port) ? "TR" : "CV";
    const jacks = [0, 1, 2, 3].map((jack) => [`${kind}${jack + 1}`, jack]);
    line(appView.input_port_name(port), "←", jacks, instances.input(at, port),
      (jack) => oc.Instances.input_request(at, port, jack));
  }
}

const cell = (text, className) => {
  const td = document.createElement("td");
  td.textContent = text;
  if (className) td.className = className;
  return td;
};

function drawParams() {
  const body = $("apps-params");
  body.replaceChildren();
  paramRows = [];
  for (let at = 0; at < appView.param_count; at += 1) {
    const row = document.createElement("tr");
    const editor = paramEditor(at);
    const status = cell("", "status");
    const label = cell(appView.param_label(at));
    const entry = { row, label, editor, status, sending: null, mark: null, panel: null };
    paramRows.push(entry);
    // A list sends what it picks; a field sends on Enter, even a value it
    // already shows, and when it loses the focus with a new one.
    editor.addEventListener("change", () => setParam(at, editor.value));
    if (editor.tagName === "INPUT") {
      editor.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          setParam(at, editor.value);
        }
      });
    }
    const editCell = document.createElement("td");
    editCell.append(editor);
    const markCell = cell("", "status");
    if (appView.param_assignable(at)) {
      entry.mark = document.createElement("button");
      entry.mark.className = "needs-probe input-mark";
      entry.mark.addEventListener("click", () => toggleInputPanel(at));
      markCell.append(entry.mark);
      showMark(entry, at);
    }
    row.append(label, editCell, cell(appView.param_range(at), "status"), markCell, status);
    markActive(row, at);
    body.append(row);
  }
}

/**
 * Opens or closes the CV input panel of parameter `at`, under its row: the
 * same panel for every assignable setting of every app (journal/D-045), the
 * fields of the module's submenu, each set as `cargo probe assign` would.
 */
function toggleInputPanel(at) {
  const entry = paramRows[at];
  if (entry.panel) {
    entry.panel.row.remove();
    entry.panel = null;
    showMark(entry, at);
    return;
  }
  const row = document.createElement("tr");
  row.className = "input-panel";
  const td = document.createElement("td");
  td.colSpan = 5;
  const line = document.createElement("div");
  line.className = "row";
  const status = document.createElement("span");
  status.className = "status";
  const editors = new Map();
  for (const field of oc.AppView.input_fields()) {
    const editor = inputEditor(field);
    editor.value = appView.input_field_text(at, field);
    editor.addEventListener("change", () => setInput(at, field, editor.value));
    if (editor.tagName === "INPUT") {
      editor.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          setInput(at, field, editor.value);
        }
      });
    }
    const label = document.createElement("label");
    label.append(`${field.toUpperCase()} `, editor);
    line.append(label);
    editors.set(field, editor);
  }
  const seconds = document.createElement("input");
  seconds.type = "number";
  seconds.min = "1";
  seconds.max = "60";
  seconds.value = String(CENTER_SECONDS);
  seconds.className = "needs-probe input-seconds";
  const center = document.createElement("button");
  center.className = "needs-probe";
  center.textContent = "CENTER";
  center.title = "Read the input for so many seconds of the module's clock, then set SCALE "
    + "and SHIFT to carry what it swung through onto the setting's range, as the submenu's "
    + "CENTER does; the stream view stops";
  center.addEventListener("click", () => centerInput(at));
  const centerLabel = document.createElement("label");
  centerLabel.append(center, " over ", seconds, " s");
  line.append(centerLabel);
  status.textContent = "each change goes to the module at once; closing this keeps it";
  line.append(status);
  td.append(line);
  row.append(td);
  entry.row.after(row);
  entry.panel = { row, editors, status, sending: null, center, seconds, centering: false };
  showMark(entry, at);
}

/**
 * The button that opens parameter `at`'s input panel: its input as the
 * module's screen marks it (`+CV2`, `CV` while none), and an arrow that
 * says whether the panel is open.
 */
function showMark(entry, at) {
  const open = entry.panel !== null;
  entry.mark.textContent = `${appView.input_mark(at)} ${open ? "\u25BE" : "\u25B8"}`;
  entry.mark.setAttribute("aria-expanded", String(open));
  entry.mark.title = open
    ? "Close the CV input panel: what is set stays on the module"
    : "Open the CV input panel: input, scale, shift, bounds, effect";
}

/** The field that edits one field of an input: a list or a text field. */
function inputEditor(field) {
  const choices = oc.AppView.input_choices(field);
  if (choices.length === 0) {
    const input = document.createElement("input");
    input.type = "text";
    input.className = "needs-probe input-field";
    return input;
  }
  const select = document.createElement("select");
  select.className = "needs-probe";
  for (const name of choices) select.add(new Option(name, name));
  return select;
}

/** Shows parameter `at`'s input as the module holds it, mark and fields. */
function showInput(at) {
  const entry = paramRows[at];
  const { mark, panel } = entry;
  if (mark) showMark(entry, at);
  if (!panel) return;
  for (const [field, editor] of panel.editors) editor.value = appView.input_field_text(at, field);
}

/** Sets one field of parameter `at`'s input and shows what the applet applied. */
async function setInput(at, field, text) {
  const { panel } = paramRows[at];
  if (!session || !appView || !panel) return;
  const key = `${field}=${text}`;
  if (panel.sending === key) return;
  let frame;
  try {
    frame = appView.assign_request(at, field, text);
  } catch (error) {
    panel.status.textContent = String(error);
    return;
  }
  panel.sending = key;
  try {
    const [item] = await session.ask(frame, 1, true);
    const shown = appView.absorb_input(at, item.message);
    release([item]);
    panel.status.textContent = `set: ${shown}`;
  } catch (error) {
    panel.status.textContent = `error: ${error.message}`;
  } finally {
    panel.sending = null;
    showInput(at);
  }
}

/**
 * `CENTER` for parameter `at`, as `cargo probe assign --center` does it:
 * streams every tick for the seconds the panel says, then sets the scale and
 * shift that carry what the input swung through onto the setting's range,
 * computed by the same function as the module's submenu (journal/D-045).
 */
async function centerInput(at) {
  const { panel } = paramRows[at];
  if (!session || !appView || !panel || panel.centering) return;
  const probe = session;
  const view = appView;
  const asked = Math.round(Number(panel.seconds.value));
  const seconds = Number.isFinite(asked) && asked >= 1 ? Math.min(asked, 60) : CENTER_SECONDS;
  panel.seconds.value = String(seconds);
  panel.centering = true;
  panel.center.disabled = true;
  panel.status.textContent = `reading the input for ${seconds} s of the module's clock…`;
  const readings = new oc.Readings(seconds);
  let streaming = false;
  try {
    const listened = probe.listen(readings, seconds);
    listened.catch(() => {}); // settled below; a failed start leaves it unwatched
    streaming = true;
    await probe.ask(oc.request_stream(1));
    await listened;
    streaming = false;
    await probe.ask(oc.request_stream(0));
    if (appView !== view) throw new Error("another app is on show now");
    const frame = view.center_request(at, readings);
    const [item] = await probe.ask(frame, 1, true);
    const shown = view.absorb_input(at, item.message);
    release([item]);
    panel.status.textContent = `centered on ${readings.len} readings: ${shown}`;
  } catch (error) {
    panel.status.textContent = `error: ${error.message ?? error}`;
  } finally {
    probe.stopListening();
    if (streaming && session === probe) probe.ask(oc.request_stream(0)).catch(() => {});
    readings.free();
    panel.centering = false;
    panel.center.disabled = !session;
    if (session === probe && appView === view) showInput(at);
  }
}

/**
 * Greys the face of a setting the module's screen does not show
 * (`RATCHET`'s `DIVISION` with `SYNC OFF`): it still takes a value, ready
 * for when the other setting brings it back.
 */
function markActive(row, at) {
  const active = appView.param_active(at);
  row.classList.toggle("inactive", !active);
  row.title = active ? "" : "not in use now: another setting shows instead";
}

/** The field that edits parameter `at`, holding its value now. */
function paramEditor(at) {
  const editor = appView.param_editor(at);
  if (editor === oc.Editor.Number) {
    const input = document.createElement("input");
    input.type = "text";
    input.className = "needs-probe";
    input.value = appView.param_edit_text(at);
    return input;
  }
  const select = document.createElement("select");
  select.className = "needs-probe";
  const names = editor === oc.Editor.Choice ? appView.param_choices(at) : ["ON", "OFF"];
  for (const name of names) select.add(new Option(name, name));
  select.value = appView.param_edit_text(at);
  return select;
}

/**
 * Sets parameter `at` as `cargo probe set` would, shows what the applet
 * applied, then reads every value back: a set may move other parameters
 * with it, as the detent would (`RATCHET`'s `GATE` off puts `COUNT` at 32).
 */
async function setParam(at, text) {
  if (!session || !appView) return;
  const row = paramRows[at];
  const { status } = row;
  // An Enter fires a keydown and a change: the second finds the same text
  // already under way.
  if (row.sending === text) return;
  let frame;
  try {
    frame = appView.set_request(at, text);
  } catch (error) {
    status.textContent = String(error);
    return;
  }
  for (const other of paramRows) other.status.textContent = "";
  row.sending = text;
  try {
    appView.settle();
    const [item] = await session.ask(frame, 1, true);
    const shown = appView.absorb_param(at, item.message);
    release([item]);
    status.textContent = appView.narrowed
      ? `set: ${shown} (narrowed by another setting)`
      : `set: ${shown}`;
    await refreshValues(appView.param_label(at));
  } catch (error) {
    status.textContent = `error: ${error.message}`;
    row.editor.value = appView.param_edit_text(at);
  } finally {
    row.sending = null;
  }
}

/** Reads every value back and shows them; `cause` names the set that moved them. */
async function refreshValues(cause) {
  const items = await session.ask(appView.refresh_request(), UNTIL_DONE, true);
  for (const item of items) appView.absorb_refresh(item.message);
  release(items);
  // A label follows the setting that changes it (`FIRST` reads `SPAN` in `MODE SPAN`).
  paramRows.forEach(({ row, label, editor, status }, at) => {
    label.textContent = appView.param_label(at);
    markActive(row, at);
    editor.value = appView.param_edit_text(at);
    if (appView.param_moved(at)) status.textContent = `moved with ${cause}: ${appView.param_text(at)}`;
    showInput(at);
  });
  drawObservables();
}

function drawObservables() {
  const body = $("apps-observed");
  body.replaceChildren();
  for (let at = 0; at < appView.observable_count; at += 1) {
    const row = document.createElement("tr");
    row.append(cell(appView.observable_name(at)), cell(appView.observable_text(at)));
    body.append(row);
  }
  $("apps-refresh").hidden = appView.observable_count === 0;
}

/** Reads every observable again. */
async function refreshObservables(probe) {
  if (!appView) return "nothing shown";
  for (let at = 0; at < appView.observable_count; at += 1) {
    const [item] = await probe.ask(appView.observe_request(at), 1, true);
    appView.absorb_observed(at, item.message);
    release([item]);
  }
  drawObservables();
  return "read";
}

function drawActions() {
  const row = $("apps-actions");
  row.replaceChildren();
  appView.actions.forEach((name, at) => {
    const button = document.createElement("button");
    button.className = "needs-probe";
    button.textContent = name;
    button.addEventListener("click", (event) => request(
      event.currentTarget, "apps-status", `${name}…`, async (probe) => {
        await probe.ask(appView.act_request(at));
        return `${name} done`;
      },
    ));
    row.append(button);
  });
}

// --- Flash ------------------------------------------------------------------

/** The image that passed the gate, or null. */
let firmware = null;

async function loadImage(file) {
  firmware?.free();
  firmware = null;
  $("confirm").checked = false;
  $("confirm").disabled = true;
  updateFlashButton();
  if (!file) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  try {
    firmware = new oc.Firmware(bytes);
  } catch (rejections) {
    $("image-out").textContent =
      `This file is not a firmware for this module; nothing will be written.\n${rejections}`;
    return;
  }
  const lines = [
    `file:        ${file.name}`,
    `kind:        ${firmware.fully_checked ? "firmware ELF, fully checked" : "Intel HEX"}`,
    `fingerprint: ${firmware.sha256}`,
    `size:        ${Math.ceil(firmware.flash_size / 1024)} KiB to write`,
    ...firmware.warnings.map((warning) => `warning: ${warning}`),
  ];
  $("image-out").textContent = lines.join("\n");
  $("confirm").disabled = false;
}

function updateFlashButton() {
  $("flash-go").disabled = !(firmware && $("confirm").checked);
}

const loaderFilter = () => ({ vendorId: oc.loader_vendor_id(), productId: oc.loader_product_id() });

/** A bootloader this page may already use, connected now. */
async function grantedLoader() {
  const { vendorId, productId } = loaderFilter();
  const devices = await navigator.hid.getDevices();
  return devices.find((device) => device.vendorId === vendorId && device.productId === productId);
}

async function findLoader() {
  const known = await grantedLoader();
  if (known) return known;
  // Asked while the click still counts as a user gesture: the browser's
  // list shows the bootloader as soon as it enumerates.
  try {
    const picked = await navigator.hid.requestDevice({ filters: [loaderFilter()] });
    if (picked.length) return picked[0];
  } catch {
    // The gesture expired or the list was dismissed: a bootloader allowed
    // earlier may still turn up.
  }
  const deadline = performance.now() + LOADER_WAIT_MS;
  while (performance.now() < deadline) {
    const device = await grantedLoader();
    if (device) return device;
    await sleep(200);
  }
  return null;
}

/** Whether the bootloader left the bus within `LOADER_WAIT_MS`, as it does to start the image. */
async function loaderGone() {
  const deadline = performance.now() + LOADER_WAIT_MS;
  while (performance.now() < deadline) {
    if (!(await grantedLoader())) return true;
    await sleep(200);
  }
  return false;
}

/** Sends one report, retrying while the bootloader is busy. */
async function sendReport(device, bytes, timeoutMs) {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    try {
      await device.sendReport(0, bytes);
      return;
    } catch (error) {
      if (performance.now() > deadline) throw error;
      await sleep(10);
    }
  }
}

async function flash() {
  if (!firmware) return;
  const image = firmware;
  const status = (text) => { $("flash-status").textContent = text; };
  $("flash-go").disabled = true;
  try {
    if (session) {
      status("switching the module to install mode…");
      try {
        await session.ask(oc.request_reboot_to_loader());
      } catch (error) {
        status(`the module did not switch (${error.message}): press PROGRAM on the Teensy`);
      }
      await closeSession();
    }
    status("waiting for the module in install mode (choose HalfKay if asked)…");
    const device = await findLoader();
    if (!device) {
      status("the module is not in install mode: press PROGRAM on the Teensy, then Install again");
      return;
    }
    if (!device.opened) await device.open();
    const total = image.report_count;
    const progress = $("flash-progress");
    progress.max = total;
    for (let index = 0; index < total - 1; index += 1) {
      status(index === 0
        ? "erasing the old firmware (a few seconds)…"
        : `writing: ${Math.floor((100 * index) / (total - 1))} %`);
      try {
        await sendReport(device, image.report(index), image.report_timeout_ms(index));
      } catch (error) {
        throw new Error(`block ${index + 1} of ${total - 1}: ${error.message}`);
      }
      progress.value = index + 1;
    }
    status("starting the new firmware…");
    try {
      // Refused while the last block is still being written, as any other.
      await sendReport(device, image.report(total - 1), image.report_timeout_ms(total - 1));
    } catch {
      // The bootloader may leave before it acknowledges: whether it left
      // is what tells.
    }
    if (!(await loaderGone())) {
      status("written, but the module did not start: switch it off and on, "
        + "or press PROGRAM and Install again");
      return;
    }
    progress.value = total;
    try { await device.close(); } catch { /* gone with the restart */ }
    status("done: the module restarts on the new firmware");
  } catch (error) {
    status(`install failed (${error.message}): press PROGRAM on the Teensy and try again`);
  } finally {
    updateFlashButton();
  }
}

// --- Start ------------------------------------------------------------------

function checkSupport() {
  const missing = [];
  if (!window.isSecureContext) missing.push("a secure context (https or localhost)");
  if (!("serial" in navigator)) missing.push("Web Serial");
  if (!("hid" in navigator)) missing.push("WebHID");
  if (missing.length) {
    const banner = $("support");
    banner.textContent = "This page needs Chrome or Edge on a desktop computer, opened from an "
      + `https:// address (missing here: ${missing.join(", ")}).`;
    banner.hidden = false;
  }
  return missing.length === 0;
}

async function main() {
  await init();
  const supported = checkSupport();
  $("connect").disabled = !supported;

  $("connect").addEventListener("click", connect);
  $("disconnect").addEventListener("click", closeSession);
  $("reboot").addEventListener("click", () => run(async (probe) => {
    await probe.ask(oc.request_reboot_to_loader());
    await closeSession();
    setProbeStatus("the module is in install mode: install a firmware below");
  }));
  $("screen-once").addEventListener("click", () => run(captureScreen));
  $("screen-live").addEventListener("change", (event) => {
    screenLive = event.target.checked;
    if (screenLive) screenLoop();
  });
  $("profile").addEventListener("click", (event) => request(
    event.currentTarget, "profile-status", "reading…", async (probe) => {
      const [profile] = await probe.ask(oc.request_profile());
      $("profile-out").textContent = profile.text;
      return "read";
    },
  ));
  $("profile-clear").addEventListener("click", (event) => request(
    event.currentTarget, "profile-status", "clearing…", async (probe) => {
      await probe.ask(oc.request_clear_profile());
      $("profile-out").textContent = "";
      return "cleared, counting from now";
    },
  ));
  $("stream-start").addEventListener("click", () => run(async (probe) => {
    const every = Math.max(1, Math.min(65535, Number($("stream-every").value) || 100));
    await probe.ask(oc.request_stream(every));
  }));
  $("stream-stop").addEventListener("click", () => run((probe) => probe.ask(oc.request_stream(0))));
  $("out-set").addEventListener("click", () => run((probe) => probe.ask(
    oc.request_force_output(Number($("out-channel").value), Math.round(Number($("out-mv").value))),
  )));
  $("out-release").addEventListener("click", () => run((probe) => probe.ask(
    oc.request_force_output(Number($("out-channel").value), undefined),
  )));

  $("apps-list").addEventListener("click", (event) => request(
    event.currentTarget, "apps-status", "listing…", listApps,
  ));
  $("apps-choice").addEventListener("change", (event) => run(
    (probe) => showApp(probe, Number(event.target.value)),
  ));
  $("apps-select").addEventListener("click", (event) => request(
    event.currentTarget, "apps-status", "switching…", async (probe) => {
      if (!appView) return "list the apps first";
      const app = Number($("apps-choice").value);
      const active = activeSlot();
      // In the slot shown on the module, which a click on a slot button
      // makes the one shown here too (journal/D-046).
      await probe.ask(active < 0 ? appView.select_request()
        : oc.Instances.place_request(app, active));
      return listApps(probe);
    },
  ));
  $("apps-refresh").addEventListener("click", (event) => request(
    event.currentTarget, "apps-status", "reading…", refreshObservables,
  ));

  $("elf").addEventListener("change", (event) => loadImage(event.target.files[0]));
  $("confirm").addEventListener("change", updateFlashButton);
  $("flash-go").addEventListener("click", flash);
  if (!supported) $("elf").disabled = true;
}

main();
