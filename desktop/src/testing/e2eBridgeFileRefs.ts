/**
 * SV-32 file chips in the E2E bridge.
 *
 * Answers `coding_session_file_refs`, `coding_session_open_file_ref` and
 * `coding_session_reveal_file_ref` from `mock.codingSessionFileRefs`, keyed
 * by provider session id. Unconfigured, it passes every command on, so they
 * stay unsupported (the lookup rejects and the paths stay plain code).
 *
 * Open and reveal launch nothing here; each call is recorded on
 * `window.__BEEKEEPER_E2E_FILE_REF_CALLS__` so a spec can assert that the
 * renderer sent the candidate as written and never an absolute path.
 */

/** One execution's mocked answer; `refs` keyed by candidate as written. */
export type MockFileRefsAnswer = {
  where:
    | "thisComputer"
    | "folderGone"
    | "notLocal"
    | "notRecorded"
    | "storeUnreadable";
  reason?: string | null;
  source?: "session" | "project" | "channel" | null;
  refs?: Record<
    string,
    {
      exists: boolean;
      isDir?: boolean;
      relativePath?: string | null;
      fullPath?: string | null;
      line?: number | null;
      column?: number | null;
    }
  >;
};

export type MockFileRefsConfig = {
  /** Keyed by `providerSessionId`. */
  bySession?: Record<string, MockFileRefsAnswer>;
  /** Candidates whose open/reveal the host refuses, with its sentence. */
  actionErrors?: Record<string, string>;
};

type Request = {
  providerSessionId?: string | null;
  candidates?: string[];
  candidate?: string;
};

type Recorder = { __BEEKEEPER_E2E_FILE_REF_CALLS__?: unknown[] };

function record(command: string, request: Request) {
  if (typeof window === "undefined") return;
  const target = window as unknown as Recorder;
  target.__BEEKEEPER_E2E_FILE_REF_CALLS__ ??= [];
  target.__BEEKEEPER_E2E_FILE_REF_CALLS__.push({ command, request });
}

/** `null` passes the command on; otherwise the value the command returns. */
export async function handleFileRefsMockCommand(
  command: string,
  payload: unknown,
  config: { mock?: Record<string, unknown> | undefined } | null | undefined,
): Promise<{ handled: true; value: unknown } | null> {
  if (
    command !== "coding_session_file_refs" &&
    command !== "coding_session_open_file_ref" &&
    command !== "coding_session_reveal_file_ref"
  ) {
    return null;
  }
  const mock = config?.mock?.codingSessionFileRefs as
    | MockFileRefsConfig
    | undefined;
  if (!mock) return null;
  const request = ((payload as { request?: Request } | null)?.request ??
    {}) as Request;
  record(command, request);
  const answer = request.providerSessionId
    ? mock.bySession?.[request.providerSessionId]
    : undefined;

  if (command === "coding_session_file_refs") {
    if (!answer) {
      return {
        handled: true,
        value: {
          where: "notRecorded",
          source: null,
          reason:
            "No working tree for this session is recorded on this computer.",
          checkedAt: new Date().toISOString(),
          refs: {},
        },
      };
    }
    const local = answer.where === "thisComputer";
    const refs: Record<string, unknown> = {};
    for (const candidate of request.candidates ?? []) {
      const ref = answer.refs?.[candidate];
      if (!local) continue;
      refs[candidate] = {
        exists: ref?.exists ?? false,
        isDir: ref?.isDir ?? false,
        relativePath: ref?.relativePath ?? null,
        fullPath: ref?.fullPath ?? null,
        line: ref?.line ?? null,
        column: ref?.column ?? null,
      };
    }
    return {
      handled: true,
      value: {
        where: answer.where,
        source: answer.source ?? (local ? "session" : null),
        reason: local ? null : (answer.reason ?? null),
        checkedAt: new Date().toISOString(),
        refs,
      },
    };
  }

  const refused = request.candidate
    ? mock.actionErrors?.[request.candidate]
    : undefined;
  if (refused) throw new Error(refused);
  if (answer?.where !== "thisComputer") {
    throw new Error(answer?.reason ?? "That file is not on this computer.");
  }
  const ref = request.candidate ? answer.refs?.[request.candidate] : undefined;
  if (!ref?.exists) {
    throw new Error("That file is no longer in this session's folder.");
  }
  return { handled: true, value: null };
}
