"""Fabrique un journal CS2 (.jsonl) réaliste pour tester Rewind avec `rewind.exe --replay-cs2`.

Scénario Wingman : round 1 = 2 kills (doit donner 0 clip avec le seuil de 3), avec un message
sans bloc « round » au milieu (le bug du 2026-11-13) ; round 2 = 3 kills (doit donner 1 clip « 3K »).
Usage : python fabriquer_partie_cs2.py sortie.jsonl
"""
import json, sys, time

ME = "76561198711869861"
out = open(sys.argv[1], "w", encoding="utf-8")
t0 = time.time() - 600
state = {"kills": 0, "round_kills": 0, "hs": 0, "ammo": 30, "round": 1, "phase": "live", "win": ""}


def msg(t, with_round=True):
    body = {
        "provider": {"steamid": ME, "appid": 730},
        "map": {"name": "de_inferno", "mode": "scrimcomp2v2", "phase": "live", "round": state["round"]},
        "player": {
            "steamid": ME, "team": "T",
            "state": {"health": 100, "round_kills": state["round_kills"], "round_killhs": state["hs"]},
            "match_stats": {"kills": state["kills"], "deaths": 0, "assists": 0},
            "weapons": {"weapon_0": {"name": "weapon_ak47", "type": "Rifle", "state": "active", "ammo_clip": state["ammo"]}},
        },
    }
    if with_round:
        body["round"] = {"phase": state["phase"], "win_team": state["win"]}
    out.write(json.dumps({"t": round(t, 3), "body": body}) + "\n")


def run(seconds, start, events):
    """events : {seconde: fonction} appliquées pendant le round, messages toutes les 0,1 s"""
    t = start
    end = start + seconds
    while t < end:
        for at, fn in list(events.items()):
            if abs((t - start) - at) < 0.05:
                fn()
        msg(t, with_round=not (abs((t - start) - 8.0) < 0.05))  # un message sans « round » à 8 s
        t += 0.1
    return t


def kill(hs=False, shots=3):
    def f():
        state["kills"] += 1
        state["round_kills"] += 1
        state["ammo"] -= shots
        if hs:
            state["hs"] += 1
    return f


def phase(p, win=""):
    def f():
        state["phase"] = p
        state["win"] = win
    return f


def next_round():
    state["round"] += 1
    state["round_kills"] = 0
    state["hs"] = 0
    state["ammo"] = 30
    state["phase"] = "freezetime"
    state["win"] = ""


t = run(45, t0, {5.0: kill(), 12.0: kill(hs=True), 40.0: phase("over", "T")})
next_round()
t = run(10, t, {9.9: phase("live")})
t = run(45, t, {4.0: kill(hs=True, shots=1), 9.0: kill(), 15.0: kill(hs=True), 40.0: phase("over", "T")})
next_round()
run(10, t, {})
out.close()
print("ok")
