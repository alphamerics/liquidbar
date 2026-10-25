import { useCallback, useEffect, useRef, useState } from "react";
import { Pause, Play } from "lucide-react";
import {
  prewarmMenuGlass,
  toggleMediaPlay,
  type MediaSnapshot,
} from "../../lib/shell";
import { MediaHoverCard } from "./MediaHoverCard";
import { IconButton } from "./IconButton";

const IDLE_LEVELS = [0.1, 0.1, 0.1, 0.1, 0.1];
const CLOSE_GRACE_MS = 350;

interface MediaGadgetProps {
  media: MediaSnapshot;
  mediaLevels: number[];
}

/**
 * The audio gadget: the waveform, track title and play/pause button inside
 * the taskbar pill. Clicking it toggles the media card above the pill;
 * leaving either the gadget or the card closes it after a grace period
 * that lets the cursor travel between them.
 */
export function MediaGadget({ media, mediaLevels }: MediaGadgetProps) {
  const [cardAnchor, setCardAnchor] = useState<DOMRect | null>(null);
  const gadgetRef = useRef<HTMLDivElement | null>(null);
  const closeTimer = useRef(0);
  // Optimistic play/pause: flip instantly on click, then yield to the
  // authoritative snapshot once it confirms (or after a grace period).
  const [playingOverride, setPlayingOverride] = useState<boolean | null>(null);
  const effectivePlaying = playingOverride ?? media.isPlaying;

  useEffect(() => {
    if (playingOverride === null) {
      return;
    }
    if (media.isPlaying === playingOverride) {
      setPlayingOverride(null);
      return;
    }
    const timer = window.setTimeout(() => setPlayingOverride(null), 4000);
    return () => window.clearTimeout(timer);
  }, [playingOverride, media]);

  useEffect(
    () => () => {
      window.clearTimeout(closeTimer.current);
    },
    [],
  );

  const clearCloseTimer = () => window.clearTimeout(closeTimer.current);

  const toggleCard = () => {
    clearCloseTimer();
    setCardAnchor((current) =>
      current ? null : gadgetRef.current?.getBoundingClientRect() ?? null,
    );
  };

  const scheduleClose = () => {
    clearCloseTimer();
    closeTimer.current = window.setTimeout(() => setCardAnchor(null), CLOSE_GRACE_MS);
  };

  const handlePlayPause = useCallback(() => {
    setPlayingOverride(!effectivePlaying);
    void toggleMediaPlay();
  }, [effectivePlaying]);

  return (
    <>
      {/* `cc-hit` routes the native hit-test layer's pointer events into the
          whole gadget, so clicking anywhere on it toggles the card (the
          play button stops propagation to keep toggling playback). */}
      <div
        ref={gadgetRef}
        className="taskbar-gadget media-gadget cc-hit"
        aria-label="Now playing"
        onClick={toggleCard}
        onPointerEnter={(event) => {
          if (event.pointerType === "touch") {
            return;
          }
          clearCloseTimer();
          void prewarmMenuGlass(7);
        }}
        onPointerLeave={scheduleClose}
      >
        <span className="media-wave" aria-hidden="true">
          {(mediaLevels.length === 5 ? mediaLevels : IDLE_LEVELS).map((level, index) => (
            <span
              key={index}
              className="media-wave-bar"
              style={{ height: `${Math.round(Math.max(10, level * 100))}%` }}
            />
          ))}
        </span>
        <span className="media-gadget-text">
          {media.artist ? `${media.artist} | ${media.title}` : media.title}
        </span>
        <IconButton
          className="media-play-button"
          aria-label={effectivePlaying ? "Pause" : "Play"}
          onClick={(event) => {
            event.stopPropagation();
            handlePlayPause();
          }}
        >
          {effectivePlaying ? <Pause size={14} /> : <Play size={14} />}
        </IconButton>
      </div>
      {cardAnchor && (
        <MediaHoverCard
          media={media}
          isPlaying={effectivePlaying}
          onTogglePlay={handlePlayPause}
          anchor={cardAnchor}
          onHover={(hovering) => (hovering ? clearCloseTimer() : scheduleClose())}
        />
      )}
    </>
  );
}
