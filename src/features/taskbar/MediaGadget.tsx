import { useCallback, useEffect, useRef, useState } from "react";
import { Pause, Play } from "lucide-react";
import {
  prewarmMenuGlass,
  toggleMediaPlay,
  type MediaSnapshot,
} from "../../lib/shell";
import { useHoverPanel } from "./hoverPanel";
import { MediaHoverCard } from "./MediaHoverCard";
import { IconButton } from "./IconButton";
import { MediaWave } from "./MediaWave";

interface MediaGadgetProps {
  media: MediaSnapshot;
}

/**
 * The audio gadget: the waveform, track title and play/pause button inside
 * the taskbar pill. Clicking it toggles the media card above the pill;
 * leaving either the gadget or the card closes it after a grace period
 * that lets the cursor travel between them. The open/close machine is the
 * taskbar's shared hover system (hoverPanel.ts) — the same one the app
 * previews run on, so the card's grace, its cancel rules and the native
 * `hover-blur` escape hatch behave identically.
 */
export function MediaGadget({ media }: MediaGadgetProps) {
  const panel = useHoverPanel({
    // The pill-resize revalidation (hoverPanel.ts): the gadget is the source,
    // and the hover card is this panel's popup (there is only ever one media
    // card, so the unscoped selector is safe).
    sourceRect: () => gadgetRef.current?.getBoundingClientRect() ?? null,
    popupRect: () =>
      document.querySelector<HTMLElement>("[data-media-hover]")
        ?.getBoundingClientRect() ?? null,
  });
  const gadgetRef = useRef<HTMLDivElement | null>(null);
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

  const toggleCard = () => {
    if (panel.isOpen) {
      panel.close();
      return;
    }
    const rect = gadgetRef.current?.getBoundingClientRect();
    if (rect) {
      panel.open(rect);
    }
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
          panel.cancelClose();
          void prewarmMenuGlass(7);
        }}
        onPointerLeave={panel.scheduleClose}
      >
        <MediaWave />
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
      {panel.anchor && (
        <MediaHoverCard
          media={media}
          isPlaying={effectivePlaying}
          onTogglePlay={handlePlayPause}
          anchor={panel.anchor}
          onHover={panel.onPopupHover}
        />
      )}
    </>
  );
}
