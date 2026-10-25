import { Volume1, Volume2, VolumeX } from "lucide-react";
import type { AudioSnapshot } from "../../../lib/shell";
import { CcSlider } from "./CcSlider";
import { Subview } from "./Subview";

/** The Sound sub-view: master output volume plus the per-app mixer. */
export function SoundView({
  audio,
  onBack,
  onMaster,
  onMute,
  onAppVolume,
}: {
  audio: AudioSnapshot | null;
  onBack: () => void;
  onMaster: (value: number) => void;
  onMute: () => void;
  onAppVolume: (pid: number, value: number) => void;
}) {
  return (
    <Subview title="Sound" onBack={onBack} contentClassName="cc-sound">
      <span className="cc-card-title">Output volume</span>
      <div className="cc-slider-row">
        <button
          type="button"
          className="cc-slider-icon"
          aria-label={audio?.masterMuted ? "Unmute" : "Mute"}
          onClick={onMute}
        >
          {audio?.masterMuted ? <VolumeX size={17} /> : <Volume2 size={17} />}
        </button>
        <CcSlider
          value={(audio?.master ?? 0.5) * 100}
          onChange={onMaster}
          onCommit={onMaster}
        />
        <span className="cc-slider-icon cc-static-icon">
          {(audio?.master ?? 0.5) > 0.5 ? <Volume2 size={17} /> : <Volume1 size={17} />}
        </span>
      </div>
      <div className="cc-sound-divider" role="separator" />
      <span className="cc-card-title">App volume mixer</span>
      {!audio || audio.apps.length === 0 ? (
        <span className="cc-empty">No apps are playing audio</span>
      ) : (
        audio.apps.map((app) => (
          <div className="cc-mixer-row" key={app.pid}>
            <span className="cc-mixer-name" title={app.name}>
              {app.icon && <img className="cc-mixer-icon" src={app.icon} alt="" />}
              <span className="cc-mixer-name-text">{app.name}</span>
            </span>
            <CcSlider
              value={app.volume * 100}
              onChange={(value) => onAppVolume(app.pid, value / 100)}
              onCommit={(value) => onAppVolume(app.pid, value / 100)}
            />
          </div>
        ))
      )}
    </Subview>
  );
}
