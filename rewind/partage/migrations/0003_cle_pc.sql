-- 2026-11-15 : identité liée au PC + clé de secours séparée.
-- La clé de chaque Rewind est maintenant dérivée de l'identifiant du PC ; la clé de secours est une
-- autre clé, dont seule la partie publique est gardée ici (elle permet de rattacher le compte à un nouveau PC).
-- À appliquer UNE fois sur une base existante (wrangler d1 execute rewind-amis --remote --file migrations/0003_cle_pc.sql).
ALTER TABLE users ADD COLUMN recovery TEXT;  -- clé publique de secours (base64), NULL = pas encore créée
CREATE UNIQUE INDEX IF NOT EXISTS users_recovery ON users (recovery);
