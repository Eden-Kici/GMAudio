import { useState } from "react";
import { exportBundle, importBundle } from "../bundle";
import { buttonClass, Modal, primaryClass, Toggle } from "./ui";

/** Asks whether to include the sound files, then saves. */
export function ExportDialog({ campaignId, name, onClose }: { campaignId: number | null; name: string; onClose: () => void }) {
  const [audio, setAudio] = useState(true);
  return (
    <Modal title={campaignId === null ? "Save everything to a file" : `Save “${name}” to a file`} onClose={onClose}>
      <div className="space-y-4 p-4">
        <p className="text-sm leading-relaxed text-dim">
          {campaignId === null
            ? "Every campaign, session, scene, board and piece of dynamic music goes into one file."
            : "The campaign's sessions, scenes, boards and music go into one file, with anything shared that they use."}{" "}
          Open it on another computer, or keep it as a backup.
        </p>
        <Toggle
          on={audio}
          onChange={setAudio}
          label="Include the sound files"
          hint={audio ? "A bigger file that works anywhere." : "A tiny file. The other computer needs the same sounds in its library."}
        />
        <div className="flex justify-end gap-2 pt-1">
          <button onClick={onClose} className={buttonClass}>
            Cancel
          </button>
          <button
            onClick={() => {
              onClose();
              void exportBundle(campaignId, audio);
            }}
            className={primaryClass}
          >
            Choose where to save…
          </button>
        </div>
      </div>
    </Modal>
  );
}

/** Settings: back everything up, or bring a bundle in. */
export function BackupSection() {
  const [exporting, setExporting] = useState(false);
  return (
    <div className="flex flex-wrap items-center gap-2">
      <button onClick={() => setExporting(true)} className={buttonClass}>
        Save everything to a file…
      </button>
      <button onClick={() => void importBundle()} className={buttonClass} title="Adds what is in the file. Nothing you have is changed or replaced.">
        Open a bundle file…
      </button>
      {exporting && <ExportDialog campaignId={null} name="" onClose={() => setExporting(false)} />}
    </div>
  );
}
