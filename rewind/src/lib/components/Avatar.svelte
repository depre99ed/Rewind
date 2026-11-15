<script lang="ts">
  // Photo de profil d'un ami (ou la mienne) : image en cache, sinon initiale sur un rond neutre.
  // Le petit point de couleur ne sert qu'à signaler « Rewind ouvert en ce moment ».
  import { friends } from "$lib/friends.svelte";
  import { fileUrl } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  let { id, version = 0, name = "", size = 32, online = false }: { id: string; version?: number; name?: string; size?: number; online?: boolean } = $props();
  const src = $derived(friends.avatarPath(id, version));
  const initial = $derived((name.trim()[0] ?? "?").toUpperCase());
</script>

<span class="av" style="--s:{size}px" aria-hidden="true">
  {#if src}<img src={fileUrl(src)} alt="" />{:else}<span class="ini">{initial}</span>{/if}
  {#if online}<i class="on" title={t("En ligne")}></i>{/if}
</span>

<style>
  .av { position: relative; flex: none; width: var(--s); height: var(--s); border-radius: 50%; background: var(--panel-3); display: inline-grid; place-items: center; }
  .av img { width: 100%; height: 100%; border-radius: 50%; object-fit: cover; }
  .ini { font-weight: 600; font-size: calc(var(--s) * 0.42); color: var(--text-2); line-height: 1; }
  .on { position: absolute; right: 0; bottom: 0; width: max(8px, calc(var(--s) * 0.26)); height: max(8px, calc(var(--s) * 0.26)); border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 2px var(--panel); }
</style>
