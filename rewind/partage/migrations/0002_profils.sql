-- 2026-11-15 : photos de profil et statut « en ligne ».
-- À appliquer UNE fois sur une base existante (wrangler d1 execute rewind-amis --remote --file migrations/0002_profils.sql).
ALTER TABLE users ADD COLUMN avatar INTEGER NOT NULL DEFAULT 0;      -- version de la photo (0 = aucune)
ALTER TABLE users ADD COLUMN show_online INTEGER NOT NULL DEFAULT 1; -- l'utilisateur accepte de montrer qu'il est en ligne
