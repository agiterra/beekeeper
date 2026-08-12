/**
 * Bindings for the H-08 standalone transcript export (two roundtrips: probe
 * filesystem facts, then execute the plan built by the pure export engine).
 */
import { invokeTauri } from "@/shared/api/tauri";
import type { TranscriptExportPlan } from "@/features/coding-sessions/lib/transcriptExport/transcriptExportEngine";

export type TranscriptExportProbe = {
  exportRoot: string;
  takenDirectoryNames: string[];
  attachmentSourceExists: Record<string, boolean>;
  appVersion: string;
  viewerDistDir: string;
};

export type WrittenTranscriptExport = {
  outputDir: string;
  indexHtmlPath: string;
  transcriptJsonPath: string;
  bundledAttachmentCount: number;
};

/** Opens the folder picker; `null` means the user cancelled. */
export async function beginCodingSessionTranscriptExport(
  attachmentAbsolutePaths: string[],
): Promise<TranscriptExportProbe | null> {
  return invokeTauri<TranscriptExportProbe | null>(
    "begin_coding_session_transcript_export",
    { attachmentAbsolutePaths },
  );
}

export async function writeCodingSessionTranscriptExport(
  plan: TranscriptExportPlan,
  viewerDistDir: string,
  exportRoot: string,
): Promise<WrittenTranscriptExport> {
  return invokeTauri<WrittenTranscriptExport>(
    "write_coding_session_transcript_export",
    { plan, viewerDistDir, exportRoot },
  );
}
