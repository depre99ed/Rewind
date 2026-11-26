// Filme l'animation d'ouverture image par image (aperçu navigateur en mode démo) et en fait une vidéo.
// Sert à vérifier le rendu réel sans lancer l'appli : chaque image est calculée à un instant précis,
// donc la vidéo est fluide même si la machine est lente.
//
// Usage (depuis rewind/, avec `npm run dev` lancé sur le port 1420) :
//   node tools/intro-frames.mjs sortie.mp4 [--dpr 1] [--fps 30] [--width 1320] [--height 840] [--keep] [--at 0.2,1.0,2.5]
//   --keep : garde les images (dossier affiché) ; --at : n'enregistre que ces instants (PNG, pas de vidéo).
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : def;
};
const out = resolve(args.find((a) => a.endsWith(".mp4")) ?? "intro.mp4");
const dpr = Number(opt("--dpr", "1"));
const fps = Number(opt("--fps", "30"));
const W = Number(opt("--width", "1320"));
const H = Number(opt("--height", "840"));
const only = opt("--at", "")
  .split(",")
  .filter(Boolean)
  .map(Number);
const url = opt("--url", "http://localhost:1420/?intro=1&introfreeze=1");
const port = 9333;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const chrome = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
].find(existsSync);
if (!chrome) throw new Error("Chrome ou Edge introuvable");
const profile = mkdtempSync(join(tmpdir(), "intro-chrome-"));
const browser = spawn(
  chrome,
  ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, `--window-size=${W},${H}`, "--hide-scrollbars", "--enable-unsafe-swiftshader", "about:blank"],
  { stdio: "ignore" },
);

function cdp(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let id = 0;
  const pending = new Map();
  ws.onmessage = (ev) => {
    const m = JSON.parse(typeof ev.data === "string" ? ev.data : ev.data.toString());
    if (m.id && pending.has(m.id)) {
      const { res, rej } = pending.get(m.id);
      pending.delete(m.id);
      if (m.error) rej(new Error(m.error.message));
      else res(m.result);
    }
  };
  const ready = new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const i = ++id;
      pending.set(i, { res, rej });
      ws.send(JSON.stringify({ id: i, method, params }));
    });
  return { ready, send, close: () => ws.close() };
}

try {
  let target;
  for (let i = 0; i < 100 && !target; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" });
      target = await r.json();
    } catch {
      await sleep(100);
    }
  }
  if (!target) throw new Error("navigateur sans réponse");
  const c = cdp(target.webSocketDebuggerUrl);
  await c.ready;
  await c.send("Page.enable");
  await c.send("Runtime.enable");
  await c.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: dpr, mobile: false });
  await c.send("Page.navigate", { url });
  // Le serveur de dev peut demander un rechargement au premier passage (dépendances optimisées) : on réessaie
  let ok = false;
  for (let attempt = 0; attempt < 4 && !ok; attempt++) {
    if (attempt) await c.send("Page.reload", { ignoreCache: true });
    for (let i = 0; i < 200 && !ok; i++) {
      const r = await c.send("Runtime.evaluate", { expression: "typeof window.__introSeek", returnByValue: true });
      ok = r.result.value === "function";
      if (!ok) await sleep(100);
    }
  }
  if (!ok) {
    const d = await c.send("Runtime.evaluate", {
      expression: "JSON.stringify({ gl2: !!document.createElement('canvas').getContext('webgl2'), mock: !!window.__REWIND_MOCK__, intro: !!document.querySelector('.intro'), texte: document.body.innerText.slice(0, 60) })",
      returnByValue: true,
    });
    throw new Error(`l'intro ne s'est pas chargée : ${d.result.value}`);
  }
  await sleep(400);

  const frames = mkdtempSync(join(tmpdir(), "intro-frames-"));
  const end = (await c.send("Runtime.evaluate", { expression: "window.__introEnd || 8", returnByValue: true })).result.value;
  const times = only.length ? only : Array.from({ length: Math.ceil(end * fps) + 1 }, (_, i) => i / fps);
  for (let i = 0; i < times.length; i++) {
    await c.send("Runtime.evaluate", { expression: `window.__introSeek(${times[i]})`, awaitPromise: true });
    const shot = await c.send("Page.captureScreenshot", { format: "png" });
    const name = only.length ? `t_${times[i].toFixed(2)}.png` : `f_${String(i).padStart(4, "0")}.png`;
    writeFileSync(join(frames, name), Buffer.from(shot.data, "base64"));
  }
  c.close();
  if (only.length) {
    console.log(`images : ${frames}`);
  } else {
    execFileSync("ffmpeg", ["-v", "error", "-y", "-framerate", String(fps), "-i", join(frames, "f_%04d.png"), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "15", "-preset", "slow", out]);
    console.log(`vidéo : ${out}`);
    if (args.includes("--keep")) console.log(`images : ${frames}`);
    else rmSync(frames, { recursive: true, force: true });
  }
} finally {
  browser.kill();
  await sleep(300);
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
