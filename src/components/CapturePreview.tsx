import { Video } from "lucide-react";
import { cx } from "../lib/cx";

type Props = {
  replaying: boolean;
  bufferSeconds: number;
};

export function CapturePreview({ replaying, bufferSeconds }: Props) {
  return (
    <div
      className={cx(
        "relative overflow-hidden rounded-[18px] border bg-black transition-shadow duration-300",
        replaying ? "capture-glow border-rec/40" : "border-line",
      )}
    >
      <div className="relative aspect-video w-full">
        <div
          aria-hidden="true"
          className="absolute inset-0 opacity-[0.12]"
          style={{
            backgroundImage:
              "linear-gradient(to right, rgba(255,255,255,0.6) 1px, transparent 1px), linear-gradient(to bottom, rgba(255,255,255,0.6) 1px, transparent 1px)",
            backgroundSize: "32px 32px",
          }}
        />

        {replaying && (
          <div
            aria-hidden="true"
            className="absolute inset-x-0 h-20 animate-[scan_4s_linear_infinite] bg-gradient-to-b from-transparent via-rec/20 to-transparent"
          />
        )}

        <div className="absolute inset-0 grid place-items-center">
          <div className="flex flex-col items-center gap-2 text-white/45">
            <Video className="size-7" />
            <p className="text-[13px]">Live preview coming soon</p>
          </div>
        </div>

        <div className="absolute inset-x-0 top-0 flex items-center justify-between p-3">
          <span className="flex items-center gap-2 rounded-full border border-white/10 bg-black/55 px-2.5 py-1 text-[11px] font-medium uppercase tracking-wider text-white/85 backdrop-blur">
            <span
              aria-hidden="true"
              className={cx(
                "size-2 rounded-full",
                replaying
                  ? "animate-[rec-pulse_1.6s_ease-in-out_infinite] bg-rec"
                  : "bg-white/35",
              )}
            />
            {replaying ? "Buffering" : "Replay off"}
          </span>
          {replaying && (
            <span className="rounded-full border border-white/10 bg-black/55 px-2.5 py-1 text-[11px] font-medium text-white/80 backdrop-blur">
              last {bufferSeconds}s
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
