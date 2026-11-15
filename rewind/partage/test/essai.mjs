// Test de bout en bout du guichet : deux faux Rewind (Alice et Bob) s'ajoutent, s'envoient un clip,
// le relèvent, vérifient l'empreinte, puis on contrôle les refus (signature, amitié, taille…).
// Usage : node test/essai.mjs http://127.0.0.1:8787   (wrangler dev --local lancé à côté)
import { webcrypto as c } from "node:crypto";

const BASE = process.argv[2] ?? "http://127.0.0.1:8787";
const enc = new TextEncoder();
const b64 = (u8) => Buffer.from(u8).toString("base64");
const hex = async (data) => Buffer.from(await c.subtle.digest("SHA-256", data)).toString("hex");
let failures = 0;
const check = (ok, label) => {
  console.log(`${ok ? "OK   " : "RATÉ "} ${label}`);
  if (!ok) failures++;
};

async function newUser(name) {
  const keys = await c.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const pub = b64(new Uint8Array(await c.subtle.exportKey("raw", keys.publicKey)));
  const time = Date.now();
  const proof = b64(new Uint8Array(await c.subtle.sign("Ed25519", keys.privateKey, enc.encode(`register\n${pub}\n${time}`))));
  const r = await fetch(`${BASE}/v1/register`, { method: "POST", body: JSON.stringify({ pubkey: pub, name, time, proof }) });
  const me = await r.json();
  return { ...me, keys, pub };
}

async function call(u, method, path, body, opts = {}) {
  const data = body instanceof Uint8Array ? body : body === undefined ? new Uint8Array() : enc.encode(JSON.stringify(body));
  const time = opts.time ?? Date.now();
  const hash = await hex(data);
  const msg = `${method}\n${path}\n${time}\n${hash}`;
  const sig = b64(new Uint8Array(await c.subtle.sign("Ed25519", (opts.signer ?? u).keys.privateKey, enc.encode(msg))));
  const headers = { "x-rewind-id": u.id, "x-rewind-time": String(time), "x-rewind-body": hash, "x-rewind-sig": sig, ...(opts.headers ?? {}) };
  const r = await fetch(`${BASE}${path}`, { method, headers, body: method === "GET" ? undefined : data });
  const type = r.headers.get("content-type") ?? "";
  return { status: r.status, json: type.includes("json") ? await r.json() : null, bytes: type.includes("json") ? null : new Uint8Array(await r.arrayBuffer()), headers: r.headers };
}

const alice = await newUser("Alice");
const bob = await newUser("Bob");
const eve = await newUser("Eve");
check(/^RW-[2-9A-Z]{4}-[2-9A-Z]{4}$/.test(alice.code), `code ami ${alice.code}`);
// Réinscrire la même clé redonne le même code
{
  const time = Date.now();
  const proof = b64(new Uint8Array(await c.subtle.sign("Ed25519", alice.keys.privateKey, enc.encode(`register\n${alice.pub}\n${time}`))));
  const again = await (await fetch(`${BASE}/v1/register`, { method: "POST", body: JSON.stringify({ pubkey: alice.pub, name: "Alice", time, proof }) })).json();
  check(again.id === alice.id && again.code === alice.code, "réinscription : même identité");
}

// Refus : mauvaise signature, horloge, corps modifié
check((await call(alice, "GET", "/v1/sync", undefined, { signer: bob })).status === 401, "signature d'un autre refusée");
check((await call(alice, "GET", "/v1/sync", undefined, { time: Date.now() - 3_600_000 })).status === 401, "heure trop ancienne refusée");

// Amis : Alice demande, Bob accepte
const req1 = await call(alice, "POST", "/v1/friends/request", { code: bob.code.toLowerCase().replace("rw-", "") });
check(req1.json?.state === "pending", "demande d'ami (code tapé sans « RW- » et en minuscules)");
let sb = (await call(bob, "GET", "/v1/sync")).json;
check(sb.incoming.length === 1 && sb.incoming[0].name === "Alice", "Bob voit la demande d'Alice");
check((await call(alice, "POST", "/v1/friends/request", { code: alice.code })).status === 400, "son propre code refusé");
check((await call(bob, "POST", "/v1/friends/answer", { user: alice.id, accept: true })).json?.ok, "Bob accepte");
let sa = (await call(alice, "GET", "/v1/sync")).json;
check(sa.friends.length === 1 && sa.friends[0].id === bob.id, "Alice et Bob sont amis");

// Photo de profil et présence
const jpeg = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, ...new Array(2000).fill(7)]);
check((await call(bob, "PUT", "/v1/me/avatar", new Uint8Array([1, 2, 3]))).status === 400, "photo qui n'est pas un JPEG refusée");
const av = await call(bob, "PUT", "/v1/me/avatar", jpeg);
check(av.json?.avatar > 0, "photo de Bob envoyée");
sa = (await call(alice, "GET", "/v1/sync")).json;
check(sa.friends[0].avatar === av.json.avatar, "Alice voit la version de la photo de Bob");
const pic = await call(alice, "GET", `/v1/users/${bob.id}/avatar`);
check(pic.status === 200 && pic.bytes.length === jpeg.length, "Alice (amie) peut voir la photo de Bob");
check((await call(eve, "GET", `/v1/users/${bob.id}/avatar`)).status === 404, "Eve (inconnue) ne peut pas voir la photo de Bob");
check(sa.friends[0].online === true, "Bob est « en ligne » (vient de relever)");
await call(bob, "POST", "/v1/me", { show_online: false });
sa = (await call(alice, "GET", "/v1/sync")).json;
check(sa.friends[0].online === false, "Bob masque sa présence : plus « en ligne »");
await call(bob, "POST", "/v1/me", { show_online: true });
check((await call(eve, "GET", "/v1/sync")).json.friends.length === 0, "Eve n'a aucun ami : elle ne voit la présence de personne");

// Envoi d'un clip en 2 morceaux (50 Mo + reste)
const size = 50 * 1024 * 1024 + 123_457;
const clip = c.getRandomValues(new Uint8Array(65536));
const file = new Uint8Array(size);
for (let i = 0; i < size; i += clip.length) file.set(clip.subarray(0, Math.min(clip.length, size - i)), i);
const sha = await hex(file);
check((await call(alice, "POST", "/v1/shares", { title: "Test", game: "CS2", duration: 30, size, sha256: sha, to: [eve.id] })).status === 403, "envoi à un non-ami refusé");
check((await call(alice, "POST", "/v1/shares", { title: "Gros", size: 600 * 1024 * 1024, sha256: sha, to: [bob.id] })).status === 413, "clip trop lourd refusé");
const created = await call(alice, "POST", "/v1/shares", { title: "CS2 3K · 2 HS", game: "Counter-Strike 2", duration: 31.5, size, sha256: sha, to: [bob.id], markers: [{ time: 3.2, kind: "headshot" }] });
check(created.json?.parts === 2, `envoi créé (${created.json?.parts} morceaux)`);
const id = created.json.id;
const part = created.json.part_size;
// Le 2e morceau d'abord (ordre libre), puis un mauvais morceau, puis le 1er
check((await call(alice, "PUT", `/v1/shares/${id}/parts/2`, file.subarray(part))).json?.ok, "morceau 2 envoyé");
check((await call(alice, "PUT", `/v1/shares/${id}/parts/1`, file.subarray(0, 1000))).status === 400, "morceau de mauvaise taille refusé");
check((await call(alice, "POST", `/v1/shares/${id}/complete`)).status === 409, "terminer avec un morceau manquant refusé");
check((await call(alice, "PUT", `/v1/shares/${id}/parts/1`, file.subarray(0, part))).json?.ok, "morceau 1 envoyé");
check((await call(alice, "PUT", `/v1/shares/${id}/thumb`, enc.encode("JPEG factice"))).json?.ok, "miniature envoyée");
sb = (await call(bob, "GET", "/v1/sync")).json;
check(sb.inbox.length === 0, "rien dans la boîte de Bob avant la fin de l'envoi");
check((await call(alice, "POST", `/v1/shares/${id}/complete`)).json?.ok, "envoi terminé");

// Réception par Bob
sb = (await call(bob, "GET", "/v1/sync")).json;
const item = sb.inbox[0];
check(item?.id === id && item.sender_name === "Alice" && item.has_thumb && item.markers?.[0]?.kind === "headshot", "Bob voit le clip (titre, miniature, repères)");
check((await call(eve, "GET", `/v1/shares/${id}/file`)).status === 404, "Eve ne peut pas lire le clip");
const thumb = await call(bob, "GET", `/v1/shares/${id}/thumb`);
check(new TextDecoder().decode(thumb.bytes) === "JPEG factice", "miniature relue");
const first = await call(bob, "GET", `/v1/shares/${id}/file`, undefined, { headers: { range: "bytes=0-1048575" } });
check(first.status === 206 && first.bytes.length === 1048576, "début du fichier (reprise par plages)");
const rest = await call(bob, "GET", `/v1/shares/${id}/file`, undefined, { headers: { range: `bytes=1048576-` } });
const whole = new Uint8Array(size);
whole.set(first.bytes, 0);
whole.set(rest.bytes, first.bytes.length);
check(rest.status === 206 && (await hex(whole)) === sha, "fichier complet, empreinte identique");
check((await call(bob, "POST", `/v1/shares/${id}/done`, { state: "received" })).json?.ok, "Bob confirme la réception");
sb = (await call(bob, "GET", "/v1/sync")).json;
check(sb.inbox.length === 0, "boîte de Bob vide après réception");
check((await call(bob, "GET", `/v1/shares/${id}/file`)).status === 404, "fichier supprimé après réception");
sa = (await call(alice, "GET", "/v1/sync")).json;
check(sa.sent[0]?.to[0]?.state === "received", "Alice voit « reçu par Bob »");
check(sa.limits.used_today === size, "quota du jour compté");

// Blocage : Bob bloque Alice, elle ne peut plus lui envoyer
check((await call(bob, "POST", "/v1/friends/remove", { user: alice.id, block: true })).json?.ok, "Bob bloque Alice");
check((await call(alice, "POST", "/v1/shares", { title: "x", size: 10, sha256: sha, to: [bob.id] })).status === 403, "envoi après blocage refusé");
check((await call(alice, "POST", "/v1/friends/request", { code: bob.code })).json?.state === "pending", "le blocage n'est pas révélé à Alice");
sb = (await call(bob, "GET", "/v1/sync")).json;
check(sb.blocked.length === 1 && sb.incoming.length === 0, "Bob voit Alice dans ses bloqués, pas de demande");

// Clé du PC et clé de secours (0.4.0)
{
  const newKeys = async () => {
    const keys = await c.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
    return { keys, pub: b64(new Uint8Array(await c.subtle.exportKey("raw", keys.publicKey))) };
  };
  const sign = async (k, text) => b64(new Uint8Array(await c.subtle.sign("Ed25519", k.keys.privateKey, enc.encode(text))));
  // Passage d'Alice (ancienne clé aléatoire) à la clé de son PC : l'ancienne devient la clé de secours
  const pc1 = await newKeys();
  let time = Date.now();
  const bad = await call(alice, "POST", "/v1/me/key", { pubkey: pc1.pub, time, proof: await sign(alice, `rekey\n${alice.id}\n${time}`) });
  check(bad.status === 401, "clé du PC sans sa propre preuve : refusée");
  const rk = await call(alice, "POST", "/v1/me/key", { pubkey: pc1.pub, time, proof: await sign(pc1, `rekey\n${alice.id}\n${time}`) });
  check(rk.json?.recovery === alice.pub, "Alice passe à la clé du PC ; l'ancienne devient sa clé de secours");
  const alicePc1 = { ...alice, keys: pc1.keys };
  check((await call(alice, "GET", "/v1/sync")).status === 401, "l'ancienne clé ne signe plus les demandes");
  const s1 = (await call(alicePc1, "GET", "/v1/sync")).json;
  check(s1?.me?.code === alice.code && s1.me.recovery === alice.pub, "même code ami avec la clé du PC");
  // Réinscription depuis le même PC (données de Rewind effacées) : même compte
  time = Date.now();
  const again = await (await fetch(`${BASE}/v1/register`, { method: "POST", body: JSON.stringify({ pubkey: pc1.pub, name: "Alice", time, proof: await sign(pc1, `register\n${pc1.pub}\n${time}`), recovery: (await newKeys()).pub }) })).json();
  check(again.id === alice.id && again.recovery === alice.pub, "même PC : même compte, la clé de secours n'est pas remplacée");
  // Nouveau PC qui a déjà son propre compte : la clé de secours y rattache le compte d'Alice
  const pc2 = await newKeys();
  time = Date.now();
  const other = await (await fetch(`${BASE}/v1/register`, { method: "POST", body: JSON.stringify({ pubkey: pc2.pub, name: "PC2", time, proof: await sign(pc2, `register\n${pc2.pub}\n${time}`) }) })).json();
  time = Date.now();
  const recBody = (p) => ({ recovery: alice.pub, pubkey: pc2.pub, time, proof_recovery: p.r, proof_device: p.d });
  const r0 = await fetch(`${BASE}/v1/recover`, { method: "POST", body: JSON.stringify(recBody({ r: await sign(alice, `recover\n${pc2.pub}\n${time}`), d: await sign(alice, `device\n${alice.pub}\n${time}`) })) });
  check(r0.status === 401, "restauration sans la preuve du nouveau PC : refusée");
  const r1 = await (await fetch(`${BASE}/v1/recover`, { method: "POST", body: JSON.stringify(recBody({ r: await sign(alice, `recover\n${pc2.pub}\n${time}`), d: await sign(pc2, `device\n${alice.pub}\n${time}`) })) })).json();
  check(r1.id === alice.id && r1.code === alice.code, "clé de secours : compte d'Alice rattaché au nouveau PC");
  const alicePc2 = { ...alice, keys: pc2.keys };
  check((await call(alicePc2, "GET", "/v1/sync")).json?.me?.code === alice.code, "le nouveau PC signe pour Alice");
  check((await call(alicePc1, "GET", "/v1/sync")).status === 401, "l'ancien PC ne signe plus pour Alice");
  check((await call({ ...other, keys: pc2.keys }, "GET", "/v1/sync")).status === 401, "l'ancien compte du nouveau PC est remplacé");
  // Nouvelle clé de secours : l'ancienne ne marche plus
  const rec2 = await newKeys();
  check((await call(alicePc2, "POST", "/v1/me/recovery", { recovery: rec2.pub })).json?.recovery === rec2.pub, "nouvelle clé de secours");
  time = Date.now();
  const r2 = await fetch(`${BASE}/v1/recover`, { method: "POST", body: JSON.stringify({ recovery: alice.pub, pubkey: pc1.pub, time, proof_recovery: await sign(alice, `recover\n${pc1.pub}\n${time}`), proof_device: await sign(pc1, `device\n${alice.pub}\n${time}`) }) });
  check(r2.status === 404, "l'ancienne clé de secours ne marche plus");
  alice.keys = pc2.keys;
}

// Suppression d'identité
check((await call(eve, "DELETE", "/v1/me")).json?.ok, "Eve supprime son identité");
check((await call(eve, "GET", "/v1/sync")).status === 401, "Eve n'existe plus");
// Ménage : les faux utilisateurs du test ne restent pas dans la base
check((await call(alice, "DELETE", "/v1/me")).json?.ok && (await call(bob, "DELETE", "/v1/me")).json?.ok, "faux utilisateurs du test supprimés");

console.log(failures ? `\n${failures} échec(s)` : "\nTout est bon");
process.exit(failures ? 1 : 0);
