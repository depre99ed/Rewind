"""Fabrique un journal CS2 compétitif (.jsonl) pour tester les clips automatiques « CS2 normal ».

Rounds (réglages par défaut attendus) :
  1. un one tap seul            -> 1 clip « One Tap »
  2. 2 kills en rafale          -> 0 clip (seuil 3 kills, 2K et headshot décochés)
  3. 3 kills dont 2 headshots   -> 1 clip « 3K » (un seul, pas un par kill)
  4. aucun kill                 -> 0 clip
  5. one tap + 2 kills (=3K)    -> 1 clip
Usage : python fabriquer_partie_competitive.py sortie.jsonl
"""
import json, sys, time

ME = "76561198711869861"
out = open(sys.argv[1], "w", encoding="utf-8")
t = time.time() - 900
st = {"kills": 0, "rk": 0, "hs": 0, "ammo": 30, "round": 1, "phase": "live", "win": ""}


def msg():
    body = {
        "provider": {"steamid": ME, "appid": 730},
        "map": {"name": "de_mirage", "mode": "competitive", "phase": "live", "round": st["round"]},
        "round": {"phase": st["phase"], "win_team": st["win"]},
        "player": {
            "steamid": ME, "team": "T",
            "state": {"health": 100, "round_kills": st["rk"], "round_killhs": st["hs"]},
            "match_stats": {"kills": st["kills"], "deaths": 0, "assists": 0},
            "weapons": {"weapon_0": {"name": "weapon_ak47", "type": "Rifle", "state": "active", "ammo_clip": st["ammo"]}},
        },
    }
    out.write(json.dumps({"t": round(t, 3), "body": body}) + "\n")


def wait(seconds):
    global t
    end = t + seconds
    while t < end:
        msg()
        t += 0.1


def kill(hs=False, bullets=4):
    st["ammo"] -= bullets
    st["kills"] += 1
    st["rk"] += 1
    if hs:
        st["hs"] += 1
    msg()


def new_round():
    st.update(rk=0, hs=0, ammo=30, phase="freezetime", win="")
    wait(3)
    st["phase"] = "live"
    wait(2)


def end_round():
    st.update(phase="over", win="T")
    wait(4)
    st["round"] += 1


wait(2)
# 1 : one tap seul
kill(hs=True, bullets=1); wait(15); end_round(); new_round()
# 2 : 2 kills en rafale
kill(bullets=5); wait(3); kill(bullets=6); wait(10); end_round(); new_round()
# 3 : 3 kills dont 2 headshots en rafale
kill(hs=True, bullets=4); wait(4); kill(bullets=5); wait(5); kill(hs=True, bullets=3); wait(10); end_round(); new_round()
# 4 : rien
wait(20); end_round(); new_round()
# 5 : one tap puis 2 kills
kill(hs=True, bullets=1); wait(6); kill(bullets=5); wait(3); kill(bullets=4); wait(10); end_round()
out.close()
print("ok")
