<script lang="ts">
  // Page « Amis » : code ami, photo, demandes, liste d'amis (avec page de chaque ami), clips reçus et envoyés,
  // clé de secours (partage entre amis, amis.rs).
  import { Copy, Check, UserPlus, X, Download, Ban, Loader2, Play, RotateCw, Trash2, Film, Upload, ArrowLeft, KeyRound, Save, Camera } from "@lucide/svelte";
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { app } from "$lib/state.svelte";
  import { friends } from "$lib/friends.svelte";
  import { api, fileUrl, formatDuration, formatSize, relativeDate, type FriendIncoming, type FriendPerson } from "$lib/api";
  import { fit } from "$lib/fit";
  import { t } from "$lib/i18n.svelte";
  import Avatar from "./Avatar.svelte";

  const v = $derived(friends.view);
  let name = $state("");
  let code = $state("");
  let adding = $state(false);
  let enabling = $state(false);
  let copied = $state("");
  let editingName = $state(false);
  let thumbs = $state<Record<string, string>>({});
  // Ami ouvert (aperçu navigateur seulement : « ?ami=f1 » pour les captures)
  let selected = $state<string | null>(typeof window !== "undefined" && (window as any).__REWIND_MOCK__ ? new URLSearchParams(location.search).get("ami") : null);
  let recovery = $state("");
  let restoring = $state(false);
  let restoreText = $state("");
  let photoBusy = $state(false);

  $effect(() => {
    friends.start();
    api.friendsRefresh().catch(() => {});
  });

  // Miniatures des clips pas encore reçus (cache local de Rewind)
  $effect(() => {
    for (const i of v?.inbox ?? []) {
      if (i.has_thumb && thumbs[i.id] === undefined) {
        thumbs[i.id] = "";
        api.shareThumb(i.id).then((p) => (thumbs[i.id] = p)).catch(() => {});
      }
    }
  });

  const receivedIds = $derived(new Set((v?.received ?? []).map((r) => r.share_id)));
  const pending = $derived((v?.inbox ?? []).filter((i) => !receivedIds.has(i.id)));
  const downloads = $derived(new Map((v?.transfers ?? []).filter((x) => x.kind === "down").map((x) => [x.share_id, x])));
  const uploads = $derived((v?.transfers ?? []).filter((x) => x.kind === "up" && x.state !== "done"));
  const history = $derived((v?.received ?? []).map((r) => ({ r, clip: app.clips.find((c) => c.id === r.clip_id) ?? null })));
  const people = $derived(new Map([...(v?.friends ?? []), ...(v?.incoming ?? []), ...(v?.outgoing ?? [])].map((p) => [p.id, p])));
  const friend = $derived(selected ? (v?.friends.find((f) => f.id === selected) ?? null) : null);
  const onlineCount = $derived((v?.friends ?? []).filter((f) => f.online).length);
  // Page d'un ami : ce qu'il m'a envoyé et ce que je lui ai envoyé
  const fromFriend = $derived(friend ? history.filter((h) => h.r.from_id === friend.id) : []);
  const pendingFromFriend = $derived(friend ? pending.filter((i) => i.sender === friend.id) : []);
  const toFriend = $derived(friend ? (v?.sent ?? []).filter((s) => s.to.some((p) => p.id === friend.id)) : []);

  async function enable() {
    if (!name.trim()) return;
    enabling = true;
    await friends.run(() => api.friendsEnable(name.trim()));
    enabling = false;
  }
  async function restore() {
    if (!restoreText.trim()) return;
    enabling = true;
    await friends.run(() => api.friendsRestore(restoreText.trim()), t("Identité retrouvée : ton code et tes amis sont revenus"));
    enabling = false;
  }
  async function add() {
    if (!code.trim()) return;
    adding = true;
    let state = "";
    const ok = await friends.run(async () => (state = await api.friendsAdd(code.trim())));
    if (ok) {
      app.toast("ok", state === "friends" ? t("Vous êtes maintenant amis") : t("Demande envoyée"));
      code = "";
    }
    adding = false;
  }
  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = what;
      setTimeout(() => (copied = ""), 1600);
    } catch {}
  }
  async function saveName(e: Event) {
    const value = (e.currentTarget as HTMLInputElement).value.trim();
    editingName = false;
    if (value && value !== v?.me?.name) await friends.run(() => api.friendsRename(value));
  }
  async function choosePhoto() {
    const path = await openDialog({ multiple: false, directory: false, filters: [{ name: t("Images"), extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp"] }] }).catch(() => null);
    if (typeof path !== "string") return;
    photoBusy = true;
    await friends.run(() => api.friendsSetAvatar(path), t("Photo de profil mise à jour"));
    photoBusy = false;
  }
  async function showRecovery() {
    try {
      recovery = recovery ? "" : await api.friendsRecoveryKey();
    } catch (e) {
      friends.run(() => Promise.reject(e));
    }
  }
  async function saveRecovery() {
    const path = await saveDialog({ defaultPath: "Rewind - cle de secours.txt", filters: [{ name: t("Texte"), extensions: ["txt"] }] }).catch(() => null);
    if (path) await friends.run(() => api.friendsSaveRecovery(path), t("Clé de secours enregistrée"));
  }
  function removeFriend(p: FriendPerson) {
    if (confirm(t("Retirer {name} de tes amis ?", { name: p.name }))) {
      selected = null;
      friends.run(() => api.friendsRemove(p.id, false));
    }
  }
  function blockFriend(p: FriendPerson) {
    if (confirm(t("Bloquer {name} ? Cette personne ne pourra plus t'envoyer de clips ni te redemander en ami.", { name: p.name }))) {
      selected = null;
      friends.run(() => api.friendsRemove(p.id, true));
    }
  }
  function dismiss(i: FriendIncoming) {
    if (confirm(t("Refuser « {title} » ? Le clip sera supprimé sans être téléchargé.", { title: i.title }))) friends.run(() => api.shareDismiss(i.id));
  }
  function report(i: FriendIncoming) {
    const reason = prompt(t("Pourquoi signaler ce clip ? (contenu choquant, harcèlement…)"));
    if (reason) friends.run(() => api.shareReport(i.id, reason), t("Signalement envoyé"));
  }
  function deleteIdentity() {
    if (confirm(t("Supprimer ton identité ? Ton code ami, ta liste d'amis et tes clips en attente sont effacés chez Cloudflare. Les clips déjà reçus restent sur ton PC."))) {
      friends.run(() => api.friendsDeleteIdentity(), t("Identité supprimée"));
    }
  }
  const sentLabel = (state: string) => (state === "received" ? t("reçu") : state === "dismissed" ? t("refusé") : t("en attente"));
  const ago = (ms: number) => relativeDate(ms / 1000);
  const sender = (id: string) => people.get(id);
</script>

{#snippet pendingCard(i: FriendIncoming)}
  {@const d = downloads.get(i.id)}
  {@const who = sender(i.sender)}
  <div class="clip">
    <div class="thumb">
      {#if thumbs[i.id]}<img use:fit src={fileUrl(thumbs[i.id])} alt="" />{:else}<Film size={18} />{/if}
      <span class="dur mono">{formatDuration(i.duration)}</span>
      {#if d?.state === "running"}<i class="bar" style="--p:{d.progress}"></i>{/if}
    </div>
    <div class="meta">
      <strong title={i.title}>{i.title}</strong>
      <span class="by"><Avatar id={i.sender} version={who?.avatar ?? 0} name={i.sender_name} size={16} /><span class="muted">{i.sender_name} · {ago(i.created)}</span></span>
    </div>
    <div class="acts">
      {#if d?.state === "running"}
        <span class="faint mono">{t("Réception…")} {Math.round(d.progress * 100)} %</span>
      {:else}
        {#if d?.state === "error"}<span class="err" title={d.error}>{t("Échec")}</span>{/if}
        <button class="btn small" onclick={() => friends.run(() => api.shareDownload(i.id))}><Download size={14} />{d?.state === "error" ? t("Réessayer") : t("Recevoir")}</button>
        <button class="btn small ghost icon" onclick={() => dismiss(i)} title={t("Refuser")} aria-label={t("Refuser")}><X size={14} /></button>
        <button class="btn small ghost icon" onclick={() => report(i)} title={t("Signaler")} aria-label={t("Signaler")}><Ban size={14} /></button>
      {/if}
    </div>
  </div>
{/snippet}

{#snippet doneCard(h: (typeof history)[number])}
  {@const who = sender(h.r.from_id)}
  <button class="clip done" onclick={() => h.clip && (app.openClip = h.clip)} disabled={!h.clip}>
    <div class="thumb">
      {#if h.clip?.thumb}<img use:fit src={fileUrl(h.clip.thumb)} alt="" />{:else}<Film size={18} />{/if}
      {#if h.clip}<span class="dur mono">{formatDuration(h.clip.duration)}</span><span class="play"><Play size={16} fill="currentColor" /></span>{/if}
    </div>
    <div class="meta">
      <strong title={h.r.title}>{h.r.title}</strong>
      <span class="by"><Avatar id={h.r.from_id} version={who?.avatar ?? 0} name={h.r.from_name} size={16} /><span class="muted">{h.r.from_name} · {relativeDate(h.r.at)}{#if !h.clip} · {t("supprimé")}{/if}</span></span>
    </div>
  </button>
{/snippet}

{#snippet sentRow(s: NonNullable<typeof v>["sent"][number])}
  <div class="sent">
    <Check size={15} class="faint" />
    <div class="stext">
      <strong>{s.title}</strong>
      <span class="muted">{#each s.to as p, k}{k ? ", " : ""}{p.name} <em class:ok={p.state === "received"}>{sentLabel(p.state)}</em>{/each}</span>
    </div>
    <span class="faint">{ago(s.created)}</span>
    {#if s.to.some((p) => p.state === "waiting")}
      <button class="btn small ghost icon" onclick={() => friends.run(() => api.shareCancel(s.id))} title={t("Annuler l'envoi")} aria-label={t("Annuler l'envoi")}><X size={14} /></button>
    {/if}
  </div>
{/snippet}

<section class="friends appear">
  <header>
    <div>
      <h1>{t("Amis")}</h1>
      <p class="muted">{t("Envoie tes clips directement dans le Rewind de tes amis. Pas de compte : un code ami suffit.")}</p>
    </div>
    {#if v?.ready}
      <label class="master">
        <span>{t("Partage")}</span>
        <span class="switch"><input type="checkbox" checked={v.enabled} onchange={(e) => friends.run(() => (e.currentTarget.checked ? api.friendsEnable(v.me?.name ?? "") : api.friendsDisable()))} /><span></span></span>
      </label>
    {/if}
  </header>

  {#if !v}
    <div class="loading"><Loader2 size={18} class="spin" /></div>
  {:else if !v.ready || !v.enabled}
    <!-- Avant l'activation : ce qui se passe, en clair ; rien n'est envoyé avant le clic -->
    <div class="intro card">
      <h2>{v.ready ? t("Le partage est coupé") : t("Activer le partage entre amis")}</h2>
      <ul>
        <li><strong>{t("Comment ça marche")}</strong><span class="muted">{t("Tu choisis un clip et des amis. Le clip arrive tout seul dans leur Rewind, onglet Amis.")}</span></li>
        <li><strong>{t("Où vont les clips")}</strong><span class="muted">{t("Ils passent par un stockage Cloudflare en Europe, le temps d'être reçus : supprimés dès réception, au plus tard après 7 jours.")}</span></li>
        <li><strong>{t("Ce qui est gardé")}</strong><span class="muted">{t("Ton pseudo, ta photo si tu en mets une, ton code ami et ta liste d'amis. Pas d'e-mail, pas de mot de passe. Tu peux tout effacer à tout moment.")}</span></li>
      </ul>
      {#if v.ready}
        <button class="btn primary" onclick={() => friends.run(() => api.friendsEnable(v.me?.name ?? ""))}>{t("Réactiver le partage")}</button>
      {:else if restoring}
        <form class="restore" onsubmit={(e) => { e.preventDefault(); restore(); }}>
          <span class="muted">{t("Colle ta clé de secours (RWK-…) : tu retrouves ton code ami, ta photo et tes amis.")}</span>
          <textarea class="field mono" rows="3" placeholder="RWK-XXXX-XXXX-…" bind:value={restoreText}></textarea>
          <div class="row2">
            <button type="button" class="btn ghost" onclick={() => (restoring = false)}>{t("Annuler")}</button>
            <button class="btn primary" disabled={!restoreText.trim() || enabling}>{#if enabling}<Loader2 size={15} class="spin" />{/if}{t("Restaurer")}</button>
          </div>
        </form>
      {:else}
        <form class="enable" onsubmit={(e) => { e.preventDefault(); enable(); }}>
          <input class="field" placeholder={t("Ton pseudo (vu par tes amis)")} bind:value={name} maxlength="24" />
          <button class="btn primary" disabled={!name.trim() || enabling}>{#if enabling}<Loader2 size={15} class="spin" />{/if}{t("Activer le partage")}</button>
        </form>
        <button class="btn ghost small hasKey" onclick={() => (restoring = true)}><KeyRound size={14} />{t("J'ai déjà une clé de secours")}</button>
      {/if}
    </div>
  {:else}
    <div class="cols">
      <div class="main">
        {#if friend}
          <!-- Page d'un ami -->
          <div class="card fhead">
            <button class="btn small ghost back" onclick={() => (selected = null)}><ArrowLeft size={14} />{t("Tous les amis")}</button>
            <div class="fwho">
              <Avatar id={friend.id} version={friend.avatar} name={friend.name} size={56} online={friend.online} />
              <div>
                <h2 class="big">{friend.name}</h2>
                <span class="muted">{friend.online ? t("En ligne") : t("Hors ligne")} · {t("amis depuis {when}", { when: ago(friend.since) })}</span>
              </div>
              <span class="grow"></span>
              <button class="btn small ghost" onclick={() => removeFriend(friend)}><X size={14} />{t("Retirer")}</button>
              <button class="btn small ghost danger" onclick={() => blockFriend(friend)}><Ban size={14} />{t("Bloquer")}</button>
            </div>
            <p class="faint tip">{t("Pour lui envoyer un clip : ouvre le clip, puis « Envoyer à un ami ».")}</p>
          </div>
          <div class="card">
            <div class="chead"><h2>{t("Ses clips")}</h2><span class="faint mono">{fromFriend.length + pendingFromFriend.length}</span></div>
            {#if fromFriend.length + pendingFromFriend.length === 0}
              <p class="empty muted">{t("{name} ne t'a encore rien envoyé.", { name: friend.name })}</p>
            {:else}
              <div class="grid">
                {#each pendingFromFriend as i (i.id)}{@render pendingCard(i)}{/each}
                {#each fromFriend as h (h.r.share_id)}{@render doneCard(h)}{/each}
              </div>
            {/if}
          </div>
          <div class="card">
            <div class="chead"><h2>{t("Tes envois")}</h2><span class="faint mono">{toFriend.length}</span></div>
            {#if toFriend.length === 0}<p class="empty muted">{t("Rien d'envoyé pour l'instant.")}</p>{/if}
            {#each toFriend as s (s.id)}{@render sentRow(s)}{/each}
          </div>
        {:else}
          <!-- Clips reçus : en attente, en cours, déjà reçus -->
          <div class="card">
            <div class="chead">
              <h2>{t("Reçus")}</h2>
              {#if !v.online && v.error}<span class="offline">{t("Hors ligne")}</span>{/if}
            </div>
            {#if pending.length === 0 && history.length === 0}
              <p class="empty muted">{t("Aucun clip reçu pour l'instant. Donne ton code ami pour qu'on t'en envoie.")}</p>
            {:else}
              <div class="grid">
                {#each pending as i (i.id)}{@render pendingCard(i)}{/each}
                {#each history as h (h.r.share_id)}{@render doneCard(h)}{/each}
              </div>
            {/if}
          </div>

          <!-- Envoyés -->
          <div class="card">
            <div class="chead"><h2>{t("Envoyés")}</h2><span class="faint">{t("Envoie un clip depuis son lecteur : « Envoyer à un ami ».")}</span></div>
            {#if uploads.length === 0 && v.sent.length === 0}
              <p class="empty muted">{t("Rien d'envoyé pour l'instant.")}</p>
            {/if}
            {#each uploads as u (u.key)}
              <div class="sent">
                <Upload size={15} class="faint" />
                <div class="stext"><strong>{u.title}</strong><span class="muted">{t("à {names}", { names: u.peer })}{#if u.state === "error"} · <span class="err">{t("Échec de l'envoi")}</span>{/if}</span></div>
                {#if u.state === "running"}
                  <span class="prog"><i style="--p:{u.progress}"></i></span>
                  <span class="faint mono pct">{Math.round(u.progress * 100)} %</span>
                  <button class="btn small ghost icon" onclick={() => friends.run(() => api.shareCancel(u.key))} title={t("Annuler")} aria-label={t("Annuler")}><X size={14} /></button>
                {:else}
                  <span class="faint">{u.error ? t("Réessaie depuis le lecteur du clip") : ""}</span>
                {/if}
              </div>
            {/each}
            {#each v.sent as s (s.id)}{@render sentRow(s)}{/each}
          </div>
        {/if}
      </div>

      <aside class="side">
        <!-- Moi : photo, pseudo, code ami -->
        <div class="card me">
          <div class="mehead">
            <button class="photo" onclick={choosePhoto} title={t("Changer la photo de profil")} aria-label={t("Changer la photo de profil")} disabled={photoBusy}>
              <Avatar id={v.me?.id ?? ""} version={v.me?.avatar ?? 0} name={v.me?.name ?? ""} size={48} />
              <span class="cam">{#if photoBusy}<Loader2 size={14} class="spin" />{:else}<Camera size={14} />{/if}</span>
            </button>
            <div class="who">
              {#if editingName}
                <!-- svelte-ignore a11y_autofocus -->
                <input class="field" value={v.me?.name} maxlength="24" autofocus onblur={saveName} onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()} />
              {:else}
                <button class="name" onclick={() => (editingName = true)} title={t("Changer de pseudo")}>{v.me?.name}</button>
              {/if}
              {#if v.me?.avatar}<button class="link faint" onclick={() => friends.run(() => api.friendsRemoveAvatar())}>{t("Retirer la photo")}</button>{/if}
            </div>
          </div>
          <span class="label">{t("Ton code ami")}</span>
          <div class="code">
            <span class="mono">{v.me?.code}</span>
            <button class="btn small icon" onclick={() => copy(v.me?.code ?? "", "code")} title={t("Copier")} aria-label={t("Copier")}>{#if copied === "code"}<Check size={14} />{:else}<Copy size={14} />{/if}</button>
          </div>
          <p class="faint state">
            {#if v.online}{t("Relevé {when}", { when: relativeDate(v.synced_at) })}{:else}{t("Hors ligne : nouvel essai bientôt")}{/if}
            <button class="btn small ghost icon" onclick={() => api.friendsRefresh()} title={t("Relever maintenant")} aria-label={t("Relever maintenant")}><RotateCw size={13} /></button>
          </p>
        </div>

        <!-- Ajouter un ami -->
        <form class="card addf" onsubmit={(e) => { e.preventDefault(); add(); }}>
          <span class="label">{t("Ajouter un ami")}</span>
          <div class="row">
            <input class="field mono" placeholder="RW-XXXX-XXXX" bind:value={code} maxlength="14" />
            <button class="btn" disabled={!code.trim() || adding}><UserPlus size={15} />{t("Ajouter")}</button>
          </div>
        </form>

        {#if v.incoming.length}
          <div class="card list">
            <span class="label">{t("Demandes reçues")}</span>
            {#each v.incoming as p (p.id)}
              <div class="person">
                <Avatar id={p.id} version={p.avatar} name={p.name} size={26} />
                <span class="pname">{p.name}</span>
                <button class="btn small primary" onclick={() => friends.run(() => api.friendsAnswer(p.id, true))}>{t("Accepter")}</button>
                <button class="btn small ghost icon" onclick={() => friends.run(() => api.friendsAnswer(p.id, false))} title={t("Refuser")} aria-label={t("Refuser")}><X size={14} /></button>
              </div>
            {/each}
          </div>
        {/if}

        <div class="card list">
          <span class="label">{t("Amis")} <em class="mono">{v.friends.length}</em>{#if onlineCount}<em class="faint"> · {t("{n} en ligne", { n: onlineCount })}</em>{/if}</span>
          {#each [...v.friends].sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name)) as p (p.id)}
            <button class="person pick" class:sel={selected === p.id} onclick={() => (selected = selected === p.id ? null : p.id)}>
              <Avatar id={p.id} version={p.avatar} name={p.name} size={26} online={p.online} />
              <span class="pname">{p.name}</span>
              <span class="faint since">{p.online ? t("en ligne") : t("depuis {when}", { when: ago(p.since) })}</span>
            </button>
          {:else}
            <p class="empty muted">{t("Ajoute un ami avec son code, ou donne-lui le tien.")}</p>
          {/each}
          {#if v.outgoing.length}
            <span class="label sub">{t("En attente de réponse")}</span>
            {#each v.outgoing as p (p.id)}
              <div class="person">
                <Avatar id={p.id} version={p.avatar} name={p.name} size={26} />
                <span class="pname faint">{p.name}</span>
                <button class="btn small ghost" onclick={() => friends.run(() => api.friendsRemove(p.id, false))}>{t("Annuler")}</button>
              </div>
            {/each}
          {/if}
          {#if v.blocked.length}
            <span class="label sub">{t("Bloqués")}</span>
            {#each v.blocked as p (p.id)}
              <div class="person">
                <span class="pname faint">{p.name}</span>
                <button class="btn small ghost" onclick={() => friends.run(() => api.friendsRemove(p.id, false))}>{t("Débloquer")}</button>
              </div>
            {/each}
          {/if}
        </div>

        <!-- Clé de secours -->
        <div class="card keyc">
          <span class="label">{t("Clé de secours")}</span>
          <p class="muted small">{t("Garde-la pour retrouver ton code ami et tes amis après une réinstallation ou sur un autre PC. Ne la donne à personne.")}</p>
          {#if recovery}
            <div class="rkey">
              <span class="mono">{recovery}</span>
              <button class="btn small icon" onclick={() => copy(recovery, "key")} title={t("Copier")} aria-label={t("Copier")}>{#if copied === "key"}<Check size={14} />{:else}<Copy size={14} />{/if}</button>
            </div>
          {/if}
          <div class="row2">
            <button class="btn small" onclick={showRecovery}><KeyRound size={14} />{recovery ? t("Masquer") : t("Afficher")}</button>
            <button class="btn small" onclick={saveRecovery}><Save size={14} />{t("Enregistrer dans un fichier")}</button>
          </div>
        </div>

        <div class="card foot">
          <label class="opt">
            <span><strong>{t("Recevoir automatiquement")}</strong><small class="muted">{t("Les clips de tes amis se téléchargent dès qu'ils arrivent.")}</small></span>
            <span class="switch"><input type="checkbox" checked={app.settings?.share_auto_download ?? true} onchange={(e) => app.patch((x) => (x.share_auto_download = e.currentTarget.checked))} /><span></span></span>
          </label>
          <label class="opt">
            <span><strong>{t("Montrer quand je suis en ligne")}</strong><small class="muted">{t("Tes amis voient un point quand Rewind est ouvert sur ton PC.")}</small></span>
            <span class="switch"><input type="checkbox" checked={v.me?.show_online ?? true} onchange={(e) => friends.run(() => api.friendsShowOnline(e.currentTarget.checked))} /><span></span></span>
          </label>
          <p class="faint quota">{t("Envoyé aujourd'hui : {used} sur {max}", { used: formatSize(v.limits.used_today), max: formatSize(v.limits.daily_bytes) })}</p>
          <button class="btn small ghost danger wipe" onclick={deleteIdentity}><Trash2 size={14} />{t("Supprimer mon identité")}</button>
        </div>
      </aside>
    </div>
  {/if}
</section>

<style>
  .friends { height: 100%; overflow-y: auto; padding: 22px 28px 40px; }
  header { display: flex; justify-content: space-between; align-items: flex-start; gap: 30px; margin-bottom: 20px; }
  h1 { margin: 0 0 4px; font-weight: 600; font-size: 22px; letter-spacing: -0.025em; }
  header p { margin: 0; max-width: 620px; font-size: 13.5px; }
  h2 { margin: 0; font-size: 14px; font-weight: 600; }
  h2.big { font-size: 18px; letter-spacing: -0.015em; }
  .master { display: flex; align-items: center; gap: 12px; padding: 10px 14px; border-radius: 12px; background: var(--panel); border: 1px solid var(--line); font-weight: 600; white-space: nowrap; }
  .loading { display: grid; place-items: center; height: 200px; color: var(--text-3); }
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius-l); padding: 16px 18px; box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025); }
  .label { font-size: 11px; font-weight: 600; letter-spacing: 0.05em; text-transform: uppercase; color: var(--text-3); }
  .label em { font-style: normal; margin-left: 4px; text-transform: none; letter-spacing: 0; }
  .label.sub { display: block; margin-top: 14px; }
  .empty { margin: 8px 0 2px; font-size: 13px; }
  .err { color: var(--rec); font-size: 12px; }
  .small { font-size: 12px; margin: 0; }
  .row2 { display: flex; gap: 8px; }
  .link { font-size: 12px; text-align: left; padding: 0; }
  .link:hover { color: var(--text); }

  /* Avant activation */
  .intro { max-width: 640px; display: flex; flex-direction: column; gap: 16px; padding: 22px 24px; }
  .intro h2 { font-size: 16px; }
  .intro ul { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: 12px; }
  .intro li { display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .enable { display: flex; gap: 8px; }
  .enable .field { flex: 1; }
  .hasKey { align-self: flex-start; margin: -8px 0 0 -10px; }
  .restore { display: flex; flex-direction: column; gap: 10px; font-size: 13px; }
  .restore textarea { height: auto; padding: 8px 10px; resize: none; text-transform: uppercase; }
  .restore .row2 { justify-content: flex-end; }

  .cols { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 18px; align-items: start; }
  .main, .side { display: flex; flex-direction: column; gap: 14px; }
  .chead { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
  .chead .faint { font-size: 12px; }
  .offline { font-size: 12px; color: var(--text-2); }

  /* Page d'un ami */
  .fhead { display: flex; flex-direction: column; gap: 12px; }
  .back { align-self: flex-start; margin-left: -10px; }
  .fwho { display: flex; align-items: center; gap: 14px; }
  .fwho > div { display: flex; flex-direction: column; gap: 3px; font-size: 13px; }
  .grow { flex: 1; }
  .tip { margin: 0; font-size: 12px; }

  /* Clips */
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 14px; }
  .clip { display: flex; flex-direction: column; gap: 8px; min-width: 0; text-align: left; border-radius: 10px; }
  .clip.done:disabled { opacity: 0.55; }
  .thumb { position: relative; aspect-ratio: 16 / 9; border-radius: 9px; overflow: hidden; display: grid; place-items: center; background: #0d0e12; color: var(--text-3); }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .dur { position: absolute; right: 6px; bottom: 6px; padding: 1px 5px; border-radius: 4px; font-size: 10.5px; background: rgba(0, 0, 0, 0.7); color: #fff; }
  .play { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; opacity: 0; background: rgba(0, 0, 0, 0.35); transition: opacity var(--dur) var(--ease); }
  .clip.done:hover:not(:disabled) .play { opacity: 1; }
  .bar { position: absolute; left: 0; right: 0; bottom: 0; height: 3px; background: color-mix(in srgb, #fff 18%, transparent); }
  .bar::after { content: ""; position: absolute; left: 0; top: 0; bottom: 0; width: calc(var(--p) * 100%); background: #fff; transition: width 300ms linear; }
  .meta { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .meta strong { font-size: 13px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .by { display: flex; align-items: center; gap: 6px; min-width: 0; font-size: 12px; }
  .by .muted { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .acts { display: flex; align-items: center; gap: 4px; min-height: 28px; }
  .acts .btn:first-of-type { flex: 1; }
  .acts .mono { font-size: 12px; }

  /* Envoyés */
  .sent { display: flex; align-items: center; gap: 12px; padding: 10px 0; border-top: 1px solid var(--line); font-size: 13px; }
  .chead + .sent, .chead + .empty + .sent { border-top: none; }
  .stext { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .stext strong { font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .stext span { font-size: 12px; }
  .stext em { font-style: normal; color: var(--text-3); }
  .stext em.ok { color: var(--text); }
  .sent > .faint { font-size: 12px; white-space: nowrap; }
  .prog { position: relative; width: 120px; height: 4px; border-radius: 2px; background: var(--panel-3); overflow: hidden; }
  .prog i { position: absolute; left: 0; top: 0; bottom: 0; width: calc(var(--p) * 100%); background: var(--hi); transition: width 300ms linear; }
  .pct { width: 38px; text-align: right; font-size: 12px; }

  /* Colonne de droite */
  .me { display: flex; flex-direction: column; gap: 10px; }
  .mehead { display: flex; align-items: center; gap: 12px; margin-bottom: 4px; }
  .photo { position: relative; border-radius: 50%; }
  .cam { position: absolute; right: -2px; bottom: -2px; width: 22px; height: 22px; border-radius: 50%; display: grid; place-items: center; background: var(--panel-3); border: 2px solid var(--panel); color: var(--text-2); transition: color var(--dur) var(--ease); }
  .photo:hover .cam { color: var(--text); }
  .who { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; min-width: 0; }
  .who .field { width: 170px; }
  .name { font-weight: 600; font-size: 15px; padding: 2px 6px; margin-left: -6px; border-radius: 6px; }
  .name:hover { background: var(--panel-2); }
  .code { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .code .mono { font-size: 20px; font-weight: 600; letter-spacing: 0.04em; user-select: text; }
  .state { display: flex; align-items: center; justify-content: space-between; margin: 0; font-size: 12px; }
  .addf { display: flex; flex-direction: column; gap: 10px; }
  .addf .row { display: flex; gap: 8px; }
  .addf .field { flex: 1; min-width: 0; text-transform: uppercase; }
  .list { display: flex; flex-direction: column; }
  .list .label { margin-bottom: 6px; }
  .person { display: flex; align-items: center; gap: 10px; min-height: 40px; border-top: 1px solid var(--line); text-align: left; }
  .label + .person { border-top: none; }
  .person.pick { margin: 0 -8px; padding: 0 8px; border-radius: 8px; border-top-color: transparent; transition: background var(--dur) var(--ease); }
  .person.pick:hover { background: var(--panel-2); }
  .person.pick.sel { background: var(--panel-3); }
  .pname { flex: 1; min-width: 0; font-size: 13px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .since { font-size: 11.5px; white-space: nowrap; }
  .keyc { display: flex; flex-direction: column; gap: 10px; }
  .rkey { display: flex; align-items: flex-start; gap: 8px; padding: 8px 10px; border-radius: var(--radius-s); background: var(--bg-2); border: 1px solid var(--line-2); }
  .rkey .mono { flex: 1; font-size: 11.5px; line-height: 1.5; word-break: break-all; user-select: text; }
  .foot { display: flex; flex-direction: column; gap: 12px; }
  .opt { display: flex; align-items: center; justify-content: space-between; gap: 14px; }
  .opt > span:first-child { display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .opt small { font-size: 12px; }
  .quota { margin: 0; font-size: 12px; }
  .wipe { align-self: flex-start; margin-left: -10px; }
  @media (max-width: 1100px) { .cols { grid-template-columns: 1fr; } }
</style>
