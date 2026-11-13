import { useState } from "react";
import { api, CHANNELS, type Asset } from "../api";
import { message, useStore } from "../store";
import { CheckIcon } from "./icons";
import { buttonClass, Field, inputBase, inputClass, Modal, primaryClass } from "./ui";

/** Saves the audio of a YouTube video into the library. */
export function YouTubeImport({ onClose }: { onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [addAs, setAddAs] = useState("Music");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [added, setAdded] = useState<Asset[]>([]);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const asset = await api.youtubeImport(url, addAs || null);
      await useStore.getState().refresh();
      if (asset) setAdded((list) => [asset, ...list]);
      setUrl("");
    } catch (e) {
      setError(message(e));
    }
    setBusy(false);
  };

  return (
    <Modal title="Add audio from YouTube" onClose={onClose}>
      <form
        className="space-y-4 p-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy && url.trim()) void run();
        }}
      >
        <Field label="Link to the video" hint="One video at a time">
          <input
            autoFocus
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://www.youtube.com/watch?v=…"
            spellCheck={false}
            className={inputClass}
          />
        </Field>
        <div className="flex items-center gap-3">
          <label className="flex items-center gap-2 text-xs text-dim">
            Add as
            <select value={addAs} onChange={(e) => setAddAs(e.target.value)} className={`${inputBase} h-9`}>
              {CHANNELS.map((c) => (
                <option key={c.id} value={c.tag}>
                  {c.label}
                </option>
              ))}
              <option value="">No tag</option>
            </select>
          </label>
          <button type="submit" disabled={busy || !url.trim()} className={`${primaryClass} ml-auto`}>
            {busy ? "Downloading…" : "Add to library"}
          </button>
        </div>
        {busy && (
          <p className="text-xs leading-relaxed text-faint">
            This takes a moment. The first time, GMAudio also fetches a small helper program (yt-dlp).
          </p>
        )}
        {error && <p className="rounded-md bg-danger/10 px-3 py-2 text-xs leading-relaxed text-danger">{error}</p>}
        {added.length > 0 && (
          <ul className="space-y-1">
            {added.map((asset) => (
              <li key={asset.id} className="flex items-center gap-2 text-xs text-dim">
                <CheckIcon width={12} height={12} className="shrink-0 text-ambience" />
                <span className="min-w-0 flex-1 truncate">{asset.name}</span>
                <button
                  type="button"
                  onClick={() => void useStore.getState().play(asset.id, CHANNELS.find((c) => c.tag === addAs)?.id ?? "music")}
                  className="font-medium text-dim underline-offset-2 hover:text-ink hover:underline"
                >
                  Play
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className="border-t border-line pt-3 text-xs leading-relaxed text-faint">
          The sound is saved as an M4A file in the YouTube folder of your library. Use this for audio you are allowed
          to download; YouTube's terms do not permit it for most videos.
        </p>
        <div className="flex justify-end">
          <button type="button" onClick={onClose} className={buttonClass}>
            Close
          </button>
        </div>
      </form>
    </Modal>
  );
}
