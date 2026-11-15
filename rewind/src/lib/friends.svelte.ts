// État du partage entre amis côté interface : relu à chaque changement annoncé par Rewind (amis.rs).
import { listen } from "@tauri-apps/api/event";
import { api, friendError, type FriendsView } from "./api";
import { app } from "./state.svelte";
import { t } from "./i18n.svelte";

class Friends {
  view = $state<FriendsView | null>(null);
  /** Photos déjà en cache : « id-version » → chemin local ("" = en cours ou absente) */
  private avatars = $state<Record<string, string>>({});
  /** Photos déjà demandées (non réactif : on n'écrit pas d'état pendant l'affichage) */
  private requested = new Set<string>();
  private started = false;

  /** Chemin local de la photo (demandée à Rewind la première fois ; "" tant qu'elle n'est pas là). */
  avatarPath(id: string, version: number): string {
    if (!version) return "";
    const k = `${id}-${version}`;
    const got = this.avatars[k];
    if (got === undefined && !this.requested.has(k)) {
      this.requested.add(k);
      api.friendsAvatar(id, version).then((p) => (this.avatars[k] = p)).catch(() => {});
    }
    return got ?? "";
  }
  private reloading = false;
  private again = false;

  /** Pastille de la barre latérale : demandes reçues + clips pas encore reçus. */
  get badge(): number {
    const v = this.view;
    if (!v?.enabled || !v.ready) return 0;
    const received = new Set(v.received.map((r) => r.share_id));
    return v.incoming.length + v.inbox.filter((i) => !received.has(i.id)).length;
  }

  async start() {
    if (this.started) return;
    this.started = true;
    await this.reload();
    await listen("friends-changed", () => this.reload());
    await listen<{ from: string; title: string }>("friends-new-clip", (e) => {
      app.toast("info", t("{name} t'a envoyé un clip", { name: e.payload.from }), e.payload.title);
    });
  }

  /** Relecture groupée : plusieurs annonces rapprochées (progression) = une seule lecture à la fois. */
  async reload() {
    if (this.reloading) {
      this.again = true;
      return;
    }
    this.reloading = true;
    try {
      do {
        this.again = false;
        this.view = await api.friendsView();
      } while (this.again);
    } catch {
    } finally {
      this.reloading = false;
    }
  }

  /** Lance une action ; en cas d'erreur, un message clair (et false). */
  async run(action: () => Promise<unknown>, ok?: string): Promise<boolean> {
    try {
      await action();
      if (ok) app.toast("ok", ok);
      await this.reload();
      return true;
    } catch (e) {
      app.toast("err", friendError(e));
      await this.reload();
      return false;
    }
  }
}

export const friends = new Friends();
