// Test de bout en bout AVEC LA VRAIE APPLI : un Rewind d'essai (A, piloté par CDP) et un faux ami (B, ce script)
// s'ajoutent et s'échangent de vrais clips à travers le guichet.
// Prérequis : wrangler dev --local (port 8787), Rewind d'essai lancé avec REWIND_SHARE_URL=http://127.0.0.1:8787
// et WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333.
// Usage : node test/appli.mjs <dossier de test> [url du guichet]
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

// --- Rewind A, par le protocole de débogage de WebView2
const targets = await (await fetch("http://127.0.0.1:9333/json")).json();
const page = targets.find((t) => t.type === "page" && /:1420\/?(\?.*)?$/.test(t.url));
if (!page) throw new Error("fenêtre principale de Rewind introuvable : " + targets.map((t) => t.url).join(", "));
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
async function invoke(cmd, args = {}) {
  const id = ++seq;
  const expression = `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then(v => JSON.stringify({ ok: v ?? null }), e => JSON.stringify({ err: String(e) }))`;
  ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  const msg = await new Promise((r) => pending.set(id, r));
  const out = JSON.parse(msg.result?.result?.value ?? '{"err":"évaluation impossible"}');
  if (out.err) throw new Error(out.err);
  return out.ok;
}
async function until(fn, label, ms = 30000) {
  const t = Date.now();
  while (Date.now() - t < ms) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {}
    await sleep(500);
  }
  throw new Error(`délai dépassé : ${label}`);
}

// --- Faux ami B
async function newUser(name) {
  const keys = await c.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const pub = b64(new Uint8Array(await c.subtle.exportKey("raw", keys.publicKey)));
  const time = Date.now();
  const proof = b64(new Uint8Array(await c.subtle.sign("Ed25519", keys.privateKey, enc.encode(`register\n${pub}\n${time}`))));
  const me = await (await fetch(`${BASE}/v1/register`, { method: "POST", body: JSON.stringify({ pubkey: pub, name, time, proof }) })).json();
  return { ...me, keys };
}
async function call(u, method, p, body, headers = {}) {
  const data = body instanceof Uint8Array ? body : body === undefined ? new Uint8Array() : enc.encode(JSON.stringify(body));
  const time = Date.now();
  const hash = await hex(data);
  const sig = b64(new Uint8Array(await c.subtle.sign("Ed25519", u.keys.privateKey, enc.encode(`${method}\n${p}\n${time}\n${hash}`))));
  const r = await fetch(`${BASE}${p}`, { method, headers: { "x-rewind-id": u.id, "x-rewind-time": String(time), "x-rewind-body": hash, "x-rewind-sig": sig, ...headers }, body: method === "GET" ? undefined : data });
  const type = r.headers.get("content-type") ?? "";
  return { status: r.status, json: type.includes("json") ? await r.json() : null, bytes: type.includes("json") ? null : new Uint8Array(await r.arrayBuffer()) };
}

try {
  let v = await invoke("friends_view");
  check(!v.enabled && !v.ready, "au départ : partage coupé, aucune identité");
  v = await invoke("friends_enable", { name: "Testeur A" });
  check(v.enabled && v.ready && /^RW-/.test(v.me?.code ?? ""), `partage activé, code ${v.me?.code}`);
  const A = v.me;

  const B = await newUser("Ami B");
  check((await call(B, "POST", "/v1/friends/request", { code: A.code })).json?.state === "pending", "B demande A en ami");
  await invoke("friends_refresh");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.incoming.some((p) => p.id === B.id) && x;
  }, "demande visible chez A");
  check(true, "A voit la demande de B");
  await invoke("friends_answer", { user: B.id, accept: true });
  v = await invoke("friends_view");
  check(v.friends.some((p) => p.id === B.id), "A accepte : amis");

  // B envoie un clip à A : réception automatique, vérifiée, rangée dans la bibliothèque
  const fileB = new Uint8Array(fs.readFileSync(path.join(DIR, "clip-de-B.mp4")));
  const shaB = await hex(fileB);
  const created = await call(B, "POST", "/v1/shares", { title: "Ace de B", game: "Counter-Strike 2", duration: 5, size: fileB.length, sha256: shaB, to: [A.id], markers: [{ time: 1.5, kind: "headshot", label: "Headshot" }] });
  await call(B, "PUT", `/v1/shares/${created.json.id}/parts/1`, fileB);
  await call(B, "POST", `/v1/shares/${created.json.id}/complete`);
  await invoke("friends_refresh");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.received.some((r) => r.share_id === created.json.id) && x;
  }, "réception du clip de B", 60000);
  const rec = v.received.find((r) => r.share_id === created.json.id);
  check(rec.from_name === "Ami B", "A a reçu le clip de B");
  const clips = await invoke("list_clips");
  const got = clips.find((c) => c.id === rec.clip_id);
  check(got?.kind === "received" && got.game === "Counter-Strike 2" && got.markers.length === 1 && got.thumb, "clip dans la bibliothèque (type reçu, jeu, repère, miniature)", JSON.stringify(got));
  check(got && got.path.includes(path.join("Reçus", "Ami B")), "rangé dans Reçus\\Ami B", got?.path);
  check(got && (await hex(new Uint8Array(fs.readFileSync(got.path)))) === shaB, "fichier identique à l'envoi (empreinte)");
  const sb = (await call(B, "GET", "/v1/sync")).json;
  check(sb.sent[0]?.to[0]?.state === "received", "B voit « reçu par A »");
  check((await call(B, "GET", `/v1/shares/${created.json.id}/file`)).status === 404, "fichier supprimé du guichet après réception");

  // A envoie un de ses clips à B
  const lib = await invoke("refresh_library");
  const mine = lib.find((c) => c.title === "Clip de A");
  check(!!mine, "clip de A dans sa bibliothèque");
  await invoke("share_send", { id: mine.id, to: [B.id] });
  v = await until(async () => {
    const x = await invoke("friends_view");
    const t = x.transfers.find((t) => t.kind === "up");
    if (t?.state === "error") throw new Error(t.error);
    return t?.state === "done" && x;
  }, "envoi de A", 60000);
  check(true, "envoi de A terminé");
  const sb2 = (await call(B, "GET", "/v1/sync")).json;
  const item = sb2.inbox.find((i) => i.title === "Clip de A");
  check(item && item.sender_name === "Testeur A" && item.game === "Counter-Strike 2", "B voit le clip de A (titre, jeu)");
  const dl = await call(B, "GET", `/v1/shares/${item.id}/file`);
  check((await hex(dl.bytes)) === (await hex(new Uint8Array(fs.readFileSync(mine.path)))), "B reçoit le fichier intact");
  check(item.has_thumb && (await call(B, "GET", `/v1/shares/${item.id}/thumb`)).bytes.length > 1000, "miniature envoyée par A");
  await call(B, "POST", `/v1/shares/${item.id}/done`, { state: "received" });
  await invoke("friends_refresh");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.sent.some((s) => s.id === item.id && s.to[0]?.state === "received") && x;
  }, "« reçu » visible chez A");
  check(true, "A voit « reçu par B »");

  // Photo de profil : A en choisit une (recadrée en carré par Rewind), B la voit
  await invoke("friends_set_avatar", { path: path.join(DIR, "photo.png") });
  v = await invoke("friends_view");
  check(v.me.avatar > 0, "A a mis une photo de profil");
  const sbPhoto = (await call(B, "GET", "/v1/sync")).json;
  const ofA = sbPhoto.friends.find((f) => f.id === A.id);
  check(ofA?.avatar === v.me.avatar, "B voit la version de la photo de A");
  const pic = await call(B, "GET", `/v1/users/${A.id}/avatar`);
  check(pic.status === 200 && pic.bytes[0] === 0xff && pic.bytes[1] === 0xd8 && pic.bytes.length < 150 * 1024, `photo reçue par B : JPEG de ${pic.bytes?.length} octets`);
  await call(B, "POST", "/v1/me", { name: "Ami B" }); // B relève : il est « en ligne »
  await invoke("friends_refresh");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.friends.find((f) => f.id === B.id)?.online && x;
  }, "B en ligne chez A");
  check(true, "A voit B « en ligne »");
  const local = await invoke("friends_avatar", { id: B.id, version: 0 }).catch((e) => String(e));
  check(String(local).includes("ami:"), "pas de photo pour B : refus propre");

  // Clé de secours : on « réinstalle » (identité locale effacée) puis on restaure
  const key = await invoke("friends_recovery_key");
  check(/^RWK-[A-Z2-7]{4}(-[A-Z2-7]{1,4})+$/.test(key), `clé de secours (${key.length} caractères)`);
  fs.rmSync(path.join(DIR, "A-data", "amis", "cle.bin"), { force: true });
  fs.rmSync(path.join(DIR, "A-data", "amis", "compte.json"), { force: true });
  let badErr = "";
  try {
    await invoke("friends_restore", { key: key.slice(0, -3) + (key.endsWith("A") ? "BBB" : "AAA") });
  } catch (e) {
    badErr = e.message;
  }
  check(badErr.includes("ami:recovery_invalid"), "clé avec une faute refusée", badErr);
  v = await invoke("friends_restore", { key: key.toLowerCase().replaceAll("-", " ") });
  check(v.me?.code === A.code && v.me?.id === A.id, "restaurée (même en minuscules) : même code ami");
  check(v.friends.some((f) => f.id === B.id), "ses amis sont revenus");
  check(v.me?.name === "Testeur A" && v.me?.avatar > 0, "pseudo et photo retrouvés");

  // Erreurs et suppression
  let codeErr = "";
  try {
    await invoke("friends_add", { code: "RW-ZZZZ-ZZZZ" });
  } catch (e) {
    codeErr = String(e.message);
  }
  check(codeErr.includes("ami:code_unknown"), "code inconnu : erreur claire", codeErr);
  await invoke("friends_delete_identity");
  v = await invoke("friends_view");
  check(!v.ready && !v.enabled && v.received.length === 1, "identité supprimée (historique local gardé)");
  check((await call(B, "GET", "/v1/sync")).json.friends.length === 0, "A a disparu chez B");
  await call(B, "DELETE", "/v1/me");
} catch (e) {
  check(false, "erreur", e.message);
}
ws.close();
console.log(failures ? `\n${failures} échec(s)` : "\nTout est bon");
process.exit(failures ? 1 : 0);
