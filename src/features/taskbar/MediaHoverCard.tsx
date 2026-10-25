import * as SliderPrimitive from "@radix-ui/react-slider";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { Music, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import {
  getMediaThumbnail,
  getMediaTimeline,
  mediaNext,
  mediaPrevious,
  mediaSeek,
  type MediaSnapshot,
} from "../../lib/shell";
import { useLiquidGlassSurface } from "glass-ui";
import { IconButton } from "./IconButton";

// The card hugs its content (the title sets the width) up to this cap,
// beyond which the text ellipsizes instead.
const MAX_CARD_WIDTH = 520;
const VIEWPORT_MARGIN = 12;

interface MediaHoverCardProps {
  media: MediaSnapshot;
  isPlaying: boolean;
  onTogglePlay: () => void;
  anchor: DOMRect;
  onHover: (hovering: boolean) => void;
}

/**
 * The media hover card: a liquid glass box floating above the audio gadget
 * with the track's album art on the left, the title and artist beside it,
 * and the transport controls (previous, play/pause, next) on the right.
 * Below them runs a seekable progress bar with the elapsed and total times.
 * The card sizes to the title's length (capped, then ellipsized). The glass
 * opens with the native springy reveal. The `cc-hit` class on the root keeps
 * the native hit-test layer routing pointer events into the card, so it stays
 * open while the cursor is over any of it (the buttons are hit-testable
 * through their `taskbar-button` class).
 */

/** Formats seconds as m:ss (or h:mm:ss past the hour). */
function formatTime(secs: number): string {
  const total = Math.max(0, Math.floor(secs));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const mmss = `${minutes}:${seconds.toString().padStart(2, "0")}`;
  return hours > 0 ? `${hours}:${mmss.padStart(5, "0")}` : mmss;
}
export function MediaHoverCard({
  media,
  isPlaying,
  onTogglePlay,
  anchor,
  onHover,
}: MediaHoverCardProps) {
  const cardId = useId();
  const cardRef = useRef<HTMLDivElement | null>(null);
  const [thumbnail, setThumbnail] = useState<string | null>(null);
  // The position is decided once, at open time (measured before the first
  // paint), and never re-clamped while the card stays visible — track
  // changes that resize the card must not drag it around mid-session.
  const [frozenLeft, setFrozenLeft] = useState<number | null>(null);

  useLayoutEffect(() => {
    const element = cardRef.current;
    if (!element) {
      return;
    }
    const width = element.offsetWidth;
    const center = anchor.left + anchor.width / 2;
    setFrozenLeft(
      Math.min(
        Math.max(center - width / 2, VIEWPORT_MARGIN),
        window.innerWidth - VIEWPORT_MARGIN - width,
      ),
    );
    // `anchor` is a fresh rect only when the card (re)opens, so this does
    // not re-run when the media snapshot changes underneath an open card.
  }, [anchor]);

  useLiquidGlassSurface(
    true,
    `[data-media-hover="${cardId}"]`,
    20,
    `mediahover${cardId}`,
    undefined,
    undefined,
    // Springy native reveal.
    true,
  );

  // The album art is fetched lazily, only while the card is open. The
  // native side caches per track, so an unchanged track returns instantly;
  // a just-changed track may not be decoded yet, so poll briefly until the
  // art (or a confirmed absence) arrives instead of showing the placeholder
  // for the card's whole lifetime.
  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    setThumbnail(null);
    const attempt = (tries: number) => {
      void getMediaThumbnail().then((thumbnail) => {
        if (cancelled) {
          return;
        }
        if (thumbnail || tries <= 0) {
          setThumbnail(thumbnail);
          return;
        }
        timer = window.setTimeout(() => attempt(tries - 1), 400);
      });
    };
    attempt(10);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [media.title, media.artist]);

  // The playback timeline: the native side interpolates the position between
  // its polls, so a light fetch (resync every second) plus a local tick
  // (every 250ms) keeps the bar smooth without hammering SMTC. Seeding from
  // the media snapshot keeps the bar right when the card opens or the track
  // changes, instead of flashing 0:00 until the first fetch lands.
  const SEEK_GRACE_MS = 2500;
  const [progress, setProgress] = useState(() => ({
    elapsed: media.positionSecs,
    duration: media.durationSecs,
  }));
  const [scrub, setScrub] = useState<number | null>(null);
  const scrubbing = useRef(false);
  const timeline = useRef({
    position: media.positionSecs,
    sampledAt: performance.now(),
    playing: media.isPlaying,
    duration: media.durationSecs,
    seekTarget: null as number | null,
    seekAt: 0,
  });

  useEffect(() => {
    let cancelled = false;
    // Rebase on (re)open and track change so the bar never resets to 0:00.
    timeline.current = {
      position: media.positionSecs,
      sampledAt: performance.now(),
      playing: media.isPlaying,
      duration: media.durationSecs,
      seekTarget: null,
      seekAt: 0,
    };
    setProgress({ elapsed: media.positionSecs, duration: media.durationSecs });
    const seekHold = () =>
      scrubbing.current ||
      (timeline.current.seekTarget !== null &&
        performance.now() - timeline.current.seekAt < SEEK_GRACE_MS);
    const sync = async () => {
      // Hold off while dragging or inside the seek grace window: an app's
      // SMTC position lags behind a just-committed seek, and syncing during
      // that window would yank the thumb back to the pre-seek spot.
      if (seekHold()) {
        return;
      }
      const next = await getMediaTimeline();
      if (cancelled || !next) {
        return;
      }
      timeline.current = {
        position: next.positionSecs,
        sampledAt: performance.now(),
        playing: next.isPlaying,
        duration: next.durationSecs,
        seekTarget: null,
        seekAt: 0,
      };
    };
    void sync();
    const syncTimer = window.setInterval(() => void sync(), 1000);
    const tickTimer = window.setInterval(() => {
      if (scrubbing.current) {
        return;
      }
      const current = timeline.current;
      const inSeekGrace =
        current.seekTarget !== null &&
        performance.now() - current.seekAt < SEEK_GRACE_MS;
      const elapsed = inSeekGrace
        ? (current.seekTarget ?? 0) +
          (current.playing ? (performance.now() - current.seekAt) / 1000 : 0)
        : current.playing
          ? current.position + (performance.now() - current.sampledAt) / 1000
          : current.position;
      setProgress({ elapsed, duration: current.duration });
    }, 250);
    return () => {
      cancelled = true;
      window.clearInterval(syncTimer);
      window.clearInterval(tickTimer);
    };
    // Only track changes re-run this (rebase); `media` updates every couple
    // of seconds otherwise, and rebasing from those raw snapshots would
    // fight the interpolated syncs and re-introduce the jitter.
  }, [media.title, media.artist]);

  const commitSeek = (position: number) => {
    // Project from the seek target through the grace window; the next
    // timeline sync afterwards confirms the new position.
    timeline.current = {
      ...timeline.current,
      seekTarget: position,
      seekAt: performance.now(),
    };
    setProgress((current) => ({ ...current, elapsed: position }));
    void mediaSeek(position);
  };

  // The position never moves, so a track change that makes the card grow
  // can only extend it rightward; cap the growth at the viewport edge.
  const maxWidth = frozenLeft
    ? Math.min(
        MAX_CARD_WIDTH,
        window.innerWidth - VIEWPORT_MARGIN - frozenLeft,
      )
    : Math.min(MAX_CARD_WIDTH, window.innerWidth - VIEWPORT_MARGIN * 2);
  const style: CSSProperties = {
    bottom: window.innerHeight - anchor.top + 13,
    width: "max-content",
    maxWidth,
    // Before the frozen position is computed (same commit, pre-paint),
    // center via translate as a fallback; it never actually shows.
    ...(frozenLeft !== null
      ? { left: frozenLeft }
      : {
          left: anchor.left + anchor.width / 2,
          transform: "translateX(-50%)",
        }),
  };

  return (
    <div
      ref={cardRef}
      className="media-hover-card cc-hit"
      data-media-hover={cardId}
      style={style}
      onPointerEnter={() => onHover(true)}
      onPointerLeave={() => onHover(false)}
    >
      <div className="media-hover-thumb">
        {thumbnail ? (
          <img src={thumbnail} alt="" draggable={false} />
        ) : (
          <Music size={20} />
        )}
      </div>
      <div className="media-hover-body">
        <div className="media-hover-main">
          <div className="media-hover-meta">
            <span className="media-hover-title">{media.title}</span>
            <span className="media-hover-artist">{media.artist ?? ""}</span>
          </div>
          <div className="media-hover-controls">
            <IconButton
              className="media-hover-button"
              aria-label="Previous track"
              onClick={() => void mediaPrevious()}
            >
              <SkipBack size={15} />
            </IconButton>
            <IconButton
              className="media-hover-button media-hover-play"
              aria-label={isPlaying ? "Pause" : "Play"}
              onClick={onTogglePlay}
            >
              {isPlaying ? <Pause size={15} /> : <Play size={15} />}
            </IconButton>
            <IconButton
              className="media-hover-button"
              aria-label="Next track"
              onClick={() => void mediaNext()}
            >
              <SkipForward size={15} />
            </IconButton>
          </div>
        </div>
        <div className="media-hover-progress">
          <span className="media-hover-time">{formatTime(scrub ?? progress.elapsed)}</span>
          <SliderPrimitive.Root
            className="media-hover-slider"
            // Radix needs a positive max; with no reported duration the
            // slider disables and both times read 0:00.
            max={Math.max(1, Math.floor(progress.duration))}
            step={1}
            disabled={progress.duration <= 0}
            value={[Math.min(scrub ?? progress.elapsed, progress.duration)]}
            aria-label="Seek"
            onPointerDown={() => {
              scrubbing.current = true;
            }}
            onValueChange={(values) => {
              setScrub(values[0] ?? null);
            }}
            onValueCommit={(values) => {
              scrubbing.current = false;
              const position = values[0] ?? null;
              setScrub(null);
              if (position !== null) {
                commitSeek(position);
              }
            }}
          >
            <SliderPrimitive.Track className="media-hover-slider-track">
              <SliderPrimitive.Range className="media-hover-slider-range" />
            </SliderPrimitive.Track>
            <SliderPrimitive.Thumb className="media-hover-slider-thumb" />
          </SliderPrimitive.Root>
          <span className="media-hover-time">{formatTime(progress.duration)}</span>
        </div>
      </div>
    </div>
  );
}
