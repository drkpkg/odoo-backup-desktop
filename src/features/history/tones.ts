import type { Tone } from "../../components/Badge";
import type { DriveUploadStatus, HistoryStatus } from "../../lib/types";

export const STATUS_TONES: Record<HistoryStatus, Tone> = {
  running: "info",
  success: "success",
  failed: "danger",
  cancelled: "neutral",
};

export const DRIVE_TONES: Record<DriveUploadStatus, Tone> = {
  skipped: "neutral",
  success: "success",
  failed: "danger",
};
