"use client";
import { useEffect, useRef, useState } from "react";
import { Switch } from "../../components/Lyrics";
import { Button } from "../../components/ui";
import { FLAT_EFFECTS, playerEffectPresets, type AudioEffects, type EffectPreset } from "../../services/tauri/player";
import { FIELD, Inset, NoteRow, SettingRow } from "./rows";

const BANDS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
const db = (n: number) => `${n > 0 ? "+" : ""}${n} dB`;

/** The equaliser curve through the ten band gains, so a change is visible as well as audible. */
function Curve({ bands }: { bands: number[] }) {
  const pts = bands.map((g, i) => `${(i / 9) * 100},${20 - (g / 12) * 18}`);
  return (
    <svg role="img" aria-label="Equaliser curve" viewBox="0 0 100 40" preserveAspectRatio="none" className="h-16 w-full rounded-[8px] bg-frost-200">
      <line x1="0" x2="100" y1="20" y2="20" className="stroke-white/20" strokeWidth="0.4" />
      <polyline points={pts.join(" ")} fill="none" strokeWidth="1.2" strokeLinejoin="round" className="stroke-lumen-lavender" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function Slider({ label, value, min, max, step = 1, onChange, show }: { label: string; value: number; min: number; max: number; step?: number; onChange: (v: number) => void; show: (v: number) => string }) {
  return (
    <SettingRow label={label} htmlFor={`fx-${label}`}>
      <input id={`fx-${label}`} type="range" min={min} max={max} step={step} value={value} aria-valuetext={show(value)} onChange={(e) => onChange(Number(e.target.value))} className="w-[200px]" />
      <span className="w-16 text-right text-[13px] tabular-nums text-secondary-text">{show(value)}</span>
    </SettingRow>
  );
}

/** Settings > Audio > Sound shaping. Edits apply live: changes are sent a moment after the last touch. */
export function AudioEffectsPanel({ value, disabled, onChange }: { value: AudioEffects; disabled: boolean; onChange: (fx: AudioEffects) => void }) {
  const [draft, setDraft] = useState(value);
  // The presets come from core: the same list the macOS Sound menu uses.
  const [presets, setPresets] = useState<EffectPreset[]>([]);
  useEffect(() => { void playerEffectPresets().then(setPresets, () => {}); }, []);
  const groups = [...new Set(presets.map((p) => p.group))];
  const current = presets.find((p) => JSON.stringify(p.effects) === JSON.stringify({ ...draft, enabled: true }))?.id ?? "";
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => setDraft(value), [value]);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const set = (patch: Partial<AudioEffects>) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => onChange(next), 150);
  };
  const setBand = (i: number, g: number) => set({ bands: draft.bands.map((b, j) => (j === i ? g : b)) });
  const lock = disabled || !draft.enabled;
  return (
    <Inset id="settings-effects" title="Sound shaping">
      <SettingRow label="Use sound shaping" hint="Off plays the song untouched and keeps your settings.">
        <Switch on={draft.enabled} onChange={(on) => set({ enabled: on })} name="Use sound shaping">{draft.enabled ? "On" : "Off"}</Switch>
      </SettingRow>
      <SettingRow label="Preset" htmlFor="fx-preset" hint={`${presets.length} presets. Choosing one replaces your current settings.`}>
        <select id="fx-preset" className={FIELD} disabled={lock || presets.length === 0} value={current} onChange={(e) => { const p = presets.find((x) => x.id === e.target.value); if (p) set({ ...FLAT_EFFECTS, ...p.effects, enabled: true }); }}>
          <option value="">{current ? "" : "Custom"}</option>
          {groups.map((g) => <optgroup key={g} label={g}>{presets.filter((p) => p.group === g).map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}</optgroup>)}
        </select>
      </SettingRow>
      <div className="px-4 pt-1 pb-3">
        <Curve bands={draft.bands} />
        <div className="mt-2 grid grid-cols-10 gap-1">
          {draft.bands.map((g, i) => (
            <label key={BANDS[i]} className="flex flex-col items-center gap-1 text-[11px] text-secondary-text">
              <input type="range" min={-12} max={12} step={1} value={g} disabled={lock} aria-label={`${BANDS[i]} Hz`} aria-valuetext={db(g)} onChange={(e) => setBand(i, Number(e.target.value))} className="h-28 w-full [writing-mode:vertical-lr] [direction:rtl]" />
              <span className="tabular-nums">{BANDS[i]}</span>
              <span className="tabular-nums">{g}</span>
            </label>
          ))}
        </div>
      </div>
      <Slider label="Preamp" value={draft.preamp} min={-12} max={12} onChange={(v) => set({ preamp: v })} show={db} />
      <Slider label="Bass" value={draft.bass} min={-12} max={12} onChange={(v) => set({ bass: v })} show={db} />
      <Slider label="Treble" value={draft.treble} min={-12} max={12} onChange={(v) => set({ treble: v })} show={db} />
      <Slider label="Stereo width" value={draft.stereoWidth} min={0} max={200} step={5} onChange={(v) => set({ stereoWidth: v })} show={(v) => (v === 0 ? "Mono" : `${v}%`)} />
      <Slider label="Headphone crossfeed" value={draft.crossfeed} min={0} max={100} step={5} onChange={(v) => set({ crossfeed: v })} show={(v) => (v === 0 ? "Off" : `${v}%`)} />
      <Slider label="Speed" value={draft.speed} min={50} max={200} step={5} onChange={(v) => set({ speed: v })} show={(v) => `${(v / 100).toFixed(2)}×`} />
      <SettingRow label="Keep pitch" hint="On: speed changes the tempo only. Off: pitch follows speed, faster and higher for nightcore, slower and lower for slowed.">
        <Switch on={draft.keepPitch} onChange={(on) => set({ keepPitch: on })} name="Keep pitch">{draft.keepPitch ? "On" : "Off"}</Switch>
      </SettingRow>
      <SettingRow label="Moving sound" htmlFor="fx-spatial" hint="For headphones. The sound travels around your head. 8D is a slow circle; 4D is faster, wider and deeper.">
        <select id="fx-spatial" className={FIELD} disabled={lock} value={draft.spatial} onChange={(e) => set({ spatial: e.target.value as AudioEffects["spatial"] })}>
          <option value="off">Off</option><option value="8d">8D</option><option value="4d">4D</option>
        </select>
      </SettingRow>
      <SettingRow label="Room" htmlFor="fx-reverb">
        <select id="fx-reverb" className={FIELD} disabled={lock} value={draft.reverb} onChange={(e) => set({ reverb: e.target.value as AudioEffects["reverb"] })}>
          <option value="off">Off</option><option value="room">Small room</option><option value="hall">Hall</option>
        </select>
      </SettingRow>
      <SettingRow label="Compressor" htmlFor="fx-comp" hint="Narrows the gap between quiet and loud parts.">
        <select id="fx-comp" className={FIELD} disabled={lock} value={draft.compressor} onChange={(e) => set({ compressor: e.target.value as AudioEffects["compressor"] })}>
          <option value="off">Off</option><option value="light">Light</option><option value="medium">Medium</option><option value="heavy">Heavy</option>
        </select>
      </SettingRow>
      <SettingRow label="Reset">
        <Button disabled={disabled} onClick={() => set({ ...FLAT_EFFECTS })}>Reset sound shaping</Button>
      </SettingRow>
      <NoteRow tone="off">These filters run inside the player, so they work on every song. Boosts are followed by a limiter to avoid clipping. Saved on this device.</NoteRow>
    </Inset>
  );
}
