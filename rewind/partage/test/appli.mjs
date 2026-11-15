// Test de bout en bout AVEC LA VRAIE APPLI : un Rewind d'essai (A, piloté par CDP) et un faux ami (B, ce script)
// s'ajoutent et s'échangent de vrais clips à travers le guichet.
// Prérequis : wrangler dev --local (port 8787), Rewind d'essai lancé avec REWIND_SHARE_URL=http://127.0.0.1:8787
// et WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333.
// Usage : node test/appli.mjs <dossier de test> [url du guichet]
import { webcrypto as c } from "node:crypto";
import fs from "node:fs";
import { execFileSync } from "node:child_process";
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
  check((await hex(dl.bytes)) === item.sha256 && dl.bytes.length <= fs.statSync(mine.path).size, "B reçoit le fichier intact (empreinte annoncée, jamais plus lourd que l'original)");
  check(item.has_thumb && (await call(B, "GET", `/v1/shares/${item.id}/thumb`)).bytes.length > 1000, "miniature envoyée par A");
  await call(B, "POST", `/v1/shares/${item.id}/done`, { state: "received" });
  await invoke("friends_refresh");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.sent.some((s) => s.id === item.id && s.to[0]?.state === "received") && x;
  }, "« reçu » visible chez A");
  check(true, "A voit « reçu par B »");

  // Allègement avant l'envoi : un clip lourd (1080p60, ~30 Mb/s) part 30 à 40 % plus léger
  const heavy = (await invoke("refresh_library")).find((c) => c.title === "Clip lourd de A");
  check(!!heavy, "clip lourd de A dans sa bibliothèque");
  await invoke("share_send", { id: heavy.id, to: [B.id] });
  let sawCompress = false;
  await until(async () => {
    const x = await invoke("friends_view");
    const t = x.transfers.find((t) => t.kind === "up" && t.title === "Clip lourd de A");
    if (t?.stage === "compress") sawCompress = true;
    if (t?.state === "error") throw new Error(t.error);
    return t?.state === "done";
  }, "envoi du clip lourd", 120000);
  check(sawCompress, "étape « allègement de la vidéo » visible");
  const heavyItem = (await call(B, "GET", "/v1/sync")).json.inbox.find((i) => i.title === "Clip lourd de A");
  const heavySize = fs.statSync(heavy.path).size;
  const ratio = heavyItem ? heavyItem.size / heavySize : 1;
  check(heavyItem && ratio < 0.75 && ratio > 0.4, `clip allégé : ${(heavySize / 1048576).toFixed(1)} Mo → ${((heavyItem?.size ?? 0) / 1048576).toFixed(1)} Mo (−${Math.round((1 - ratio) * 100)} %)`);
  const heavyDl = await call(B, "GET", `/v1/shares/${heavyItem.id}/file`);
  check((await hex(heavyDl.bytes)) === heavyItem.sha256, "fichier allégé intact (empreinte annoncée)");
  fs.writeFileSync(path.join(DIR, "recu-par-B.mp4"), heavyDl.bytes);
  await call(B, "POST", `/v1/shares/${heavyItem.id}/done`, { state: "received" });
  check(!fs.existsSync(path.join(DIR, "A-data", "amis", "envoi")) || fs.readdirSync(path.join(DIR, "A-data", "amis", "envoi")).length === 0, "copie allégée temporaire supprimée");

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

  // Compte lié au PC : aucune clé n'est écrite sur le disque, seule la clé de secours (chiffrée par Windows)
  const amisDir = path.join(DIR, "A-data", "amis");
  check(!fs.existsSync(path.join(amisDir, "cle.bin")) && fs.existsSync(path.join(amisDir, "secours.bin")), "pas de clé du compte sur le disque (dérivée du PC), clé de secours chiffrée");
  check(v.recovery_here === true, "la clé de secours du compte est sur ce PC");
  const key = await invoke("friends_recovery_key");
  check(/^RWK-[A-Z2-7]{4}(-[A-Z2-7]{1,4})+$/.test(key), `clé de secours (${key.length} caractères)`);

  // « Réinstallation » de Rewind : toutes ses données du partage effacées → le même compte revient tout seul
  fs.rmSync(amisDir, { recursive: true, force: true });
  await sleep(1200);
  v = await invoke("friends_enable", { name: "Testeur A" });
  check(v.me?.code === A.code && v.me?.id === A.id, "données effacées, partage réactivé : même code ami (même PC)");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.friends.some((f) => f.id === B.id) && x;
  }, "amis revenus");
  check(v.me?.avatar > 0, "amis et photo revenus sans clé de secours");
  check(v.recovery_here === false, "la clé de secours n'est plus sur ce PC (elle reste valable)");
  let missErr = "";
  try {
    await invoke("friends_recovery_key");
  } catch (e) {
    missErr = e.message;
  }
  check(missErr.includes("ami:recovery_missing"), "afficher la clé : « plus sur ce PC »", missErr);

  // Nouveau PC / Windows réinstallé : la clé de secours notée rattache le compte (ici, au même PC)
  let badErr = "";
  try {
    await invoke("friends_restore", { key: key.slice(0, -3) + (key.endsWith("A") ? "BBB" : "AAA") });
  } catch (e) {
    badErr = e.message;
  }
  check(badErr.includes("ami:recovery_invalid"), "clé avec une faute refusée", badErr);
  v = await invoke("friends_restore", { key: key.toLowerCase().replaceAll("-", " ") });
  check(v.me?.code === A.code && v.me?.id === A.id, "restaurée (même en minuscules) : même code ami");
  check(v.friends.some((f) => f.id === B.id), "ses amis sont là");
  check(v.me?.name === "Testeur A" && v.me?.avatar > 0 && v.recovery_here, "pseudo, photo et clé de secours retrouvés");

  // Nouvelle clé de secours : l'ancienne ne marche plus
  const key2 = await invoke("friends_new_recovery");
  check(key2 !== key && (await invoke("friends_recovery_key")) === key2, "nouvelle clé de secours créée et affichée");
  let oldErr = "";
  try {
    await invoke("friends_restore", { key });
  } catch (e) {
    oldErr = e.message;
  }
  check(oldErr.includes("ami:recovery_unknown"), "l'ancienne clé de secours est refusée", oldErr);

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

  // Rewind 0.3.8/0.3.9 → 0.4.0 : un compte créé avec l'ancienne clé aléatoire passe à la clé du PC,
  // et l'ancienne clé devient sa clé de secours (une clé « RWK- » déjà notée reste valable)
  const L = await newUser("Ancien A");
  const pkcs8 = new Uint8Array(await c.subtle.exportKey("pkcs8", L.keys.privateKey));
  const seed = pkcs8.slice(-32);
  const ps = `Add-Type -AssemblyName System.Security; $s=[Convert]::FromBase64String('${b64(seed)}'); $p=[Security.Cryptography.ProtectedData]::Protect($s,$null,'CurrentUser'); [IO.File]::WriteAllBytes('${path.join(amisDir, "cle.bin").replaceAll("'", "''")}',$p)`;
  fs.mkdirSync(amisDir, { recursive: true });
  execFileSync("powershell.exe", ["-NoProfile", "-Command", ps]);
  fs.writeFileSync(path.join(amisDir, "compte.json"), JSON.stringify({ id: L.id, code: L.code, name: "Ancien A" }));
  fs.rmSync(path.join(amisDir, "secours.bin"), { force: true });
  await call(B, "POST", "/v1/friends/request", { code: L.code });
  v = await invoke("friends_enable", { name: "Ancien A" });
  check(v.me?.code === L.code, "ancien compte repris tel quel (code ami)");
  await until(() => !fs.existsSync(path.join(amisDir, "cle.bin")) && fs.existsSync(path.join(amisDir, "secours.bin")), "passage à la clé du PC", 20000);
  check(true, "ancienne clé remplacée par la clé du PC");
  check((await call(L, "GET", "/v1/sync")).status === 401, "l'ancienne clé ne signe plus pour ce compte");
  v = await until(async () => {
    const x = await invoke("friends_view");
    return x.incoming.some((p) => p.id === B.id) && x;
  }, "compte utilisable avec la clé du PC");
  check(v.me?.code === L.code && v.recovery_here, "même code, demandes visibles, clé de secours sur ce PC");
  // Sa clé de secours est l'ancienne clé : même texte qu'avant la mise à jour
  const oldKeyText = await invoke("friends_recovery_key");
  const expected = await (async () => {
    const data = [...seed, ...new Uint8Array(await c.subtle.digest("SHA-256", seed)).slice(0, 3)];
    const A32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let bits = 0, buf = 0, out = "";
    for (const b of data) {
      buf = (buf << 8) | b;
      bits += 8;
      while (bits >= 5) {
        bits -= 5;
        out += A32[(buf >> bits) & 31];
      }
      buf &= (1 << bits) - 1;
    }
    if (bits) out += A32[(buf << (5 - bits)) & 31];
    return "RWK-" + out.match(/.{1,4}/g).join("-");
  })();
  check(oldKeyText === expected, "clé de secours = ancienne clé (une « RWK- » déjà notée reste valable)", `${oldKeyText} ≠ ${expected}`);
  await invoke("friends_delete_identity");
  await call(B, "DELETE", "/v1/me");
} catch (e) {
  check(false, "erreur", e.message);
}
ws.close();
console.log(failures ? `\n${failures} échec(s)` : "\nTout est bon");
process.exit(failures ? 1 : 0);
