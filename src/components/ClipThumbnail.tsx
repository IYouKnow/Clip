import { useState } from "react";
import { cx } from "../lib/cx";

type Props = {
  src: string;
  className?: string;
};

/// Renders a still frame from a clip by seeking the video element itself.
/// Avoids canvas readback, which the asset protocol would taint.
export function ClipThumbnail({ src, className }: Props) {
  const [ready, setReady] = useState(false);

  return (
    <video
      key={src}
      src={src}
      muted
      playsInline
      preload="metadata"
      aria-hidden="true"
      tabIndex={-1}
      className={cx(
        "size-full object-cover transition-opacity duration-200",
        ready ? "opacity-100" : "opacity-0",
        className,
      )}
      onLoadedMetadata={(event) => {
        const video = event.currentTarget;
        const duration = Number.isFinite(video.duration) ? video.duration : 0;
        video.currentTime = Math.max(0.1, duration * 0.2) || 0.1;
      }}
      onSeeked={() => setReady(true)}
      onLoadedData={() => setReady(true)}
      onError={() => setReady(true)}
    />
  );
}
