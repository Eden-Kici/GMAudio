import { useState } from "react";
import { api, type AudioProfile } from "../api";
import { message, useStore } from "../store";
import { Segmented, Slider, Toggle, formatDb } from "./ui";

const FLAT: AudioProfile = { bass_db: 0, treble_db: 0, gain_db: 0, compress: 0, limit: true };

/** Ready-made shapes for common speakers. */
const PRESETS: { id: string; label: string; hint: string; profile: AudioProfile }[] = [
  { id: "flat", label: "As it is", hint: "No changes. Good for a proper sound system.", profile: FLAT },
  {
    id: "headphones",
    label: "Headphones",
    hint: "A touch softer on top, so long sessions do not tire the ears.",
    profile: { bass_db: 0, treble_db: -1.5, gain_db: 0, compress: 0.1, limit: true },
  },
  {
    id: "laptop",
    label: "Laptop speakers",
    hint: "Less deep bass (small speakers only rattle on it) and quiet passages brought up so they carry.",
    profile: { bass_db: -5, treble_db: 1, gain_db: 3, compress: 0.6, limit: true },
  },
  {
    id: "speaker",
    label: "Table speaker",
    hint: "For a Bluetooth or bookshelf speaker: a little more body and sparkle, and steadier volume across the room.",
    profile: { bass_db: 2, treble_db: 1.5, gain_db: 0, compress: 0.35, limit: true },
  },
];

const same = (a: AudioProfile, b: AudioProfile) =>
  a.bass_db === b.bass_db && a.treble_db === b.treble_db && a.gain_db === b.gain_db && Math.abs(a.compress - b.compress) < 0.005 && a.limit === b.limit;

/** Chooses how the mix is shaped for the speakers in use. */
export function ProfilePicker() {
  const profile = useStore((s) => s.engine?.profile ?? FLAT);
  const matching = PRESETS.find((p) => same(p.profile, profile));
  const [custom, setCustom] = useState(!matching);
  const current = custom || !matching ? "custom" : matching.id;
  const apply = (next: AudioProfile) => void api.setProfile(next).catch((e) => useStore.getState().notify(message(e)));

  return (
    <div className="space-y-3">
      <Segmented
        value={current}
        onChange={(id) => {
          const preset = PRESETS.find((p) => p.id === id);
          setCustom(!preset);
          if (preset) apply(preset.profile);
        }}
        options={[...PRESETS.map((p) => ({ value: p.id, label: p.label, title: p.hint })), { value: "custom", label: "Custom", title: "Set it by ear" }]}
      />
      {current !== "custom" ? (
        <p className="text-xs leading-relaxed text-faint">{matching?.hint}</p>
      ) : (
        <div className="space-y-3 rounded-lg bg-bg p-3">
          <Row label="Bass">
            <Slider value={profile.bass_db} min={-12} max={12} onChange={(bass_db) => apply({ ...profile, bass_db })} format={formatDb} label="Bass" />
          </Row>
          <Row label="Treble">
            <Slider value={profile.treble_db} min={-12} max={12} onChange={(treble_db) => apply({ ...profile, treble_db })} format={formatDb} label="Treble" />
          </Row>
          <Row label="Level">
            <Slider value={profile.gain_db} min={-12} max={12} onChange={(gain_db) => apply({ ...profile, gain_db })} format={formatDb} label="Overall level" />
          </Row>
          <Row label="Even out">
            <Slider
              value={Math.round(profile.compress * 100)}
              min={0}
              max={100}
              step={5}
              onChange={(v) => apply({ ...profile, compress: v / 100 })}
              format={(v) => (v === 0 ? "Off" : v < 35 ? "A little" : v < 70 ? "Clearly" : "A lot")}
              label="Bring quiet and loud passages closer together"
            />
          </Row>
          <Toggle
            on={profile.limit}
            onChange={(limit) => apply({ ...profile, limit })}
            label="Protect against distortion"
            hint="Catches the loudest peaks just before they would crackle. Best left on."
          />
        </div>
      )}
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-16 shrink-0 text-xs text-dim">{label}</span>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
