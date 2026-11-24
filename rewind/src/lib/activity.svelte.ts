// Activité de la fenêtre (2026-11-24, audit de performances).
//
// Rewind tourne des heures à côté d'un jeu : quand sa fenêtre est réduite, masquée ou simplement
// derrière le jeu, l'interface ne doit presque rien consommer. Ce module expose deux états réactifs :
//  - `visible` : la page est affichée (pas réduite, pas entièrement masquée) ;
//  - `active`  : affichée ET au premier plan (la fenêtre a le focus).
// Il pose aussi `data-bg="1"` sur <html> quand la fenêtre n'est pas active : le CSS met alors toutes
// les animations en pause (aucun changement visible quand la fenêtre est au premier plan).

class Activity {
  visible = $state(typeof document === "undefined" ? true : !document.hidden);
  focused = $state(typeof document === "undefined" ? true : document.hasFocus());
  active = $derived(this.visible && this.focused);
  private started = false;

  start() {
    if (this.started || typeof document === "undefined") return;
    this.started = true;
    const sync = () => {
      this.visible = !document.hidden;
      this.focused = document.hasFocus();
      document.documentElement.dataset.bg = this.visible && this.focused ? "" : "1";
    };
    document.addEventListener("visibilitychange", sync);
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    sync();
  }
}

export const activity = new Activity();
