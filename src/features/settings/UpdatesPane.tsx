import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Download, RefreshCw } from "lucide-react";
import { OptionRow, SettingsGroup } from "./OptionRow";

export interface UpdateStatus {
  available: boolean;
  version: string | null;
  notes: string | null;
  current_version: string;
}

interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

export function UpdatesPane() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unlisteners = [
      listen<UpdateStatus>("update://available", (event) => {
        setStatus(event.payload);
      }),
      listen<UpdateProgress>("update://progress", (event) => {
        setProgress(event.payload);
      }),
    ];
    return () => {
      void Promise.all(unlisteners).then((removers) => {
        removers.forEach((remove) => remove());
      });
    };
  }, []);

  const check = () => {
    setChecking(true);
    setError(null);
    invoke<UpdateStatus>("check_for_update")
      .then(setStatus)
      .catch((err) => setError(String(err)))
      .finally(() => setChecking(false));
  };

  const install = () => {
    setInstalling(true);
    setError(null);
    invoke("install_update").catch((err) => {
      setInstalling(false);
      setProgress(null);
      setError(String(err));
    });
  };

  const percent =
    progress != null && progress.total != null && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <>
      <SettingsGroup title="Version">
        <OptionRow label="Current version">
          <span className="settings-version-value">
            {status?.current_version ?? "…"}
          </span>
        </OptionRow>
        <OptionRow label="Check GitHub releases for a new version">
          <button
            type="button"
            className="settings-mini-button"
            onClick={() => void check()}
            disabled={checking || installing}
          >
            <RefreshCw size={13} strokeWidth={2} className={checking ? "spin" : ""} />
            {checking ? "Checking…" : "Check now"}
          </button>
        </OptionRow>
      </SettingsGroup>

      {status?.available && (
        <SettingsGroup title={`Update available — ${status.version}`}>
          {status.notes && (
            <p className="settings-update-notes">{status.notes}</p>
          )}
          {installing ? (
            <OptionRow label={percent != null ? `Downloading… ${percent}%` : "Downloading…"}>
              <span className="settings-update-progress" data-indeterminate={percent == null}>
                <span style={{ width: `${percent ?? 100}%` }} />
              </span>
            </OptionRow>
          ) : (
            <OptionRow label="Glassbar will close, update, and restart itself">
              <button
                type="button"
                className="settings-mini-button primary"
                onClick={() => void install()}
              >
                <Download size={13} strokeWidth={2} />
                Install update
              </button>
            </OptionRow>
          )}
        </SettingsGroup>
      )}

      {!status?.available && status != null && !checking && (
        <p className="settings-pane-note">You're on the latest version.</p>
      )}

      {error && <p className="settings-pane-note error">{error}</p>}
    </>
  );
}
