// Filme l'animation d'ouverture image par image (aperçu navigateur en mode démo) et en fait une vidéo.
// Sert à vérifier le rendu réel sans lancer l'appli : chaque image est calculée à un instant précis,
// donc la vidéo est fluide même si la machine est lente.
//
// Usage (depuis rewind/, avec `npm run dev` lancé sur le port 1420) :
//   node tools/intro-frames.mjs sortie.mp4 [--dpr 1] [--fps 30] [--width 1320] [--height 840] [--keep] [--at 0.2,1.0,2.5]
//   --keep : garde les images (dossier affiché) ; --at : n'enregistre que ces instants (PNG, pas de vidéo).
//   --from 6 --to 8.8 : seulement ce passage (secondes ; --to peut dépasser la fin de l'intro, pour voir la suite).
//   --build : sans serveur, sert lui-même le dossier build/ à Chrome (sur ce PC, 5 à 10 % des connexions
//             locales échouent et une page de dev reste noire). Construire d'abord la version démo :
//             NODE_ENV=development npx vite build --mode development   (puis `npm run build` pour revenir).
// Les animations CSS (logo de la barre, barre latérale…) sont calées sur la même horloge que l'intro.
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
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
const fromBuild = args.includes("--build");
const url = opt("--url", fromBuild ? "http://rewind.local/?intro=1&introfreeze=1&lang=fr" : "http://localhost:1420/?intro=1&introfreeze=1");
const from = Number(opt("--from", "0"));
const toArg = opt("--to", "");
const BUILD = resolve(new URL("../build", import.meta.url).pathname.replace(/^\/([A-Z]:)/, "$1"));
const TYPES = { ".js": "text/javascript", ".css": "text/css", ".html": "text/html", ".json": "application/json", ".png": "image/png", ".jpg": "image/jpeg", ".webp": "image/webp", ".svg": "image/svg+xml", ".wav": "audio/wav", ".mp4": "video/mp4", ".woff2": "font/woff2", ".woff": "font/woff" };
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
    if (m.method === "Fetch.requestPaused") return serve(m.params);
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
  const api = { ready, send, close: () => ws.close() };
  // --build : chaque fichier demandé est lu dans build/ (index.html si absent : application d'une page)
  function serve(p) {
    let f = BUILD + decodeURIComponent(new URL(p.request.url).pathname);
    let ok = false;
    try { ok = statSync(f).isFile(); } catch {}
    if (!ok) f = join(BUILD, "index.html");
    const ext = f.slice(f.lastIndexOf(".")).toLowerCase();
    send("Fetch.fulfillRequest", { requestId: p.requestId, responseCode: 200, responseHeaders: [{ name: "Content-Type", value: TYPES[ext] ?? "application/octet-stream" }], body: readFileSync(f).toString("base64") }).catch(() => {});
  }
  return api;
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
  // Sans focus, l'appli met ses animations CSS en pause (data-bg) : on simule une fenêtre au premier plan
  await c.send("Page.bringToFront");
  await c.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  if (fromBuild) await c.send("Fetch.enable", { patterns: [{ urlPattern: "http://rewind.local/*" }] });
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
  const to = toArg ? Number(toArg) : end;
  const times = only.length ? only : Array.from({ length: Math.floor((to - from) * fps) + 1 }, (_, i) => from + i / fps);
  // Animations CSS déjà là (apparition de la page…) : terminées ; les suivantes sont calées sur l'horloge
  await c.send("Runtime.evaluate", { expression: "window.__cssOld = new WeakSet(document.getAnimations()); document.getAnimations().forEach((a) => { try { a.finish(); } catch {} }); window.__cssT = new Map();" });
  const syncCss = (t) => `for (const a of document.getAnimations()) { if (window.__cssOld.has(a)) continue; if (!window.__cssT.has(a)) window.__cssT.set(a, ${t}); a.pause(); a.currentTime = (${t} - window.__cssT.get(a)) * 1000; } new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))`;
  for (let i = 0; i < times.length; i++) {
    await c.send("Runtime.evaluate", { expression: `window.__introSeek(${times[i]})`, awaitPromise: true });
    await c.send("Runtime.evaluate", { expression: syncCss(times[i]), awaitPromise: true });
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
