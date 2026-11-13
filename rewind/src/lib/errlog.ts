// Garde les erreurs de l'interface dans le journal de Rewind (rapport de diagnostic).
import { logFront } from "./api";

const text = (v: unknown): string => {
  if (v instanceof Error) return `${v.name}: ${v.message}${v.stack ? "\n" + v.stack.split("\n").slice(0, 4).join("\n") : ""}`;
  if (typeof v === "string") return v;
  try { return JSON.stringify(v) ?? String(v); } catch { return String(v); }
};

let installed = false;
export function installErrorLog() {
  if (installed || typeof window === "undefined" || (window as any).__REWIND_MOCK__) return;
  installed = true;
  window.addEventListener("error", (e) => {
    const where = e.filename ? ` (${e.filename.split("/").pop()}:${e.lineno}:${e.colno})` : "";
    logFront("error", `${e.message || "erreur"}${where}`);
  });
  window.addEventListener("unhandledrejection", (e) => logFront("error", `promesse rejetée : ${text(e.reason)}`));
  const original = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    original(...args);
    logFront("warn", `console.error : ${args.map(text).join(" ").slice(0, 600)}`);
  };
}
