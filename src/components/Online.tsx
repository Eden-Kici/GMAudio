import { useEffect, useRef, useState } from "react";
import { api, CHANNELS, type FreesoundSound, type FreesoundStatus } from "../api";
import { formatTime } from "../format";
import { message, useStore } from "../store";
import { CheckIcon, PlayIcon, SearchIcon, StopIcon } from "./icons";
import { buttonClass, Field, inputBase, inputClass, Modal, primaryClass } from "./ui";

const notify = (e: unknown) => useStore.getState().notify(message(e));
const openPage = (url: string) => void api.freesoundOpenPage(url).catch(notify);

/** Search Freesound and download straight into the library. */
export function OnlineSounds({ onClose, startWithKey }: { onClose: () => void; startWithKey?: boolean }) {
  const [status, setStatus] = useState<FreesoundStatus | null>(null);
  const [editingKey, setEditingKey] = useState(!!startWithKey);
  useEffect(() => {
    void api.freesoundStatus().then(setStatus).catch(notify);
  }, []);

  return (
    <Modal title="Find sounds online (Freesound)" onClose={onClose} wide>
      {status === null ? (
        <p className="p-6 text-sm text-dim">One moment…</p>
      ) : !status.hasKey || editingKey ? (
        <KeyForm
          status={status}
          onSaved={(next) => {
            setStatus(next);
            setEditingKey(false);
          }}
          onCancel={status.hasKey ? () => setEditingKey(false) : undefined}
        />
      ) : (
        <Search status={status} setStatus={setStatus} onEditKey={() => setEditingKey(true)} />
      )}
    </Modal>
  );
}

function KeyForm({ status, onSaved, onCancel }: { status: FreesoundStatus; onSaved: (status: FreesoundStatus) => void; onCancel?: () => void }) {
  const [clientId, setClientId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const save = () => void api.freesoundSetCredentials(clientId, apiKey).then(onSaved).catch(notify);
  const step = "flex gap-3 text-sm leading-relaxed text-dim";
  const number = "flex size-6 shrink-0 items-center justify-center rounded-full bg-raised text-xs font-semibold text-ink";
  return (
    <div className="space-y-4 overflow-y-auto p-5">
      <p className="text-sm leading-relaxed text-dim">
        Freesound is a free library of several hundred thousand sounds shared by people all over the world. GMAudio can
        search it and put sounds straight into your library. Freesound asks every app for a key. It is free and takes
        a couple of minutes:
      </p>
      <ol className="space-y-3">
        <li className={step}>
          <span className={number}>1</span>
          <span>
            Make a Freesound account, or log in.{" "}
            <button onClick={() => openPage("https://freesound.org/home/login/")} className="font-medium text-accent hover:underline">
              Open Freesound
            </button>
          </span>
        </li>
        <li className={step}>
          <span className={number}>2</span>
          <span>
            Ask for a key. Name it GMAudio; the description can be “personal use”. Leave the callback address as it
            is.{" "}
            <button onClick={() => openPage("https://freesound.org/apiv2/apply")} className="font-medium text-accent hover:underline">
              Open the key page
            </button>
          </span>
        </li>
        <li className={step}>
          <span className={number}>3</span>
          <span>Copy the two values Freesound shows you into these boxes.</span>
        </li>
      </ol>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Client id">
          <input value={clientId} onChange={(e) => setClientId(e.target.value)} spellCheck={false} autoComplete="off" className={inputClass} />
        </Field>
        <Field label="Client secret / API key">
          <input value={apiKey} onChange={(e) => setApiKey(e.target.value)} spellCheck={false} autoComplete="off" type="password" className={inputClass} />
        </Field>
      </div>
      <p className="text-xs text-faint">
        {status.hasKey
          ? "A key is saved on this computer and is not shown again. Fill in a box only to replace that value; an empty box keeps what is saved."
          : "The key is saved on this computer, so you only do this once."}
      </p>
      <div className="flex items-center justify-end gap-2">
        {status.hasKey && (
          <button
            onClick={() => void api.freesoundRemoveKey().then(onSaved).catch(notify)}
            className="mr-auto text-xs font-medium text-dim underline-offset-2 hover:text-danger hover:underline"
          >
            Remove the saved key
          </button>
        )}
        {onCancel && (
          <button onClick={onCancel} className={buttonClass}>
            Cancel
          </button>
        )}
        <button onClick={save} disabled={status.hasKey ? !apiKey.trim() && !clientId.trim() : !apiKey.trim()} className={primaryClass}>
          Save
        </button>
      </div>
    </div>
  );
}

type AddState = "busy" | "done" | "copy" | "failed";

function Search({ status, setStatus, onEditKey }: { status: FreesoundStatus; setStatus: (s: FreesoundStatus) => void; onEditKey: () => void }) {
  const [query, setQuery] = useState("");
  const [asked, setAsked] = useState("");
  const [page, setPage] = useState(1);
  const [sounds, setSounds] = useState<FreesoundSound[]>([]);
  const [count, setCount] = useState<number | null>(null);
  const [more, setMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [addAs, setAddAs] = useState("Sound effects");
  const [added, setAdded] = useState<Record<number, AddState>>({});
  const [previewing, setPreviewing] = useState<number | null>(null);
  const player = useRef<HTMLAudioElement | null>(null);

  useEffect(() => () => player.current?.pause(), []);

  const run = async (text: string, nextPage: number) => {
    if (!text.trim()) return;
    setBusy(true);
    try {
      const found = await api.freesoundSearch(text.trim(), nextPage);
      setSounds(nextPage === 1 ? found.sounds : [...sounds, ...found.sounds]);
      setCount(found.count);
      setMore(found.has_more);
      setAsked(text.trim());
      setPage(nextPage);
    } catch (e) {
      notify(e);
    }
    setBusy(false);
  };

  const preview = (sound: FreesoundSound) => {
    player.current?.pause();
    if (previewing === sound.id) {
      setPreviewing(null);
      return;
    }
    const audio = new Audio(sound.preview_url);
    audio.onended = () => setPreviewing((id) => (id === sound.id ? null : id));
    audio.onerror = () => {
      setPreviewing(null);
      useStore.getState().notify("That preview could not be played.");
    };
    player.current = audio;
    setPreviewing(sound.id);
    void audio.play().catch(() => setPreviewing(null));
  };

  const add = async (sound: FreesoundSound) => {
    setAdded((a) => ({ ...a, [sound.id]: "busy" }));
    try {
      const result = await api.freesoundDownload(sound, addAs || null);
      setAdded((a) => ({ ...a, [sound.id]: result.original || !status.signedIn ? "done" : "copy" }));
      await useStore.getState().refresh();
    } catch (e) {
      setAdded((a) => ({ ...a, [sound.id]: "failed" }));
      notify(e);
    }
  };

  return (
    <>
      <form
        className="flex items-center gap-2 border-b border-line p-3"
        onSubmit={(e) => {
          e.preventDefault();
          void run(query, 1);
        }}
      >
        <label className="relative min-w-0 flex-1">
          <SearchIcon className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="door creak, tavern crowd, dragon roar…"
            spellCheck={false}
            className={`${inputBase} h-10 w-full pl-9`}
          />
        </label>
        <button type="submit" disabled={busy || !query.trim()} className={primaryClass}>
          {busy && page === 1 ? "Searching…" : "Search"}
        </button>
        <label className="flex shrink-0 items-center gap-2 text-xs text-dim">
          Add as
          <select value={addAs} onChange={(e) => setAddAs(e.target.value)} className={`${inputBase} h-9`} title="The channel new downloads are tagged for">
            {CHANNELS.map((c) => (
              <option key={c.id} value={c.tag}>
                {c.label}
              </option>
            ))}
            <option value="">No tag</option>
          </select>
        </label>
      </form>

      <div className="min-h-64 flex-1 overflow-y-auto p-2">
        {count === null ? (
          <p className="px-4 py-10 text-center text-sm leading-relaxed text-dim">
            Type what you are looking for. Listen first, then add the ones you like.
            <br />
            English words find the most.
          </p>
        ) : sounds.length === 0 ? (
          <p className="px-4 py-10 text-center text-sm text-dim">Nothing found for “{asked}”. Try other words.</p>
        ) : (
          <>
            {sounds.map((sound) => {
              const state = added[sound.id];
              return (
                <div key={sound.id} className="flex min-h-12 items-center gap-2.5 rounded-lg px-2 hover:bg-raised/50">
                  <button
                    onClick={() => preview(sound)}
                    aria-label={previewing === sound.id ? `Stop ${sound.name}` : `Listen to ${sound.name}`}
                    title="Listen. Plays on this computer's own speakers, not through the mixer."
                    className={`flex size-8 shrink-0 items-center justify-center rounded-full ${previewing === sound.id ? "bg-accent text-bg" : "bg-raised text-ink hover:bg-line"}`}
                  >
                    {previewing === sound.id ? <StopIcon width={12} height={12} /> : <PlayIcon width={12} height={12} />}
                  </button>
                  <div className="min-w-0 flex-1 py-1.5">
                    <button onClick={() => openPage(`https://freesound.org/s/${sound.id}/`)} title="Open this sound's page on Freesound" className="block max-w-full truncate text-left text-[13.5px] font-medium hover:underline">
                      {sound.name}
                    </button>
                    <span className="flex items-center gap-1.5 text-xs text-faint">
                      <span className="truncate">by {sound.username}</span>
                      <span className="shrink-0 rounded bg-raised px-1.5 py-px text-[11px] text-dim" title={licenseHint(sound.license)}>
                        {sound.license}
                      </span>
                      <span className="truncate">{sound.tags.slice(0, 4).join(", ")}</span>
                    </span>
                  </div>
                  <span className="w-10 shrink-0 text-right text-xs tabular-nums text-dim">{formatTime(sound.duration * 1000)}</span>
                  <button
                    onClick={() => void add(sound)}
                    disabled={state === "busy" || state === "done" || state === "copy"}
                    title={state === "copy" ? "The original could not be fetched, so the compressed copy was added" : undefined}
                    className={`flex h-8 w-24 shrink-0 items-center justify-center gap-1 rounded-md text-xs font-medium ${
                      state === "done" || state === "copy" ? "text-ambience" : "bg-raised hover:bg-line disabled:opacity-60"
                    }`}
                  >
                    {state === "busy" ? "Adding…" : state === "done" || state === "copy" ? (
                      <>
                        <CheckIcon width={12} height={12} /> Added
                      </>
                    ) : state === "failed" ? "Try again" : "Add"}
                  </button>
                </div>
              );
            })}
            <div className="flex items-center justify-center gap-3 py-3 text-xs text-faint">
              <span>
                {sounds.length.toLocaleString()} of {count.toLocaleString()}
              </span>
              {more && (
                <button onClick={() => void run(asked, page + 1)} disabled={busy} className={buttonClass}>
                  {busy ? "Loading…" : "Show more"}
                </button>
              )}
            </div>
          </>
        )}
      </div>

      <SignIn status={status} setStatus={setStatus} onEditKey={onEditKey} />
    </>
  );
}

function licenseHint(license: string): string {
  if (license === "CC0") return "Free to use for anything, no credit needed";
  if (license.includes("NC")) return "Free for non-commercial use, with a credit. GMAudio keeps the credit with the sound.";
  if (license.startsWith("CC BY")) return "Free to use with a credit. GMAudio keeps the credit with the sound.";
  return "See the sound's page for its terms";
}

function SignIn({ status, setStatus, onEditKey }: { status: FreesoundStatus; setStatus: (s: FreesoundStatus) => void; onEditKey: () => void }) {
  const [asking, setAsking] = useState(false);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const finish = async () => {
    setBusy(true);
    try {
      setStatus(await api.freesoundSignIn(code));
      setAsking(false);
      setCode("");
    } catch (e) {
      notify(e);
    }
    setBusy(false);
  };
  const link = "font-medium text-dim underline-offset-2 hover:text-ink hover:underline";
  return (
    <div className="space-y-2 border-t border-line px-4 py-3 text-xs text-faint">
      {asking ? (
        <div className="flex flex-wrap items-center gap-2">
          <span className="shrink-0 text-dim">Your browser opened Freesound. Allow access, then paste the code it shows:</span>
          <input autoFocus value={code} onChange={(e) => setCode(e.target.value)} spellCheck={false} className={`${inputBase} h-8 min-w-0 flex-1`} aria-label="Code from Freesound" />
          <button onClick={() => void finish()} disabled={busy || !code.trim()} className={primaryClass}>
            {busy ? "Checking…" : "Finish"}
          </button>
          <button
            onClick={() =>
              void api
                .freesoundSignInUrl()
                .then((url) => navigator.clipboard.writeText(url))
                .then(() => useStore.getState().notify("Sign-in link copied. Paste it into your browser's address bar.", "info"))
                .catch(notify)
            }
            title="If the browser did not open, or showed “Access has been denied”, paste this link into its address bar instead"
            className={link}
          >
            Copy link
          </button>
          <button onClick={() => setAsking(false)} className={link}>
            Cancel
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-3">
          <span className="min-w-0 flex-1">
            {status.signedIn
              ? "Signed in to Freesound: downloads are the original files."
              : "Downloads are good-quality compressed copies. Sign in to Freesound to get the original files instead."}
          </span>
          {status.signedIn ? (
            <button onClick={() => void api.freesoundSignOut().then(setStatus).catch(notify)} className={link}>
              Sign out
            </button>
          ) : status.canSignIn ? (
            <button
              onClick={() =>
                void api
                  .freesoundOpenSignIn()
                  .then(() => setAsking(true))
                  .catch(notify)
              }
              className={link}
            >
              Sign in…
            </button>
          ) : null}
          <button onClick={onEditKey} className={link}>
            {status.canSignIn || status.signedIn ? "Change key" : "Add client id to sign in"}
          </button>
        </div>
      )}
    </div>
  );
}
