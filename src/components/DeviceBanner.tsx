import { useStore } from "../store";
import { WarningIcon } from "./icons";

/** Shown only while there is no working audio output. */
export function DeviceBanner() {
  const engine = useStore((s) => s.engine);
  const retry = useStore((s) => s.retryDevice);
  if (!engine || engine.device_ok) return null;

  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-danger/40 bg-danger/15 px-4 py-2.5 text-sm" role="alert">
      <WarningIcon className="shrink-0 text-danger" />
      <span>
        <strong className="font-semibold">No speakers or headphones found.</strong>{" "}
        <span className="text-dim">Plug in or switch on an audio output. GMAudio checks again every few seconds.</span>
      </span>
      <button onClick={() => void retry()} className="ml-auto rounded-md bg-raised px-3 py-1.5 text-xs font-medium hover:bg-line">
        Check now
      </button>
    </div>
  );
}
