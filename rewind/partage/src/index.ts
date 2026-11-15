// Rewind : guichet du partage de clips entre amis (Cloudflare Worker).
//
// Aucun compte : chaque Rewind a une paire de clés Ed25519 (dérivée de l'identifiant du PC depuis la 0.4.0)
// et signe ses demandes. Une seconde clé, « de secours », permet de rattacher le compte à un nouveau PC.
// Le guichet vérifie la signature, l'amitié et les quotas, garde les clips dans R2 le temps
// qu'ils soient relevés (supprimés dès réception, au plus tard 7 jours) et ne stocke rien d'autre
// qu'un pseudo, un code ami, les liens d'amitié et la liste des envois.
//
// Signature d'une demande : en-têtes
//   x-rewind-id   identifiant de l'utilisateur
//   x-rewind-time heure en millisecondes
//   x-rewind-body empreinte SHA-256 (hex) du corps (corps vide : empreinte de rien)
//   x-rewind-sig  base64( Ed25519( MÉTHODE \n chemin \n heure \n empreinte ) )
// L'empreinte est recalculée ici pour les petits corps JSON ; pour les morceaux de vidéo elle n'est
// pas recalculée (trop coûteux), l'empreinte du fichier entier est vérifiée par le destinataire.

export interface Env {
  DB: D1Database;
  CLIPS: R2Bucket;
}

type User = { id: string; pubkey: string; name: string; code: string; sent_day: string; sent_bytes: number; last_seen: number; avatar: number; show_online: number; recovery: string | null };

const MB = 1024 * 1024;
const LIMITS = {
  maxClip: 500 * MB, // un clip
  dailyBytes: 2048 * MB, // envoyé par jour et par personne
  part: 50 * MB, // morceau d'envoi (sous la limite de 100 Mo d'un Worker)
  maxFriends: 200,
  maxPendingOut: 50,
  maxRecipients: 20,
  maxThumb: 512 * 1024,
  maxAvatar: 150 * 1024, // photo de profil carrée (JPEG 256 px)
  // « En ligne » = Rewind ouvert (relève toutes les 2 min) ; la présence n'est écrite que toutes les 5 min
  presenceWriteMs: 5 * 60_000,
  onlineMs: 8 * 60_000,
  maxMarkers: 32 * 1024,
  registerPerIpDay: 20,
  keepMs: 7 * 86_400_000,
  clockSkewMs: 10 * 60_000,
};
const CODE_ALPHABET = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ"; // ni 0/O ni 1/I
const EMPTY_SHA = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

class Fail extends Error {
  constructor(public status: number, public code: string) {
    super(code);
  }
}

const json = (data: unknown, status = 200) =>
  new Response(JSON.stringify(data), { status, headers: { "content-type": "application/json; charset=utf-8" } });

const now = () => Date.now();
const today = () => new Date().toISOString().slice(0, 10);
const pair = (x: string, y: string) => (x < y ? [x, y] : [y, x]);

function b64(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
}
function unb64(text: string): Uint8Array {
  const s = atob(text);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}
async function sha256hex(data: ArrayBuffer | Uint8Array): Promise<string> {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", data));
  return [...d].map((b) => b.toString(16).padStart(2, "0")).join("");
}
async function verify(pubkey: string, sig: string, message: string): Promise<boolean> {
  try {
    const key = await crypto.subtle.importKey("raw", unb64(pubkey), { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify({ name: "Ed25519" }, key, unb64(sig), new TextEncoder().encode(message));
  } catch {
    return false;
  }
}

function cleanName(raw: unknown): string {
  const name = String(raw ?? "")
    .replace(/[\u0000-\u001f\u007f]/g, "")
    .trim()
    .slice(0, 24);
  if (!name) throw new Fail(400, "name_invalid");
  return name;
}
function cleanText(raw: unknown, max: number): string {
  return String(raw ?? "")
    .replace(/[\u0000-\u001f\u007f]/g, " ")
    .trim()
    .slice(0, max);
}
function newCode(): string {
  const r = crypto.getRandomValues(new Uint8Array(8));
  const c = [...r].map((b) => CODE_ALPHABET[b % 32]).join("");
  return `RW-${c.slice(0, 4)}-${c.slice(4)}`;
}

// ---------------------------------------------------------------------------
// Authentification
// ---------------------------------------------------------------------------

async function authenticate(req: Request, env: Env, url: URL, body: ArrayBuffer | null): Promise<User> {
  const id = req.headers.get("x-rewind-id") ?? "";
  const time = Number(req.headers.get("x-rewind-time") ?? "0");
  const hash = (req.headers.get("x-rewind-body") ?? "").toLowerCase();
  const sig = req.headers.get("x-rewind-sig") ?? "";
  if (!id || !sig || !/^[0-9a-f]{64}$/.test(hash)) throw new Fail(401, "unsigned");
  if (!Number.isFinite(time) || Math.abs(now() - time) > LIMITS.clockSkewMs) throw new Fail(401, "clock");
  if (body) {
    const real = body.byteLength ? await sha256hex(body) : EMPTY_SHA;
    if (real !== hash) throw new Fail(401, "body_hash");
  }
  const user = await env.DB.prepare("SELECT id, pubkey, name, code, sent_day, sent_bytes, last_seen, avatar, show_online, recovery FROM users WHERE id = ?").bind(id).first<User>();
  if (!user) throw new Fail(401, "unknown_user");
  const message = `${req.method}\n${url.pathname}\n${time}\n${hash}`;
  if (!(await verify(user.pubkey, sig, message))) throw new Fail(401, "bad_signature");
  if (now() - user.last_seen > LIMITS.presenceWriteMs) {
    await env.DB.prepare("UPDATE users SET last_seen = ? WHERE id = ?").bind(now(), id).run();
  }
  return user;
}

async function readJson(body: ArrayBuffer | null): Promise<any> {
  if (!body || !body.byteLength) return {};
  try {
    return JSON.parse(new TextDecoder().decode(body));
  } catch {
    throw new Fail(400, "bad_json");
  }
}

// ---------------------------------------------------------------------------
// Inscription (une clé = un utilisateur ; réinscrire la même clé redonne le même code)
// ---------------------------------------------------------------------------

async function register(req: Request, env: Env, body: any): Promise<Response> {
  const pubkey = String(body.pubkey ?? "");
  const time = Number(body.time ?? 0);
  if (unb64Safe(pubkey)?.length !== 32) throw new Fail(400, "pubkey_invalid");
  if (Math.abs(now() - time) > LIMITS.clockSkewMs) throw new Fail(401, "clock");
  if (!(await verify(pubkey, String(body.proof ?? ""), `register\n${pubkey}\n${time}`))) throw new Fail(401, "bad_signature");
  const name = cleanName(body.name);
  // Clé de secours proposée (facultative) : gardée seulement si le compte n'en a pas encore
  const recovery = body.recovery === undefined || body.recovery === null ? null : String(body.recovery);
  if (recovery !== null && (unb64Safe(recovery)?.length !== 32 || recovery === pubkey)) throw new Fail(400, "recovery_invalid");
  const existing = await env.DB.prepare("SELECT id, code, name, avatar, show_online, recovery FROM users WHERE pubkey = ?").bind(pubkey).first<User>();
  if (existing) {
    // Même PC (ou même clé) : on redonne le même compte
    let rec = existing.recovery;
    if (!rec && recovery && !(await keyUsed(env, recovery))) {
      await env.DB.prepare("UPDATE users SET recovery = ? WHERE id = ?").bind(recovery, existing.id).run();
      rec = recovery;
    }
    return json(account(existing, rec));
  }
  if (recovery && (await keyUsed(env, recovery))) throw new Fail(409, "recovery_used");

  const ip = req.headers.get("cf-connecting-ip") ?? "?";
  const day = today();
  const row = await env.DB.prepare("SELECT count FROM ip_limits WHERE ip = ? AND day = ?").bind(ip, day).first<{ count: number }>();
  if ((row?.count ?? 0) >= LIMITS.registerPerIpDay) throw new Fail(429, "too_many_registrations");
  await env.DB.prepare("INSERT INTO ip_limits (ip, day, count) VALUES (?, ?, 1) ON CONFLICT (ip, day) DO UPDATE SET count = count + 1").bind(ip, day).run();

  const id = crypto.randomUUID();
  for (let attempt = 0; attempt < 5; attempt++) {
    const code = newCode();
    try {
      await env.DB.prepare("INSERT INTO users (id, pubkey, name, code, created, last_seen, recovery) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(id, pubkey, name, code, now(), now(), recovery)
        .run();
      return json({ id, code, name, avatar: 0, show_online: true, recovery: recovery ?? "" });
    } catch (e) {
      if (!String(e).includes("UNIQUE")) throw e;
    }
  }
  throw new Fail(500, "code_collision");
}

function account(u: Pick<User, "id" | "code" | "name" | "avatar" | "show_online">, recovery: string | null) {
  return { id: u.id, code: u.code, name: u.name, avatar: u.avatar ?? 0, show_online: !!(u.show_online ?? 1), recovery: recovery ?? "" };
}

/** Cette clé publique sert-elle déjà (clé de secours ou clé d'un PC) ? */
async function keyUsed(env: Env, key: string): Promise<boolean> {
  return !!(await env.DB.prepare("SELECT id FROM users WHERE recovery = ?1 OR pubkey = ?1").bind(key).first());
}

// ---------------------------------------------------------------------------
// Clé du PC et clé de secours
// ---------------------------------------------------------------------------

/**
 * Passage à la clé du PC (Rewind 0.4.0) : signé par l'ancienne clé, avec la preuve de la nouvelle.
 * L'ancienne clé devient la clé de secours si le compte n'en a pas (les clés « RWK- » déjà notées restent valables).
 */
async function rekey(env: Env, me: User, body: any): Promise<Response> {
  const pubkey = String(body.pubkey ?? "");
  const time = Number(body.time ?? 0);
  if (unb64Safe(pubkey)?.length !== 32) throw new Fail(400, "pubkey_invalid");
  if (Math.abs(now() - time) > LIMITS.clockSkewMs) throw new Fail(401, "clock");
  if (!(await verify(pubkey, String(body.proof ?? ""), `rekey\n${me.id}\n${time}`))) throw new Fail(401, "bad_signature");
  if (pubkey === me.pubkey) return json(account(me, me.recovery));
  if (await keyUsed(env, pubkey)) throw new Fail(409, "device_has_account");
  const recovery = me.recovery ?? me.pubkey;
  await env.DB.prepare("UPDATE users SET pubkey = ?, recovery = ? WHERE id = ?").bind(pubkey, recovery, me.id).run();
  return json(account(me, recovery));
}

/** Nouvelle clé de secours (l'ancienne ne marche plus). */
async function setRecovery(env: Env, me: User, body: any): Promise<Response> {
  const recovery = String(body.recovery ?? "");
  if (unb64Safe(recovery)?.length !== 32 || recovery === me.pubkey) throw new Fail(400, "recovery_invalid");
  if (recovery !== me.recovery && (await keyUsed(env, recovery))) throw new Fail(409, "recovery_used");
  await env.DB.prepare("UPDATE users SET recovery = ? WHERE id = ?").bind(recovery, me.id).run();
  return json(account(me, recovery));
}

/**
 * Restauration (Windows réinstallé, nouveau PC) : la clé de secours rattache le compte à la clé de ce PC.
 * Deux preuves : celle de la clé de secours et celle du PC. Si ce PC avait son propre compte, il est remplacé.
 */
async function recover(env: Env, body: any): Promise<Response> {
  const recovery = String(body.recovery ?? "");
  const pubkey = String(body.pubkey ?? "");
  const time = Number(body.time ?? 0);
  if (unb64Safe(recovery)?.length !== 32 || unb64Safe(pubkey)?.length !== 32 || recovery === pubkey) throw new Fail(400, "recovery_invalid");
  if (Math.abs(now() - time) > LIMITS.clockSkewMs) throw new Fail(401, "clock");
  if (!(await verify(recovery, String(body.proof_recovery ?? ""), `recover\n${pubkey}\n${time}`))) throw new Fail(401, "bad_signature");
  if (!(await verify(pubkey, String(body.proof_device ?? ""), `device\n${recovery}\n${time}`))) throw new Fail(401, "bad_signature");
  // Clé de secours, ou ancienne clé d'un compte pas encore passé à la clé du PC (clé « RWK- » d'avant la 0.4.0)
  const user = await env.DB.prepare("SELECT id, pubkey, name, code, avatar, show_online, recovery FROM users WHERE recovery = ?1 OR (pubkey = ?1 AND recovery IS NULL)")
    .bind(recovery)
    .first<User>();
  if (!user) throw new Fail(404, "recovery_unknown");
  if (user.pubkey !== pubkey) {
    const other = await env.DB.prepare("SELECT id FROM users WHERE pubkey = ? AND id != ?").bind(pubkey, user.id).first<{ id: string }>();
    if (other) await deleteUser(env, other.id);
  }
  await env.DB.prepare("UPDATE users SET pubkey = ?, recovery = ? WHERE id = ?").bind(pubkey, recovery, user.id).run();
  return json(account(user, recovery));
}

function unb64Safe(text: string): Uint8Array | null {
  try {
    return unb64(text);
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Amis
// ---------------------------------------------------------------------------

async function relation(env: Env, x: string, y: string) {
  const [a, b] = pair(x, y);
  return env.DB.prepare("SELECT status, requester FROM friends WHERE a = ? AND b = ?").bind(a, b).first<{ status: string; requester: string }>();
}

async function friendCount(env: Env, id: string): Promise<number> {
  const r = await env.DB.prepare("SELECT COUNT(*) AS n FROM friends WHERE (a = ? OR b = ?) AND status = 'accepted'").bind(id, id).first<{ n: number }>();
  return r?.n ?? 0;
}

async function requestFriend(env: Env, me: User, body: any): Promise<Response> {
  const code = String(body.code ?? "").toUpperCase().replace(/\s+/g, "");
  const normalized = code.startsWith("RW-") ? code : `RW-${code.slice(0, 4)}-${code.slice(4).replace(/^-/, "")}`;
  const other = await env.DB.prepare("SELECT id, name FROM users WHERE code = ?").bind(normalized).first<{ id: string; name: string }>();
  if (!other) throw new Fail(404, "code_unknown");
  if (other.id === me.id) throw new Fail(400, "own_code");
  const rel = await relation(env, me.id, other.id);
  const [a, b] = pair(me.id, other.id);
  if (rel?.status === "accepted") return json({ state: "friends", user: other });
  if (rel?.status === "blocked") {
    // On ne révèle pas un blocage : l'autre côté ne voit jamais rien
    if (rel.requester === me.id) throw new Fail(409, "you_blocked");
    return json({ state: "pending", user: other });
  }
  if (rel?.status === "pending") {
    if (rel.requester === me.id) return json({ state: "pending", user: other });
    // L'autre m'avait déjà demandé : on devient amis tout de suite
    await ensureRoom(env, me.id, other.id);
    await env.DB.prepare("UPDATE friends SET status = 'accepted' WHERE a = ? AND b = ?").bind(a, b).run();
    return json({ state: "friends", user: other });
  }
  const pendingOut = await env.DB.prepare("SELECT COUNT(*) AS n FROM friends WHERE (a = ? OR b = ?) AND status = 'pending' AND requester = ?")
    .bind(me.id, me.id, me.id)
    .first<{ n: number }>();
  if ((pendingOut?.n ?? 0) >= LIMITS.maxPendingOut) throw new Fail(429, "too_many_requests");
  if ((await friendCount(env, me.id)) >= LIMITS.maxFriends) throw new Fail(409, "friends_full");
  await env.DB.prepare("INSERT INTO friends (a, b, status, requester, created) VALUES (?, ?, 'pending', ?, ?)").bind(a, b, me.id, now()).run();
  return json({ state: "pending", user: other });
}

async function ensureRoom(env: Env, x: string, y: string) {
  if ((await friendCount(env, x)) >= LIMITS.maxFriends || (await friendCount(env, y)) >= LIMITS.maxFriends) throw new Fail(409, "friends_full");
}

async function answerFriend(env: Env, me: User, body: any): Promise<Response> {
  const other = String(body.user ?? "");
  const rel = await relation(env, me.id, other);
  if (!rel || rel.status !== "pending" || rel.requester === me.id) throw new Fail(404, "no_request");
  const [a, b] = pair(me.id, other);
  if (body.accept) {
    await ensureRoom(env, me.id, other);
    await env.DB.prepare("UPDATE friends SET status = 'accepted' WHERE a = ? AND b = ?").bind(a, b).run();
  } else {
    await env.DB.prepare("DELETE FROM friends WHERE a = ? AND b = ?").bind(a, b).run();
  }
  return json({ ok: true });
}

/** Retirer un ami, annuler une demande, ou bloquer (block: true) ; débloquer = retirer. */
async function removeFriend(env: Env, me: User, body: any): Promise<Response> {
  const other = String(body.user ?? "");
  const [a, b] = pair(me.id, other);
  const rel = await relation(env, me.id, other);
  if (rel?.status === "blocked" && rel.requester !== me.id) {
    // Bloqué par l'autre : on ne peut ni lever ni changer ce blocage
    return json({ ok: true });
  }
  if (body.block) {
    await env.DB.prepare(
      "INSERT INTO friends (a, b, status, requester, created) VALUES (?, ?, 'blocked', ?, ?) ON CONFLICT (a, b) DO UPDATE SET status = 'blocked', requester = excluded.requester",
    )
      .bind(a, b, me.id, now())
      .run();
    // Les clips en attente entre les deux ne sont plus livrés
    await env.DB.prepare(
      "UPDATE recipients SET state = 'dismissed', updated = ? WHERE state = 'waiting' AND ((user_id = ? AND share_id IN (SELECT id FROM shares WHERE sender = ?)) OR (user_id = ? AND share_id IN (SELECT id FROM shares WHERE sender = ?)))",
    )
      .bind(now(), me.id, other, other, me.id)
      .run();
  } else {
    await env.DB.prepare("DELETE FROM friends WHERE a = ? AND b = ?").bind(a, b).run();
  }
  return json({ ok: true });
}

// ---------------------------------------------------------------------------
// État complet (une seule demande toutes les 2 minutes pour tout savoir)
// ---------------------------------------------------------------------------

async function sync(env: Env, me: User): Promise<Response> {
  const [rels, inbox, sent] = await env.DB.batch([
    env.DB.prepare(
      "SELECT f.status, f.requester, f.created, u.id, u.name, u.avatar, u.last_seen, u.show_online FROM friends f JOIN users u ON u.id = CASE WHEN f.a = ?1 THEN f.b ELSE f.a END WHERE f.a = ?1 OR f.b = ?1",
    ).bind(me.id),
    env.DB.prepare(
      "SELECT s.id, s.sender, u.name AS sender_name, s.title, s.game, s.duration, s.size, s.sha256, s.markers, s.has_thumb, s.created FROM recipients r JOIN shares s ON s.id = r.share_id JOIN users u ON u.id = s.sender WHERE r.user_id = ? AND r.state = 'waiting' AND s.state = 'ready' ORDER BY s.created DESC LIMIT 200",
    ).bind(me.id),
    env.DB.prepare(
      "SELECT s.id, s.title, s.game, s.size, s.state, s.created, r.user_id, r.state AS rstate, u.name FROM shares s JOIN recipients r ON r.share_id = s.id JOIN users u ON u.id = r.user_id WHERE s.sender = ? ORDER BY s.created DESC LIMIT 400",
    ).bind(me.id),
  ]);
  const friends: unknown[] = [];
  const incoming: unknown[] = [];
  const outgoing: unknown[] = [];
  const blocked: unknown[] = [];
  for (const r of rels.results as any[]) {
    // En ligne : seulement entre amis, et seulement si la personne l'accepte
    const online = r.status === "accepted" && !!r.show_online && now() - r.last_seen < LIMITS.onlineMs;
    const who = { id: r.id, name: r.name, since: r.created, avatar: r.avatar ?? 0, online };
    if (r.status === "accepted") friends.push(who);
    else if (r.status === "pending") (r.requester === me.id ? outgoing : incoming).push(who);
    else if (r.status === "blocked" && r.requester === me.id) blocked.push(who);
  }
  const byShare = new Map<string, any>();
  for (const r of sent.results as any[]) {
    let s = byShare.get(r.id);
    if (!s) {
      s = { id: r.id, title: r.title, game: r.game, size: r.size, state: r.state, created: r.created, to: [] };
      byShare.set(r.id, s);
    }
    s.to.push({ id: r.user_id, name: r.name, state: r.rstate });
  }
  const used = me.sent_day === today() ? me.sent_bytes : 0;
  return json({
    me: account(me, me.recovery),
    friends,
    incoming,
    outgoing,
    blocked,
    inbox: (inbox.results as any[]).map((s) => ({ ...s, markers: safeJson(s.markers), has_thumb: !!s.has_thumb })),
    sent: [...byShare.values()],
    limits: { max_clip: LIMITS.maxClip, daily_bytes: LIMITS.dailyBytes, used_today: used, part: LIMITS.part, keep_days: 7 },
  });
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return [];
  }
}

// ---------------------------------------------------------------------------
// Envois
// ---------------------------------------------------------------------------

async function createShare(env: Env, me: User, body: any): Promise<Response> {
  const size = Number(body.size);
  const sha = String(body.sha256 ?? "").toLowerCase();
  const to: string[] = Array.isArray(body.to) ? [...new Set(body.to.map(String))] : [];
  if (!Number.isInteger(size) || size <= 0) throw new Fail(400, "size_invalid");
  if (size > LIMITS.maxClip) throw new Fail(413, "clip_too_big");
  if (!/^[0-9a-f]{64}$/.test(sha)) throw new Fail(400, "sha_invalid");
  if (!to.length || to.length > LIMITS.maxRecipients) throw new Fail(400, "recipients_invalid");
  const markers = JSON.stringify(body.markers ?? []);
  if (markers.length > LIMITS.maxMarkers) throw new Fail(400, "markers_too_big");
  for (const id of to) {
    const rel = await relation(env, me.id, id);
    if (rel?.status !== "accepted") throw new Fail(403, "not_friends");
  }
  const used = me.sent_day === today() ? me.sent_bytes : 0;
  if (used + size > LIMITS.dailyBytes) throw new Fail(429, "daily_quota");

  const id = crypto.randomUUID();
  const parts = Math.max(1, Math.ceil(size / LIMITS.part));
  const upload = await env.CLIPS.createMultipartUpload(`clips/${id}`, { httpMetadata: { contentType: "video/mp4" } });
  const t = now();
  await env.DB.batch([
    env.DB.prepare("INSERT INTO shares (id, sender, title, game, duration, size, sha256, markers, upload_id, parts, state, created) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'uploading', ?)").bind(
      id,
      me.id,
      cleanText(body.title, 120) || "Clip",
      cleanText(body.game, 60),
      Math.max(0, Number(body.duration) || 0),
      size,
      sha,
      markers,
      upload.uploadId,
      parts,
      t,
    ),
    ...to.map((uid) => env.DB.prepare("INSERT INTO recipients (share_id, user_id, state, updated) VALUES (?, ?, 'waiting', ?)").bind(id, uid, t)),
    env.DB.prepare("UPDATE users SET sent_day = ?, sent_bytes = ? WHERE id = ?").bind(today(), used + size, me.id),
  ]);
  return json({ id, part_size: LIMITS.part, parts });
}

async function ownShare(env: Env, me: User, id: string) {
  const s = await env.DB.prepare("SELECT * FROM shares WHERE id = ? AND sender = ?").bind(id, me.id).first<any>();
  if (!s) throw new Fail(404, "share_unknown");
  return s;
}

async function uploadPart(req: Request, env: Env, me: User, id: string, n: number): Promise<Response> {
  const s = await ownShare(env, me, id);
  if (s.state !== "uploading") throw new Fail(409, "share_not_uploading");
  if (!Number.isInteger(n) || n < 1 || n > s.parts) throw new Fail(400, "part_invalid");
  const len = Number(req.headers.get("content-length") ?? "0");
  const expected = n < s.parts ? LIMITS.part : s.size - LIMITS.part * (s.parts - 1);
  if (len !== expected || !req.body) throw new Fail(400, "part_size");
  const upload = env.CLIPS.resumeMultipartUpload(`clips/${id}`, s.upload_id);
  const part = await upload.uploadPart(n, req.body);
  await env.DB.prepare("INSERT INTO parts (share_id, n, etag) VALUES (?, ?, ?) ON CONFLICT (share_id, n) DO UPDATE SET etag = excluded.etag").bind(id, n, part.etag).run();
  return json({ ok: true });
}

async function uploadThumb(data: ArrayBuffer, env: Env, me: User, id: string): Promise<Response> {
  await ownShare(env, me, id);
  if (!data.byteLength || data.byteLength > LIMITS.maxThumb) throw new Fail(413, "thumb_too_big");
  await env.CLIPS.put(`thumbs/${id}`, data, { httpMetadata: { contentType: "image/jpeg" } });
  await env.DB.prepare("UPDATE shares SET has_thumb = 1 WHERE id = ?").bind(id).run();
  return json({ ok: true });
}

async function completeShare(env: Env, me: User, id: string): Promise<Response> {
  const s = await ownShare(env, me, id);
  if (s.state !== "uploading") return json({ ok: true });
  const rows = (await env.DB.prepare("SELECT n, etag FROM parts WHERE share_id = ? ORDER BY n").bind(id).all<{ n: number; etag: string }>()).results;
  if (rows.length !== s.parts) throw new Fail(409, "parts_missing");
  const upload = env.CLIPS.resumeMultipartUpload(`clips/${id}`, s.upload_id);
  await upload.complete(rows.map((r) => ({ partNumber: r.n, etag: r.etag })));
  const head = await env.CLIPS.head(`clips/${id}`);
  if (!head || head.size !== s.size) {
    await env.CLIPS.delete(`clips/${id}`);
    throw new Fail(409, "size_mismatch");
  }
  await env.DB.batch([
    env.DB.prepare("UPDATE shares SET state = 'ready', created = ? WHERE id = ?").bind(now(), id),
    env.DB.prepare("DELETE FROM parts WHERE share_id = ?").bind(id),
  ]);
  return json({ ok: true });
}

/** Le destinataire (ou l'envoyeur) lit le clip ou sa miniature ; reprise possible (Range). */
async function readObject(req: Request, env: Env, me: User, id: string, kind: "file" | "thumb"): Promise<Response> {
  const s = await env.DB.prepare("SELECT sender, state FROM shares WHERE id = ?").bind(id).first<{ sender: string; state: string }>();
  if (!s || s.state !== "ready") throw new Fail(404, "share_unknown");
  if (s.sender !== me.id) {
    const r = await env.DB.prepare("SELECT state FROM recipients WHERE share_id = ? AND user_id = ?").bind(id, me.id).first<{ state: string }>();
    if (r?.state !== "waiting") throw new Fail(404, "share_unknown");
  }
  const key = kind === "file" ? `clips/${id}` : `thumbs/${id}`;
  const obj = await env.CLIPS.get(key, { range: req.headers });
  if (!obj) throw new Fail(404, "file_gone");
  const headers = new Headers();
  obj.writeHttpMetadata(headers);
  headers.set("etag", obj.httpEtag);
  headers.set("accept-ranges", "bytes");
  const range = (obj as R2ObjectBody & { range?: { offset?: number; length?: number } }).range;
  if (req.headers.has("range") && range && typeof range.offset === "number") {
    const length = range.length ?? obj.size - range.offset;
    headers.set("content-range", `bytes ${range.offset}-${range.offset + length - 1}/${obj.size}`);
    headers.set("content-length", String(length));
    return new Response(obj.body, { status: 206, headers });
  }
  headers.set("content-length", String(obj.size));
  return new Response(obj.body, { headers });
}

/** Reçu (ou refusé) : quand plus personne n'attend, le fichier est supprimé tout de suite. */
async function finishShare(env: Env, me: User, id: string, body: any): Promise<Response> {
  const state = body.state === "dismissed" ? "dismissed" : "received";
  const r = await env.DB.prepare("UPDATE recipients SET state = ?, updated = ? WHERE share_id = ? AND user_id = ? AND state = 'waiting'").bind(state, now(), id, me.id).run();
  if (!r.meta.changes) return json({ ok: true });
  const left = await env.DB.prepare("SELECT COUNT(*) AS n FROM recipients WHERE share_id = ? AND state = 'waiting'").bind(id).first<{ n: number }>();
  if (!left?.n) await purge(env, id, true);
  return json({ ok: true });
}

/** Supprime les fichiers d'un envoi ; garde la ligne (« reçu par… ») si keepRow. */
async function purge(env: Env, id: string, keepRow: boolean) {
  const s = await env.DB.prepare("SELECT upload_id, state FROM shares WHERE id = ?").bind(id).first<{ upload_id: string; state: string }>();
  if (s?.state === "uploading") {
    try {
      await env.CLIPS.resumeMultipartUpload(`clips/${id}`, s.upload_id).abort();
    } catch {}
  }
  await env.CLIPS.delete([`clips/${id}`, `thumbs/${id}`]);
  if (keepRow) {
    await env.DB.prepare("UPDATE shares SET state = 'done' WHERE id = ?").bind(id).run();
  } else {
    await env.DB.batch([
      env.DB.prepare("DELETE FROM recipients WHERE share_id = ?").bind(id),
      env.DB.prepare("DELETE FROM parts WHERE share_id = ?").bind(id),
      env.DB.prepare("DELETE FROM shares WHERE id = ?").bind(id),
    ]);
  }
}

async function cancelShare(env: Env, me: User, id: string): Promise<Response> {
  await ownShare(env, me, id);
  await purge(env, id, false);
  return json({ ok: true });
}

async function report(env: Env, me: User, id: string, body: any): Promise<Response> {
  const s = await env.DB.prepare("SELECT sender FROM shares WHERE id = ?").bind(id).first<{ sender: string }>();
  if (!s) throw new Fail(404, "share_unknown");
  await env.DB.prepare("INSERT INTO reports (share_id, reporter, sender, reason, created) VALUES (?, ?, ?, ?, ?)").bind(id, me.id, s.sender, cleanText(body.reason, 500), now()).run();
  return json({ ok: true });
}

/** Pseudo et/ou « montrer quand je suis en ligne ». */
async function updateMe(env: Env, me: User, body: any): Promise<Response> {
  if (body.name !== undefined) {
    await env.DB.prepare("UPDATE users SET name = ? WHERE id = ?").bind(cleanName(body.name), me.id).run();
  }
  if (body.show_online !== undefined) {
    await env.DB.prepare("UPDATE users SET show_online = ? WHERE id = ?").bind(body.show_online ? 1 : 0, me.id).run();
  }
  return json({ ok: true });
}

/** Photo de profil : JPEG carré préparé par Rewind ; chaque changement donne une nouvelle version. */
async function setAvatar(env: Env, me: User, data: ArrayBuffer): Promise<Response> {
  const bytes = new Uint8Array(data);
  if (!bytes.length || bytes.length > LIMITS.maxAvatar) throw new Fail(413, "avatar_too_big");
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) throw new Fail(400, "avatar_not_jpeg");
  await env.CLIPS.put(`avatars/${me.id}`, data, { httpMetadata: { contentType: "image/jpeg" } });
  const version = now();
  await env.DB.prepare("UPDATE users SET avatar = ? WHERE id = ?").bind(version, me.id).run();
  return json({ ok: true, avatar: version });
}

async function removeAvatar(env: Env, me: User): Promise<Response> {
  await env.CLIPS.delete(`avatars/${me.id}`);
  await env.DB.prepare("UPDATE users SET avatar = 0 WHERE id = ?").bind(me.id).run();
  return json({ ok: true });
}

/** Photo d'un utilisateur : visible par lui-même, ses amis et les personnes en demande avec lui. */
async function getAvatar(env: Env, me: User, id: string): Promise<Response> {
  if (id !== me.id) {
    const rel = await relation(env, me.id, id);
    if (!rel || rel.status === "blocked") throw new Fail(404, "not_found");
  }
  const obj = await env.CLIPS.get(`avatars/${id}`);
  if (!obj) throw new Fail(404, "not_found");
  const headers = new Headers();
  obj.writeHttpMetadata(headers);
  headers.set("cache-control", "private, max-age=86400");
  return new Response(obj.body, { headers });
}

/** « Supprimer mon identité » : tout ce qui concerne cet utilisateur disparaît. */
async function deleteMe(env: Env, me: User): Promise<Response> {
  await deleteUser(env, me.id);
  return json({ ok: true });
}

async function deleteUser(env: Env, id: string) {
  const mine = (await env.DB.prepare("SELECT id FROM shares WHERE sender = ?").bind(id).all<{ id: string }>()).results;
  for (const s of mine) await purge(env, s.id, false);
  await env.DB.batch([
    env.DB.prepare("DELETE FROM recipients WHERE user_id = ?").bind(id),
    env.DB.prepare("DELETE FROM friends WHERE a = ? OR b = ?").bind(id, id),
    env.DB.prepare("DELETE FROM users WHERE id = ?").bind(id),
  ]);
  await env.CLIPS.delete(`avatars/${id}`);
}

// ---------------------------------------------------------------------------
// Aiguillage
// ---------------------------------------------------------------------------

async function route(req: Request, env: Env): Promise<Response> {
  const url = new URL(req.url);
  const p = url.pathname.replace(/\/+$/, "");
  const m = req.method;
  if (m === "GET" && (p === "" || p === "/v1/health")) return json({ ok: true, service: "rewind-partage", version: 1 });

  // Morceaux de vidéo : lus en flux, sans les garder en mémoire ni recalculer leur empreinte
  const isPart = m === "PUT" && /^\/v1\/shares\/[^/]+\/parts\/\d+$/.test(p);
  // Le reste (JSON, miniature) : lu et vérifié ici
  let body: ArrayBuffer | null = null;
  if (!isPart) {
    body = await req.arrayBuffer();
    const max = p.endsWith("/thumb") ? LIMITS.maxThumb : p.endsWith("/avatar") ? LIMITS.maxAvatar : 64 * 1024;
    if (body.byteLength > max) throw new Fail(413, "body_too_big");
  }
  if (m === "POST" && p === "/v1/register") return register(req, env, await readJson(body));
  if (m === "POST" && p === "/v1/recover") return recover(env, await readJson(body));

  const me = await authenticate(req, env, url, body);
  const data = m === "PUT" ? {} : await readJson(body);
  const share = p.match(/^\/v1\/shares\/([0-9a-f-]{36})(\/.*)?$/);

  if (m === "GET" && p === "/v1/sync") return sync(env, me);
  if (m === "POST" && p === "/v1/me") return updateMe(env, me, data);
  if (m === "POST" && p === "/v1/me/key") return rekey(env, me, data);
  if (m === "POST" && p === "/v1/me/recovery") return setRecovery(env, me, data);
  if (m === "PUT" && p === "/v1/me/avatar") return setAvatar(env, me, body ?? new ArrayBuffer(0));
  if (m === "DELETE" && p === "/v1/me/avatar") return removeAvatar(env, me);
  const av = p.match(/^\/v1\/users\/([0-9a-f-]{36})\/avatar$/);
  if (m === "GET" && av) return getAvatar(env, me, av[1]);
  if (m === "DELETE" && p === "/v1/me") return deleteMe(env, me);
  if (m === "POST" && p === "/v1/friends/request") return requestFriend(env, me, data);
  if (m === "POST" && p === "/v1/friends/answer") return answerFriend(env, me, data);
  if (m === "POST" && p === "/v1/friends/remove") return removeFriend(env, me, data);
  if (m === "POST" && p === "/v1/shares") return createShare(env, me, data);
  if (share) {
    const [, id, rest = ""] = share;
    const part = rest.match(/^\/parts\/(\d+)$/);
    if (m === "PUT" && part) return uploadPart(req, env, me, id, Number(part[1]));
    if (m === "PUT" && rest === "/thumb") return uploadThumb(body ?? new ArrayBuffer(0), env, me, id);
    if (m === "POST" && rest === "/complete") return completeShare(env, me, id);
    if (m === "GET" && rest === "/file") return readObject(req, env, me, id, "file");
    if (m === "GET" && rest === "/thumb") return readObject(req, env, me, id, "thumb");
    if (m === "POST" && rest === "/done") return finishShare(env, me, id, data);
    if (m === "POST" && rest === "/report") return report(env, me, id, data);
    if (m === "DELETE" && rest === "") return cancelShare(env, me, id);
  }
  throw new Fail(404, "not_found");
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    try {
      return await route(req, env);
    } catch (e) {
      if (e instanceof Fail) return json({ error: e.code }, e.status);
      console.error(e);
      return json({ error: "server_error" }, 500);
    }
  },

  /** Ménage quotidien : envois de plus de 7 jours et envois abandonnés en cours de route. */
  async scheduled(_event: ScheduledController, env: Env): Promise<void> {
    const old = now() - LIMITS.keepMs;
    const stale = now() - 86_400_000;
    const rows = (
      await env.DB.prepare("SELECT id FROM shares WHERE created < ? OR (state = 'uploading' AND created < ?) LIMIT 500").bind(old, stale).all<{ id: string }>()
    ).results;
    for (const r of rows) await purge(env, r.id, false);
    await env.DB.prepare("DELETE FROM ip_limits WHERE day < ?").bind(new Date(old).toISOString().slice(0, 10)).run();
  },
};
