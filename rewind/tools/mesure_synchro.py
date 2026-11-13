"""Mesure le décalage son/image d'un clip Rewind qui a filmé la mire FFmpeg « avsynctest ».

Usage : python mesure_synchro.py <clip.mp4> <ffmpeg.exe> [crop=w:h:x:y] [piste_audio]
Affiche, pour chaque flash, l'écart avec le bip le plus proche (positif = le son arrive après l'image).
"""
import re
import subprocess
import sys
import tempfile
import os


def run(cmd):
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)


def series(path, key):
    out = []
    t = None
    for line in open(path, encoding="utf-8", errors="ignore"):
        m = re.search(r"pts_time:([\d.]+)", line)
        if m:
            t = float(m.group(1))
            continue
        m = re.search(key + r"=(-?[\d.]+|-inf)", line)
        if m and t is not None:
            v = m.group(1)
            out.append((t, -120.0 if v == "-inf" else float(v)))
    return out


def onsets(values, threshold, min_gap):
    res, last = [], -1e9
    prev = None
    for t, v in values:
        if prev is not None and prev < threshold <= v and t - last > min_gap:
            res.append(t)
            last = t
        prev = v
    return res


def main():
    clip, ffmpeg = sys.argv[1], sys.argv[2]
    crop = sys.argv[3] if len(sys.argv) > 3 else "crop=900:506:100:100"
    track = sys.argv[4] if len(sys.argv) > 4 and not sys.argv[4].startswith("--") else "0"
    tmp = tempfile.mkdtemp()
    vf = os.path.join(tmp, "v.txt").replace("\\", "/").replace(":", "\\:")
    af = os.path.join(tmp, "a.txt").replace("\\", "/").replace(":", "\\:")
    run([ffmpeg, "-hide_banner", "-i", clip, "-map", "0:v:0", "-vf",
         f"{crop},signalstats,metadata=print:key=lavfi.signalstats.YAVG:file='{vf}'", "-f", "null", "-"])
    run([ffmpeg, "-hide_banner", "-i", clip, "-map", f"0:a:{track}", "-af",
         f"asetnsamples=240,astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level:file='{af}'",
         "-f", "null", "-"])
    v = series(os.path.join(tmp, "v.txt"), r"lavfi\.signalstats\.YAVG")
    a = series(os.path.join(tmp, "a.txt"), r"lavfi\.astats\.Overall\.RMS_level")
    if not v or not a:
        print("analyse impossible (pas de données)")
        return
    lo, hi = min(x for _, x in v), max(x for _, x in v)
    flashes = onsets(v, lo + (hi - lo) * 0.5, 0.5)
    beeps = onsets(a, -35.0, 0.5)
    print(f"luminance min {lo:.1f} max {hi:.1f} ; {len(flashes)} flashs, {len(beeps)} bips")
    diffs = []
    for f in flashes:
        if beeps:
            b = min(beeps, key=lambda x: abs(x - f))
            if abs(b - f) < 0.5:
                diffs.append(b - f)
                if "--detail" in sys.argv:
                    print(f"  t={f:6.2f}s  écart {round((b - f) * 1000):4d} ms")
    if diffs:
        diffs_ms = [round(d * 1000) for d in diffs]
        print("écarts (ms) :", diffs_ms)
        print(f"moyenne {sum(diffs)/len(diffs)*1000:.0f} ms")


main()
