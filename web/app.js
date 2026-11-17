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

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// --- Probe session ----------------------------------------------------------

/** The open session, or null. */
let session = null;

class Session {
  constructor(port) {
    this.port = port;
    this.reader = new oc.ProbeReader();
    this.pending = [];
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
      if (!this.closed) setProbeStatus(`port lost: ${error.message ?? error}`);
    } finally {
      this.failPending(new Error("the port closed"));
      if (!this.closed) closeSession();
    }
  }

  /** Hands a message to the stream view or to the oldest waiting request. */
  dispatch(message) {
    const kind = message.kind;
    const item = { kind, text: message.text, page: message.page, bytes: message.bytes };
    message.free();
    if (kind === oc.MessageKind.Sample) {
      appendStream(item.text);
      return;
    }
    const head = this.pending[0];
    if (!head) return; // an answer nobody waits for
    if (kind === oc.MessageKind.Error) {
      this.pending.shift();
      clearTimeout(head.timer);
      head.reject(new Error(item.text));
      return;
    }
    head.messages.push(item);
    if (head.messages.length === head.count) {
      this.pending.shift();
      clearTimeout(head.timer);
      head.resolve(head.messages);
    }
  }

  failPending(error) {
    for (const waiting of this.pending.splice(0)) {
      clearTimeout(waiting.timer);
      waiting.reject(error);
    }
  }

  /** Sends one frame and waits for `count` answers. */
  ask(frame, count = 1) {
    return new Promise((resolve, reject) => {
      const waiting = { count, messages: [], resolve, reject };
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
    setProbeStatus(`cannot open the port: ${error.message}`);
    return;
  }
  session = new Session(port);
  try {
    const [hello] = await session.ask(oc.request_hello());
    setProbeStatus(`connected: ${hello.text}`);
    setProbeEnabled(true);
  } catch (error) {
    setProbeStatus(`no hello on this port (${error.message}): try the other one`);
    await closeSession();
  }
}

async function closeSession() {
  const closing = session;
  session = null;
  setProbeEnabled(false);
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
    $("image-out").textContent = `refused, nothing will be written:\n${rejections}`;
    return;
  }
  const lines = [
    `image:         ${file.name}`,
    `elf sha256:    ${firmware.sha256}`,
    `loadable size: ${firmware.loadable_size} bytes`,
    `flash written: ${firmware.flash_size} bytes in ${firmware.report_count - 1} blocks`,
    `entry:         0x${firmware.entry.toString(16).padStart(8, "0")}`,
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
      status("restarting the module into its bootloader…");
      try {
        await session.ask(oc.request_reboot_to_loader());
      } catch (error) {
        status(`the module did not restart (${error.message}): press PROGRAM`);
      }
      await closeSession();
    }
    status("waiting for the bootloader…");
    const device = await findLoader();
    if (!device) {
      status("no bootloader: press PROGRAM on the module, then Flash again");
      return;
    }
    if (!device.opened) await device.open();
    const total = image.report_count;
    const progress = $("flash-progress");
    progress.max = total;
    for (let index = 0; index < total - 1; index += 1) {
      status(index === 0 ? "erasing…" : `writing block ${index + 1} of ${total - 1}`);
      await sendReport(device, image.report(index), image.report_timeout_ms(index));
      progress.value = index + 1;
    }
    status("starting the new image…");
    try {
      await device.sendReport(0, image.report(total - 1));
    } catch {
      // The bootloader may leave before it acknowledges.
    }
    progress.value = total;
    try { await device.close(); } catch { /* gone with the restart */ }
    status("done: the module restarts on the new image");
  } catch (error) {
    status(`flash failed: ${error.message}; press PROGRAM and try again`);
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
    banner.textContent = `This browser lacks ${missing.join(", ")}: use Chrome or Edge on a desktop.`;
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
  $("screen-once").addEventListener("click", () => run(captureScreen));
  $("screen-live").addEventListener("change", (event) => {
    screenLive = event.target.checked;
    if (screenLive) screenLoop();
  });
  $("profile").addEventListener("click", () => run(async (probe) => {
    const [profile] = await probe.ask(oc.request_profile());
    $("profile-out").textContent = profile.text;
  }));
  $("profile-clear").addEventListener("click", () => run(async (probe) => {
    await probe.ask(oc.request_clear_profile());
    $("profile-out").textContent = "";
  }));
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

  $("elf").addEventListener("change", (event) => loadImage(event.target.files[0]));
  $("confirm").addEventListener("change", updateFlashButton);
  $("flash-go").addEventListener("click", flash);
  if (!supported) $("elf").disabled = true;
}

main();
