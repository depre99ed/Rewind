// Une icône et une couleur par type d'événement (kill, headshot, one tap, mort…).
// Ici la couleur SIGNALE : chaque action se reconnaît d'un coup d'œil sur la barre de lecture et dans la liste des moments.
import type { Component } from "svelte";
import { Crosshair, Target, Zap, Skull, Sword, Flame, ShieldCheck, Handshake, Bookmark, Layers2, Crown, Trophy, Circle } from "@lucide/svelte";

export type MomentStyle = { icon: Component<any>; color: string };

export const MOMENTS: Record<string, MomentStyle> = {
  kill: { icon: Crosshair, color: "#ff5d5d" },
  headshot: { icon: Target, color: "#ff8a3d" },
  onetap: { icon: Zap, color: "#ffc53d" },
  knife: { icon: Sword, color: "#b07cff" },
  zeus: { icon: Zap, color: "#38bdf8" },
  collateral: { icon: Layers2, color: "#f472b6" },
  "2k": { icon: Trophy, color: "#ffd25a" },
  "3k": { icon: Trophy, color: "#ffd25a" },
  "4k": { icon: Trophy, color: "#ffd25a" },
  ace: { icon: Crown, color: "#ffd25a" },
  clutch: { icon: ShieldCheck, color: "#3ddc97" },
  streak: { icon: Flame, color: "#ff7a45" },
  death: { icon: Skull, color: "#9aa3b5" },
  assist: { icon: Handshake, color: "#5ab0ff" },
  manual: { icon: Bookmark, color: "#eef0f4" },
};

export function momentStyle(kind: string): MomentStyle {
  return MOMENTS[kind] ?? { icon: Circle, color: "#9aa3b5" };
}
