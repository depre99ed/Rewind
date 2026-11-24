// Planche de développement : introuvable dans la version publiée (audit 0.4.12)
import { dev } from "$app/environment";
import { error } from "@sveltejs/kit";

export function load() {
  if (!dev) error(404, "Page de développement");
}
