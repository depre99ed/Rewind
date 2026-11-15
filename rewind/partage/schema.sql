-- Rewind : partage de clips entre amis (base D1 « rewind-amis »).
-- Aucun compte : un utilisateur = une clé publique Ed25519 créée par son Rewind.

CREATE TABLE IF NOT EXISTS users (
  id         TEXT PRIMARY KEY,           -- identifiant interne (aléatoire)
  pubkey     TEXT NOT NULL UNIQUE,       -- clé publique Ed25519 (base64, 32 octets)
  name       TEXT NOT NULL,              -- pseudo affiché aux amis
  code       TEXT NOT NULL UNIQUE,       -- code ami « RW-XXXX-XXXX »
  created    INTEGER NOT NULL,
  last_seen  INTEGER NOT NULL,
  sent_day   TEXT NOT NULL DEFAULT '',   -- quota d'envoi du jour (AAAA-MM-JJ)
  sent_bytes INTEGER NOT NULL DEFAULT 0,
  avatar     INTEGER NOT NULL DEFAULT 0, -- version de la photo de profil (0 = aucune)
  show_online INTEGER NOT NULL DEFAULT 1,
  recovery   TEXT                        -- clé publique de secours (base64), NULL = pas encore créée
);
CREATE UNIQUE INDEX IF NOT EXISTS users_recovery ON users (recovery);

-- Une ligne par paire (a < b) : demande en attente, amis, ou bloqué
CREATE TABLE IF NOT EXISTS friends (
  a         TEXT NOT NULL,
  b         TEXT NOT NULL,
  status    TEXT NOT NULL,               -- 'pending' | 'accepted' | 'blocked'
  requester TEXT NOT NULL,               -- qui a demandé (ou qui a bloqué)
  created   INTEGER NOT NULL,
  PRIMARY KEY (a, b)
);
CREATE INDEX IF NOT EXISTS friends_b ON friends (b);

-- Un clip envoyé (le fichier est dans R2 : clips/<id>, miniature thumbs/<id>)
CREATE TABLE IF NOT EXISTS shares (
  id        TEXT PRIMARY KEY,
  sender    TEXT NOT NULL,
  title     TEXT NOT NULL,
  game      TEXT NOT NULL,
  duration  REAL NOT NULL,
  size      INTEGER NOT NULL,
  sha256    TEXT NOT NULL,
  markers   TEXT NOT NULL DEFAULT '[]',  -- repères (kills CS2…), JSON
  upload_id TEXT NOT NULL,               -- envoi en plusieurs morceaux R2
  parts     INTEGER NOT NULL,
  state     TEXT NOT NULL,               -- 'uploading' | 'ready' | 'done'
  has_thumb INTEGER NOT NULL DEFAULT 0,
  created   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS shares_sender ON shares (sender);
CREATE INDEX IF NOT EXISTS shares_created ON shares (created);

CREATE TABLE IF NOT EXISTS recipients (
  share_id TEXT NOT NULL,
  user_id  TEXT NOT NULL,
  state    TEXT NOT NULL,                -- 'waiting' | 'received' | 'dismissed'
  updated  INTEGER NOT NULL,
  PRIMARY KEY (share_id, user_id)
);
CREATE INDEX IF NOT EXISTS recipients_user ON recipients (user_id);

-- Morceaux déjà envoyés (permet de reprendre un envoi interrompu)
CREATE TABLE IF NOT EXISTS parts (
  share_id TEXT NOT NULL,
  n        INTEGER NOT NULL,
  etag     TEXT NOT NULL,
  PRIMARY KEY (share_id, n)
);

-- Inscriptions par adresse IP et par jour (anti-abus)
CREATE TABLE IF NOT EXISTS ip_limits (
  ip    TEXT NOT NULL,
  day   TEXT NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (ip, day)
);

-- Signalements d'abus (lus par le responsable avec wrangler d1 execute)
CREATE TABLE IF NOT EXISTS reports (
  share_id TEXT NOT NULL,
  reporter TEXT NOT NULL,
  sender   TEXT NOT NULL,
  reason   TEXT NOT NULL,
  created  INTEGER NOT NULL
);
