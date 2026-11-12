#!/usr/bin/env python3
"""Synthesizes GMAudio's built-in starter sounds.

Everything is generated from scratch (no samples), so the pack is free to
ship. Output: src-tauri/starter-sounds/<Channel folder>/<Name>.ogg

    pip install numpy scipy soundfile
    python tools/make_starter_sounds.py

Music loops are bar-aligned at a known tempo, and "Battle March" is also
written as three equal-length stems, so they double as test material for
dynamic music.
"""
import math
import pathlib
import sys

import numpy as np
import soundfile as sf
from scipy import signal

SR = 44100
OUT = pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "starter-sounds"
rng = np.random.default_rng(20261112)


# ---- building blocks ---------------------------------------------------------

def secs(n):
    return int(round(n * SR))


def t_axis(dur):
    return np.arange(secs(dur)) / SR


def midi(note):
    return 440.0 * 2 ** ((note - 69) / 12)


def env_exp(dur, decay, attack=0.002):
    t = t_axis(dur)
    return np.minimum(t / attack, 1.0) * np.exp(-t / decay)


def env_adsr(dur, a, r):
    n = secs(dur)
    e = np.ones(n)
    na, nr = min(secs(a), n), min(secs(r), n)
    if na:
        e[:na] = np.linspace(0, 1, na) ** 2
    if nr:
        e[-nr:] *= np.linspace(1, 0, nr) ** 2
    return e


def lowpass(x, hz, order=2):
    sos = signal.butter(order, hz, "low", fs=SR, output="sos")
    return signal.sosfilt(sos, x, axis=0)


def highpass(x, hz, order=2):
    sos = signal.butter(order, hz, "high", fs=SR, output="sos")
    return signal.sosfilt(sos, x, axis=0)


def bandpass(x, lo, hi, order=2):
    sos = signal.butter(order, [lo, hi], "band", fs=SR, output="sos")
    return signal.sosfilt(sos, x, axis=0)


def noise(dur):
    return rng.standard_normal(secs(dur))


def pink(dur):
    """Noise with equal energy per octave."""
    n = secs(dur)
    spectrum = np.fft.rfft(rng.standard_normal(n))
    f = np.fft.rfftfreq(n, 1 / SR)
    f[0] = f[1]
    out = np.fft.irfft(spectrum / np.sqrt(f), n)
    return out / np.max(np.abs(out))


def brown(dur):
    out = np.cumsum(rng.standard_normal(secs(dur)))
    out = highpass(out, 20)
    return out / np.max(np.abs(out))


def saw(freq, dur, harmonics=14):
    t = t_axis(dur)
    out = np.zeros_like(t)
    for k in range(1, harmonics + 1):
        if freq * k > SR / 2.2:
            break
        out += np.sin(2 * math.pi * freq * k * t) / k
    return out * 0.6


def pluck(freq, dur, bright=0.5):
    """Karplus-Strong plucked string."""
    n = secs(dur)
    period = max(2, int(SR / freq))
    buf = rng.uniform(-1, 1, period)
    buf = lowpass(buf, 1500 + 6000 * bright)
    out = np.empty(n)
    for i in range(n):
        j = i % period
        out[i] = buf[j]
        buf[j] = 0.996 * 0.5 * (buf[j] + buf[(j + 1) % period])
    return out


def bell(freq, dur, decay=1.5):
    t = t_axis(dur)
    out = np.zeros_like(t)
    for ratio, amp, d in [(0.5, 0.5, 1.4), (1.0, 1.0, 1.0), (1.19, 0.6, 0.8), (1.56, 0.45, 0.6),
                          (2.0, 0.7, 0.5), (2.51, 0.35, 0.35), (2.66, 0.3, 0.3), (3.01, 0.25, 0.25),
                          (4.1, 0.2, 0.2)]:
        out += amp * np.sin(2 * math.pi * freq * ratio * t + rng.uniform(0, 6.28)) * np.exp(-t / (decay * d))
    return out * np.minimum(t / 0.002, 1.0) * 0.3


def pad(freqs, dur, attack=1.0, release=1.5, bright=900):
    out = np.zeros(secs(dur))
    for f in freqs:
        for detune in (-0.004, 0.0, 0.005):
            out += saw(f * (1 + detune), dur, 10)
    out = lowpass(out, bright, 2)
    return out * env_adsr(dur, attack, release) / (len(freqs) * 2.2)


def drum(f0, f1, dur, decay, noise_amt=0.0, noise_hz=3000):
    t = t_axis(dur)
    freq = f1 + (f0 - f1) * np.exp(-t / 0.03)
    body = np.sin(2 * math.pi * np.cumsum(freq) / SR) * np.exp(-t / decay)
    if noise_amt:
        body += noise_amt * lowpass(noise(dur), noise_hz) * np.exp(-t / (decay * 0.4))
    return body * np.minimum(t / 0.001, 1.0)


def hat(dur=0.06):
    return highpass(noise(dur), 7000) * env_exp(dur, dur / 4) * 0.35


def reverb(x, seconds=1.8, mix=0.25, loop=False):
    """Convolution with decaying noise. With `loop`, the tail wraps round."""
    n = secs(seconds)
    t = np.arange(n) / SR
    ir = rng.standard_normal(n) * np.exp(-t * 6.9 / seconds)
    ir = lowpass(ir, 5000)
    ir /= np.sqrt(np.sum(ir ** 2))
    wet = signal.fftconvolve(x, ir)
    if loop:
        body = wet[: len(x)].copy()
        tail = wet[len(x):]
        body[: len(tail)] += tail[: len(body)]
        wet = body
    else:
        x = np.concatenate([x, np.zeros(len(wet) - len(x))])
    return x * (1 - mix) + wet * mix


class Loop:
    """A mono buffer of fixed length; notes that run past the end wrap round."""

    def __init__(self, dur):
        self.buf = np.zeros(secs(dur))

    def add(self, at, sig, gain=1.0):
        n = len(self.buf)
        start = secs(at) % n
        idx = (start + np.arange(len(sig))) % n
        np.add.at(self.buf, idx, sig * gain)


def stereo(mono, width=0.0):
    """Mono to stereo. `width` adds a slightly delayed, filtered copy to one side."""
    if width <= 0:
        return np.stack([mono, mono], axis=1)
    d = secs(0.011)
    side = np.roll(mono, d) * width
    return np.stack([mono + side * 0.5, mono - side * 0.5], axis=1)


def wide_noise(make, dur):
    """Two independent noise beds, one per ear."""
    return np.stack([make(dur), make(dur)], axis=1)


def seamless(x, fade=1.5):
    """Trims `fade` seconds off the end and blends them into the start."""
    n = secs(fade)
    body, tail = x[:-n].copy(), x[-n:]
    ramp = np.linspace(0, 1, n)
    if x.ndim == 2:
        ramp = ramp[:, None]
    body[:n] = body[:n] * np.sqrt(ramp) + tail * np.sqrt(1 - ramp)
    return body


def normalize(x, peak_db=-3.0):
    return x / np.max(np.abs(x)) * 10 ** (peak_db / 20)


def save(folder, name, x, peak_db=-3.0):
    if x.ndim == 1:
        x = stereo(x)
    x = normalize(x, peak_db).astype(np.float32)
    assert np.isfinite(x).all(), name
    path = OUT / folder / f"{name}.ogg"
    path.parent.mkdir(parents=True, exist_ok=True)
    with sf.SoundFile(path, "w", SR, 2, format="OGG", subtype="VORBIS") as f:
        for i in range(0, len(x), SR):  # libsndfile dislikes huge single writes
            f.write(x[i:i + SR])
    print(f"  {folder}/{name}.ogg  {len(x) / SR:5.1f}s  {path.stat().st_size // 1024} KB")


# ---- music -------------------------------------------------------------------

def tavern_evening():
    bpm, bars = 100, 8
    beat = 60 / bpm
    loop = Loop(bars * 4 * beat)
    chords = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]] * 2  # Am F C G
    melody = [76, 74, 72, 69, 72, 74, 76, 79, 77, 76, 72, 69, 67, 69, 71, 72]
    for bar, chord in enumerate(chords):
        t0 = bar * 4 * beat
        pattern = [0, 1, 2, 1, 0, 2, 1, 2]
        for step, which in enumerate(pattern):
            note = chord[which] + (12 if step in (2, 5) else 0)
            loop.add(t0 + step * beat / 2, pluck(midi(note), 1.2, 0.55), 0.5)
        for b in (0, 2):
            loop.add(t0 + b * beat, pluck(midi(chord[0] - 12), 1.6, 0.2), 0.8)
        for b in range(4):
            loop.add(t0 + (b + 0.5) * beat, hat(0.05), 0.5)
        if bar >= 4:
            for i in range(4):
                note = melody[(bar - 4) * 4 + i]
                loop.add(t0 + i * beat, pluck(midi(note), 1.0, 0.9), 0.55)
    return stereo(reverb(loop.buf, 1.2, 0.18, loop=True), 0.35), bpm


def battle_march():
    """Returns (stems dict, bpm). All stems have identical length."""
    bpm, bars = 140, 16
    beat = 60 / bpm
    dur = bars * 4 * beat
    drums, strings, brass = Loop(dur), Loop(dur), Loop(dur)
    roots = [38, 38, 34, 36] * 4  # D D Bb C
    for bar, root in enumerate(roots):
        t0 = bar * 4 * beat
        for b in (0, 1.5, 2, 3):
            drums.add(t0 + b * beat, drum(110, 48, 0.6, 0.22, 0.5, 900), 1.0)
        for b in (1, 3):
            drums.add(t0 + b * beat, drum(210, 150, 0.25, 0.08, 1.4, 5000), 0.55)
        for step in range(8):
            drums.add(t0 + step * beat / 2, hat(0.04), 0.35 if step % 2 else 0.2)
        if bar % 4 == 3:
            for step in range(6):
                drums.add(t0 + (2.5 + step * 0.25) * beat, drum(160 - step * 8, 80, 0.3, 0.1, 0.4, 1500), 0.7)
        for step in range(8):
            note = root + (12 if step % 4 == 3 else 0) + (7 if step % 4 == 2 else 0)
            s = lowpass(saw(midi(note), beat * 0.42), 1400) * env_exp(beat * 0.42, 0.12, 0.004)
            strings.add(t0 + step * beat / 2, s, 0.9)
        if bar % 2 == 0:
            third = 3 if root != 34 and root != 36 else 4
            chord = [midi(root + 24), midi(root + 24 + third), midi(root + 31)]
            stab = lowpass(sum(saw(f, beat * 3.5) for f in chord), 2200)
            brass.add(t0, stab * env_adsr(beat * 3.5, 0.05, 0.6), 0.5)
    stems = {
        "Drums": stereo(reverb(drums.buf, 0.8, 0.12, loop=True), 0.2),
        "Strings": stereo(reverb(strings.buf, 1.2, 0.2, loop=True), 0.4),
        "Brass": stereo(reverb(brass.buf, 1.6, 0.3, loop=True), 0.5),
    }
    return stems, bpm


def quiet_road():
    bpm, bars = 72, 8
    beat = 60 / bpm
    loop = Loop(bars * 4 * beat)
    chords = [[48, 55, 59, 64], [45, 52, 55, 60], [41, 48, 52, 57], [43, 50, 55, 59]] * 2
    tune = [76, None, 79, 76, 72, None, 74, None, 72, None, 69, 72, 71, None, 67, None]
    for bar, chord in enumerate(chords):
        t0 = bar * 4 * beat
        loop.add(t0, pad([midi(n) for n in chord], 4 * beat + 1.2, 1.2, 1.6, 1100), 0.9)
        for i in range(2):
            note = tune[(bar * 2 + i) % len(tune)]
            if note:
                loop.add(t0 + i * 2 * beat, bell(midi(note), 3.0, 1.1), 0.5)
    return stereo(reverb(loop.buf, 2.4, 0.35, loop=True), 0.5), bpm


def dungeon_depths():
    bpm, bars = 60, 8
    beat = 60 / bpm
    dur = bars * 4 * beat
    loop = Loop(dur)
    t = t_axis(dur)
    drone = sum(np.sin(2 * math.pi * f * t + p) * a for f, a, p in
                [(36.71, 1.0, 0), (36.95, 0.8, 1), (73.42, 0.5, 2), (73.9, 0.4, 3), (110.3, 0.25, 4)])
    # Whole numbers of cycles keep the loop seamless.
    swell = 0.75 + 0.25 * np.sin(2 * math.pi * t * 2 / dur)
    loop.add(0, drone * swell, 0.22)
    for bar in range(bars):
        t0 = bar * 4 * beat
        if bar % 2 == 0:
            loop.add(t0, pad([midi(n) for n in (50, 51, 57)], 8 * beat, 3.0, 4.0, 500), 0.5)
        if bar in (1, 5):
            loop.add(t0 + beat, bell(midi(38), 6.0, 2.5), 0.6)
        if bar in (3, 6):
            loop.add(t0 + 2 * beat, bell(midi(44), 5.0, 2.0), 0.35)
    rumble = lowpass(noise(dur), 90) * 0.5
    loop.add(0, seamless(np.concatenate([rumble, rumble[: secs(1.5)]]), 1.5), 0.6)
    return stereo(reverb(loop.buf, 3.5, 0.4, loop=True), 0.6), bpm


# ---- ambience ----------------------------------------------------------------

AMB = 24.0  # seconds, before the 1.5 s loop blend


def rain():
    bed = wide_noise(lambda d: highpass(pink(d), 700), AMB) * 0.5
    drops = wide_noise(lambda d: bandpass(noise(d), 2500, 9000), AMB)
    gate = (rng.random(drops.shape) > 0.9985).astype(float)
    gate = signal.lfilter([1], [1, -0.995], gate, axis=0)
    return seamless(bed + drops * np.minimum(gate, 4) * 0.12)


def wind():
    out = np.zeros((secs(AMB), 2))
    t = t_axis(AMB)
    for lo, hi, rate in [(120, 420, 0.07), (300, 900, 0.11), (700, 1800, 0.17)]:
        layer = wide_noise(lambda d: bandpass(noise(d), lo, hi), AMB)
        lfo = 0.55 + 0.45 * np.sin(2 * math.pi * rate * t + rng.uniform(0, 6.28))
        gust = 0.6 + 0.4 * np.sin(2 * math.pi * rate * 0.37 * t + rng.uniform(0, 6.28))
        out += layer * (lfo * gust)[:, None] ** 2
    return seamless(out)


def campfire():
    bed = wide_noise(lambda d: lowpass(brown(d), 400), AMB) * 0.9
    crackle = np.zeros((secs(AMB), 2))
    for _ in range(int(AMB * 14)):
        at = rng.integers(0, secs(AMB) - secs(0.05))
        length = rng.uniform(0.004, 0.03)
        pop = bandpass(noise(length), 900, rng.uniform(3000, 9000)) * env_exp(length, length / 3, 0.0005)
        pan = rng.uniform(0.2, 0.8)
        amp = rng.uniform(0.15, 1.0) ** 2
        crackle[at:at + len(pop), 0] += pop * amp * (1 - pan)
        crackle[at:at + len(pop), 1] += pop * amp * pan
    return seamless(bed + crackle * 1.4)


def night_crickets():
    out = wide_noise(lambda d: bandpass(pink(d), 200, 2000), AMB) * 0.08
    t = t_axis(AMB)
    for _ in range(7):
        freq = rng.uniform(3900, 5200)
        rate = rng.uniform(11, 17)       # pulses per second inside a chirp
        every = rng.uniform(0.7, 1.6)    # chirps repeat this often
        phase = rng.uniform(0, every)
        chirp_on = ((t + phase) % every) < every * 0.45
        pulses = np.clip(np.sin(2 * math.pi * rate * t), 0, 1) ** 2
        tone = np.sin(2 * math.pi * freq * t) * pulses * chirp_on
        pan = rng.uniform(0.1, 0.9)
        amp = rng.uniform(0.05, 0.16)
        out[:, 0] += tone * amp * (1 - pan)
        out[:, 1] += tone * amp * pan
    return seamless(out)


def cave_drips():
    out = np.zeros(secs(AMB))
    for _ in range(int(AMB * 0.9)):
        at = rng.integers(0, secs(AMB) - secs(0.4))
        f = rng.uniform(900, 2200)
        t = t_axis(0.3)
        drop = np.sin(2 * math.pi * (f + 900 * np.exp(-t / 0.012)) * t) * np.exp(-t / 0.05)
        out[at:at + len(drop)] += drop * rng.uniform(0.3, 1.0)
    wet = reverb(out, 3.0, 0.6, loop=True)
    rumble = lowpass(noise(AMB), 120) * 0.25
    return seamless(stereo(wet + rumble, 0.7))


def river():
    out = wide_noise(lambda d: bandpass(pink(d), 300, 5000), AMB)
    burble = wide_noise(lambda d: lowpass(np.abs(noise(d)), 9), AMB)
    burble = 0.6 + 0.4 * burble / np.max(burble)
    return seamless(out * burble)


# ---- sound effects -----------------------------------------------------------

def sword_clash():
    dur = 1.2
    t = t_axis(dur)
    ring = sum(np.sin(2 * math.pi * f * t + rng.uniform(0, 6)) * np.exp(-t / d) * a for f, d, a in
               [(1870, 0.35, 1), (2930, 0.28, 0.8), (4410, 0.2, 0.7), (6120, 0.14, 0.5), (7790, 0.1, 0.4), (910, 0.2, 0.3)])
    hit = highpass(noise(0.03), 2000) * env_exp(0.03, 0.006, 0.0003) * 3
    ring[: len(hit)] += hit
    return reverb(ring, 0.6, 0.12)


def door_knock():
    out = np.zeros(secs(1.0))
    for at in (0.0, 0.22, 0.44):
        k = drum(190, 120, 0.15, 0.035, 0.8, 1800)
        i = secs(at)
        out[i:i + len(k)] += k
    return reverb(out, 0.5, 0.2)


def coin_drop():
    out = np.zeros(secs(1.6))
    at, gap, amp = 0.0, 0.26, 1.0
    while gap > 0.02:
        ping = bell(rng.uniform(3200, 3600), 0.5, 0.12)
        i = secs(at)
        out[i:i + len(ping)] += ping[: len(out) - i] * amp
        at += gap
        gap *= 0.62
        amp *= 0.75
    return out


def thunder():
    dur = 7.0
    t = t_axis(dur)
    crack = lowpass(noise(dur), 1800) * np.exp(-t / 0.25) * 1.5
    roll = lowpass(noise(dur), 160) * np.exp(-t / 2.4)
    shake = 0.5 + 0.5 * np.abs(lowpass(noise(dur), 4))
    shake /= np.max(shake)
    return stereo(reverb((crack + roll * 2.5) * shake, 2.5, 0.35), 0.6)


def fireball():
    dur = 2.2
    t = t_axis(dur)
    n = noise(dur)
    # Sweep a band-pass by mixing a few fixed bands with moving weights.
    centers = [300, 600, 1200, 2400, 4800]
    sweep = 1 - np.abs(t / dur * 2 - 0.9)
    whoosh = np.zeros_like(t)
    for i, c in enumerate(centers):
        weight = np.exp(-((sweep * (len(centers) - 1) - i) ** 2) / 0.5)
        whoosh += bandpass(n, c * 0.7, c * 1.4) * weight
    whoosh *= np.sin(math.pi * t / dur) ** 2
    boom = drum(90, 35, dur, 0.5, 0.6, 500) * 1.2
    boom = np.roll(boom, secs(1.0)) * (t > 1.0)
    return stereo(reverb(whoosh + boom, 1.2, 0.2), 0.5)


def bell_toll():
    return stereo(reverb(bell(midi(50), 6.0, 3.0), 2.5, 0.3), 0.4)


def arrow_shot():
    dur = 0.9
    t = t_axis(dur)
    whoosh = bandpass(noise(dur), 1500, 6000) * np.exp(-((t - 0.12) / 0.07) ** 2)
    thunk = drum(240, 90, 0.2, 0.04, 1.0, 2500)
    out = whoosh.copy()
    i = secs(0.38)
    out[i:i + len(thunk)] += thunk * 1.3
    return reverb(out, 0.4, 0.12)


def heavy_impact():
    hit = drum(120, 28, 2.0, 0.55, 1.2, 700)
    debris = bandpass(noise(2.0), 400, 3000) * env_exp(2.0, 0.25, 0.01) * 0.3
    return stereo(reverb(hit + debris, 1.5, 0.25), 0.4)


def magic_chime():
    out = np.zeros(secs(3.0))
    for i, note in enumerate([72, 76, 79, 84, 88, 91, 96]):
        ping = bell(midi(note), 2.0, 0.7)
        at = secs(i * 0.09)
        out[at:at + len(ping)] += ping[: len(out) - at] * (0.9 - i * 0.05)
    return stereo(reverb(out, 2.5, 0.4), 0.7)


def heartbeat():
    out = np.zeros(secs(2.0))
    for at, amp in ((0.0, 1.0), (0.28, 0.7), (1.0, 1.0), (1.28, 0.7)):
        k = drum(70, 38, 0.3, 0.09)
        i = secs(at)
        out[i:i + len(k)] += k * amp
    return lowpass(out, 200)


# ---- stingers ----------------------------------------------------------------

def brass_note(note, dur):
    return lowpass(saw(midi(note), dur) + saw(midi(note) * 1.003, dur), 2600) * env_adsr(dur, 0.03, 0.15)


def victory_fanfare():
    out = np.zeros(secs(4.2))
    for at, note, dur in [(0.0, 60, 0.2), (0.2, 64, 0.2), (0.4, 67, 0.2), (0.6, 72, 0.5), (1.15, 67, 0.2), (1.35, 72, 2.2)]:
        n = brass_note(note, dur)
        i = secs(at)
        out[i:i + len(n)] += n
    for note in (64, 67, 76):
        n = brass_note(note, 2.2) * 0.6
        i = secs(1.35)
        out[i:i + len(n)] += n
    roll = drum(160, 90, 0.4, 0.15, 0.6, 1500)
    i = secs(1.35)
    out[i:i + len(roll)] += roll * 1.5
    return stereo(reverb(out, 1.8, 0.25), 0.4)


def ominous_reveal():
    dur = 6.0
    swell = pad([midi(n) for n in (38, 39, 44, 50)], dur, 2.5, 2.5, 700) * 2
    hit = drum(80, 30, 3.0, 0.9, 0.8, 400)
    i = secs(2.4)
    swell[i:i + len(hit)] += hit * 1.4
    low_bell = bell(midi(38), 3.5, 2.0)
    swell[i:i + len(low_bell)] += low_bell
    return stereo(reverb(swell, 3.0, 0.35), 0.6)


def critical_hit():
    dur = 1.6
    t = t_axis(dur)
    hit = drum(200, 50, dur, 0.18, 1.5, 4000)
    rise = np.sin(2 * math.pi * np.cumsum(500 + 2500 * (t / 0.35).clip(0, 1) ** 2) / SR) * np.exp(-t / 0.3) * 0.5
    ring = bell(midi(86), dur, 0.5)
    return stereo(reverb(hit + rise + ring, 0.9, 0.2), 0.4)


def mystery_chime():
    out = np.zeros(secs(4.5))
    for i, note in enumerate([62, 64, 66, 68, 70, 72, 74, 76, 78]):
        p = pluck(midi(note), 2.5, 0.8)
        at = secs(i * 0.11)
        out[at:at + len(p)] += p[: len(out) - at] * 0.7
    return stereo(reverb(out, 3.0, 0.45), 0.7)


def defeat():
    out = np.zeros(secs(4.5))
    for at, note, dur in [(0.0, 67, 0.45), (0.5, 63, 0.45), (1.0, 60, 0.45), (1.5, 55, 2.4)]:
        n = brass_note(note, dur) * 0.9
        i = secs(at)
        out[i:i + len(n)] += n
    low = brass_note(43, 2.4) * 0.8
    i = secs(1.5)
    out[i:i + len(low)] += low
    return stereo(reverb(lowpass(out, 1800), 2.2, 0.3), 0.4)


# ---- main --------------------------------------------------------------------

def main():
    print(f"Writing starter sounds to {OUT}")
    tempos = {}

    track, tempos["Tavern Evening"] = tavern_evening()
    save("Music", "Tavern Evening", track)
    track, tempos["Quiet Road"] = quiet_road()
    save("Music", "Quiet Road", track)
    track, tempos["Dungeon Depths"] = dungeon_depths()
    save("Music", "Dungeon Depths", track)

    stems, tempos["Battle March"] = battle_march()
    full = sum(stems.values())
    scale = 10 ** (-3 / 20) / np.max(np.abs(full))  # one gain for all, so stems sum to the mix
    save("Music", "Battle March", full)
    for name, stem in stems.items():
        x = (stem * scale).astype(np.float32)
        path = OUT / "Music" / "Battle March (stems)" / f"Battle March - {name}.ogg"
        path.parent.mkdir(parents=True, exist_ok=True)
        with sf.SoundFile(path, "w", SR, 2, format="OGG", subtype="VORBIS") as f:
            for i in range(0, len(x), SR):
                f.write(x[i:i + SR])
        print(f"  {path.relative_to(OUT)}  {len(x) / SR:5.1f}s")

    for name, make in [("Rain", rain), ("Wind", wind), ("Campfire", campfire),
                       ("Night Crickets", night_crickets), ("Cave Drips", cave_drips), ("River", river)]:
        save("Ambience", name, make(), -6.0)

    for name, make in [("Sword Clash", sword_clash), ("Door Knock", door_knock), ("Coin Drop", coin_drop),
                       ("Thunder", thunder), ("Fireball", fireball), ("Bell Toll", bell_toll),
                       ("Arrow Shot", arrow_shot), ("Heavy Impact", heavy_impact),
                       ("Magic Chime", magic_chime), ("Heartbeat", heartbeat)]:
        save("Sound effects", name, make())

    for name, make in [("Victory Fanfare", victory_fanfare), ("Ominous Reveal", ominous_reveal),
                       ("Critical Hit", critical_hit), ("Mystery Chime", mystery_chime), ("Defeat", defeat)]:
        save("Stingers", name, make())

    lines = [f"{name}: {bpm} BPM, 4/4" for name, bpm in sorted(tempos.items())]
    (OUT / "Music" / "tempos.txt").write_text("\n".join(lines) + "\n")
    total = sum(p.stat().st_size for p in OUT.rglob("*.ogg"))
    print(f"{len(list(OUT.rglob('*.ogg')))} files, {total / 1e6:.1f} MB")


if __name__ == "__main__":
    sys.exit(main())
