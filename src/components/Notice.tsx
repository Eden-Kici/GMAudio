import { useEffect } from "react";
import { useStore } from "../store";
import { CloseIcon, WarningIcon } from "./icons";

/** A single toast for problems, e.g. a file that could not be played. */
export function Notice() {
  const notice = useStore((s) => s.notice);
  const dismiss = useStore((s) => s.dismissNotice);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(dismiss, 7000);
    return () => clearTimeout(timer);
  }, [notice, dismiss]);

  if (!notice) return null;
  return (
    <div
      role="alert"
      className="fixed bottom-64 left-1/2 z-50 flex max-w-xl -translate-x-1/2 items-start gap-3 rounded-lg border border-line bg-raised px-4 py-3 text-sm shadow-xl shadow-black/50"
    >
      {notice.kind === "error" && <WarningIcon className="mt-0.5 shrink-0 text-danger" />}
      <span className="select-text">{notice.text}</span>
      <button onClick={dismiss} aria-label="Dismiss" className="-mr-1 rounded p-1 text-dim hover:bg-line hover:text-ink">
        <CloseIcon width={14} height={14} />
      </button>
    </div>
  );
}
