/**
 * H-08 — orchestrates a standalone transcript export: map the transcript,
 * probe filesystem facts (folder picker, taken names, viewer dist), build
 * the plan with the pure engine, execute it Rust-side.
 *
 * v1 ships metadata attachment mode with an empty workspace path — Beekeeper
 * transcript items carry no attachments or workspace path today, so the
 * engine's attachment matrix and share rewrite run law-complete over empty
 * inputs (both banked vectors).
 */
import * as React from "react";
import { toast } from "sonner";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  beginCodingSessionTranscriptExport,
  writeCodingSessionTranscriptExport,
} from "@/shared/api/tauriCodingSessionExport";
import { buildTranscriptExportPlan } from "@/features/coding-sessions/lib/transcriptExport/transcriptExportEngine";
import { mapTranscriptItemsToExportMessages } from "@/features/coding-sessions/lib/transcriptExport/transcriptExportMessages";

type ExportableCodingSession = {
  title: string | null;
  transcript: TranscriptItem[];
};

export function useCodingSessionExport(
  generationId: string,
  session: ExportableCodingSession,
): { exportTranscript: () => void; isExporting: boolean } {
  const [isExporting, setIsExporting] = React.useState(false);

  const exportTranscript = React.useCallback(() => {
    if (isExporting) return;
    setIsExporting(true);
    void (async () => {
      try {
        const probe = await beginCodingSessionTranscriptExport([]);
        if (!probe) return; // user cancelled the folder picker
        const plan = buildTranscriptExportPlan({
          chatId: generationId,
          title: session.title ?? "",
          workspacePath: "",
          theme: document.documentElement.classList.contains("dark")
            ? "dark"
            : "light",
          attachmentMode: "metadata",
          messages: mapTranscriptItemsToExportMessages(session.transcript),
          nowIso: new Date().toISOString(),
          viewerVersion: probe.appVersion,
          takenDirectoryNames: probe.takenDirectoryNames,
          attachmentSourceExists: probe.attachmentSourceExists,
        });
        const written = await writeCodingSessionTranscriptExport(
          plan,
          probe.viewerDistDir,
          probe.exportRoot,
        );
        toast.success(`Transcript exported to ${written.outputDir}`);
      } catch (error) {
        toast.error(
          error instanceof Error ? error.message : "Transcript export failed.",
        );
      } finally {
        setIsExporting(false);
      }
    })();
  }, [generationId, isExporting, session.title, session.transcript]);

  return { exportTranscript, isExporting };
}
