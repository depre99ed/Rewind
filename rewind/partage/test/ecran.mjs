// Test de l'ÉCRAN Amis dans la vraie appli d'essai (pilotée par CDP) : les signaux de Rewind mettent bien
// la page à jour toute seule (demande d'ami, clip reçu, pastille de la barre latérale, envoi en cours).
// Mêmes prérequis que appli.mjs. Usage : node test/ecran.mjs <dossier de test> [url du guichet]
import { webcrypto as c } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const DIR = process.argv[2];
const BASE = process.argv[3] ?? "http://127.0.0.1:8787";
const enc = new TextEncoder();
const b64 = (u8) => Buffer.from(u8).toString("base64");
const hex = async (data) => Buffer.from(await c.subtle.digest("SHA-256", data)).toString("hex");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failures = 0;
const check = (ok, label, extra = "") => {
  console.log(`${ok ? "OK   " : "RATÉ "} ${label}${!ok && extra ? ` — ${extra}` : ""}`);
  if (!ok) failures++;
};

const targets = await (await fetch("http://127.0.0.1:9333/json")).json();
const page = targets.find((t) => t.type === "page" && /:1420\/?(\?.*)?$/.test(t.url));
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let seq = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
};
async function evaluate(expression) {
  const id = ++seq;
  ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  const msg = await new Promise((r) => pending.set(id, r));
  return msg.result?.result?.value;
}
const invoke = async (cmd, args = {}) => {
  const out = JSON.parse(await evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(v => JSON.stringify({ ok: v ?? null }), e => JSON.stringify({ err: String(e) }))`));
  if (out.err) throw new Error(out.err);
  return out.ok;
};
const text = (selector) => evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].map(e => e.textContent.replace(/\\s+/g, " ").trim())`);
async function until(fn, label, ms = 30000) {
  const t = Date.now();
  while (Date.now() - t < ms) {
    const v = await fn().catch(() => null);
    if (v) return v;
    await sleep(400);
  }
  throw new Error(`délai dépassé : ${label}`);
}
async function newUser(name) {
  const keys = await c.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const pub = b64(new Uint8Array(await c.subtle.exportKey("raw", keys.publicKey)));
  const time = Date.now();
  const proof = b64(new Uint8Array(await c.subtle.sign("Ed25519", keys.privateKey, enc.encode(`register\n${pub}\n${time}`))));
  return { ...(await (await fetch(`${BASE}/v1/register`, { method: "POST", body: JSON.stringify({ pubkey: pub, name, time, proof }) })).json()), keys };
}
async function call(u, method, p, body) {
  const data = body instanceof Uint8Array ? body : body === undefined ? new Uint8Array() : enc.encode(JSON.stringify(body));
  const time = Date.now();
  const hash = await hex(data);
  const sig = b64(new Uint8Array(await c.subtle.sign("Ed25519", u.keys.privateKey, enc.encode(`${method}\n${p}\n${time}\n${hash}`))));
  const r = await fetch(`${BASE}${p}`, { method, headers: { "x-rewind-id": u.id, "x-rewind-time": String(time), "x-rewind-body": hash, "x-rewind-sig": sig }, body: method === "GET" ? undefined : data });
  return r.headers.get("content-type")?.includes("json") ? r.json() : null;
}

try {
  // Ouvrir la page Amis comme un utilisateur
  await evaluate(`[...document.querySelectorAll("nav button")].find(b => /Amis|Friends/.test(b.textContent)).click()`);
  await until(async () => (await text("h1")).some((t) => /Amis|Friends/.test(t)), "page Amis");
  check(true, "page Amis ouverte");
  check((await text(".intro h2")).length === 1, "écran d'activation affiché (partage pas encore activé)");
  // Activer depuis le formulaire de la page
  await evaluate(`(() => { const i = document.querySelector(".enable input"); i.value = "Testeur écran"; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);
  await sleep(300); // le bouton s'active une fois le pseudo pris en compte
  await evaluate(`document.querySelector(".enable button").click()`);
  const code = await until(async () => (await text(".code .mono"))[0], "code ami affiché");
  check(/^RW-/.test(code), `code ami affiché dans la page (${code})`);

  const B = await newUser("Ami écran");
  await call(B, "POST", "/v1/friends/request", { code });
  await invoke("friends_refresh");
  await until(async () => (await text(".person .pname")).includes("Ami écran"), "demande affichée");
  check(true, "la demande de B apparaît toute seule");
  check(Number((await text("nav em.new"))[0]) >= 1, "pastille sur l'onglet Amis");
  await evaluate(`[...document.querySelectorAll(".person")].find(p => p.textContent.includes("Ami écran")).querySelector(".btn.primary").click()`);
  await until(async () => (await text(".list .label")).some((t) => /AMIS\s*1|FRIENDS\s*1/i.test(t)), "liste d'amis à 1");
  check(true, "accepté depuis la page : 1 ami");

  // B envoie un clip : il doit apparaître tout seul dans « Reçus »
  const file = new Uint8Array(fs.readFileSync(path.join(DIR, "clip-de-B.mp4")));
  const sh = await call(B, "POST", "/v1/shares", { title: "Clip écran", game: "Valorant", duration: 5, size: file.length, sha256: await hex(file), to: [(await invoke("friends_view")).me.id] });
  await call(B, "PUT", `/v1/shares/${sh.id}/parts/1`, file);
  await call(B, "POST", `/v1/shares/${sh.id}/complete`);
  await invoke("friends_refresh");
  await until(async () => (await text(".grid .clip.done strong")).includes("Clip écran"), "clip reçu affiché", 40000);
  check(true, "le clip de B apparaît dans Reçus, prêt à lire");
  check((await text(".toast")).some((t) => /Ami écran/.test(t)), "message « Ami écran t'a envoyé un clip »");
  check(!(await text("nav em.new")).length, "pastille effacée une fois le clip reçu");

  // A envoie un clip : la ligne d'envoi apparaît puis « reçu »
  const lib = await invoke("refresh_library");
  const mine = lib.find((c) => c.title === "Clip de A");
  await invoke("share_send", { id: mine.id, to: [B.id] });
  const sent = await until(async () => (await text(".sent strong")).includes("Clip de A") && (await text(".sent")), "envoi affiché", 40000);
  check(true, "l'envoi apparaît dans Envoyés");
  // L'envoi s'affiche dès le départ (barre de progression) : B attend qu'il soit fini
  const inbox = await until(async () => {
    const x = (await call(B, "GET", "/v1/sync")).inbox;
    return x.length && x;
  }, "clip de A arrivé chez B", 60000);
  await call(B, "POST", `/v1/shares/${inbox[0].id}/done`, { state: "received" });
  await invoke("friends_refresh");
  await until(async () => (await text(".sent")).some((t) => t.includes("Clip de A") && /Ami écran reçu|Ami écran received/.test(t)), "« reçu » affiché");
  check(true, "« Ami écran reçu » affiché sous l'envoi");

  await invoke("friends_delete_identity");
  await call(B, "DELETE", "/v1/me");
} catch (e) {
  check(false, "erreur", e.message);
}
ws.close();
console.log(failures ? `\n${failures} échec(s)` : "\nTout est bon");
process.exit(failures ? 1 : 0);
