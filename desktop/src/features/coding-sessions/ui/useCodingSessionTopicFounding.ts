import * as React from "react";

import type { CodingSessionCrewLaunchStep } from "../lib/codingSessionCrewLaunch";
import { publishCodingSessionGenesis } from "../lib/codingSessionGenesis";
import { publishCodingSessionGoal } from "../lib/codingSessionGoal";
import { createCodingSessionSessionRef } from "../lib/codingSessionLifecycleCommand";
import { publishCodingSessionName } from "../lib/codingSessionName";
import {
  type CodingSessionTopicFoundingInput,
  type CodingSessionTopicFoundingResult,
  foundCodingSessionTopic,
} from "../lib/codingSessionTopicFounding";

/** Everything outside this hook that founding a topic has to reach. */
export type CodingSessionTopicFoundingHostDeps = {
  /** The founding sequence itself. Real by default; never faked in tests. */
  runFounding: typeof foundCodingSessionTopic;
  newSessionRef: typeof createCodingSessionSessionRef;
  publishGenesis: typeof publishCodingSessionGenesis;
  publishGoal: typeof publishCodingSessionGoal;
  publishName: typeof publishCodingSessionName;
};

/** The real thing: this computer's relay and keyring. */
export const DEFAULT_CODING_SESSION_TOPIC_FOUNDING_DEPS: CodingSessionTopicFoundingHostDeps =
  {
    runFounding: foundCodingSessionTopic,
    newSessionRef: createCodingSessionSessionRef,
    publishGenesis: publishCodingSessionGenesis,
    publishGoal: publishCodingSessionGoal,
    publishName: publishCodingSessionName,
  };

/**
 * Founding a topic, wired to this computer's relay and keyring.
 *
 * The sequence lives in `codingSessionTopicFounding.ts` and is tested there;
 * this hook supplies the publishes and keeps the step list for the screen.
 * There is no provider in it anywhere — a founded topic is a signed genesis
 * and a row that says "Not started".
 *
 * `found` is re-entrant-safe on its own: a second call while one is in
 * flight returns the first call's promise rather than founding again.
 * `isFounding` is React state and only flips after the first render that
 * follows the first `await`, which is too late for a same-tick double call —
 * so the guard is a ref, set before anything is awaited.
 */
export function useCodingSessionTopicFounding(input: {
  /**
   * Resolve — creating it if needed — the channel the topic is founded in.
   * The project flow's own `ensureChannelId`, so a founding and a create mint
   * the same channel.
   */
  ensureChannelId?: (() => Promise<string>) | null;
  /** Injected in tests; this computer's relay and keyring by default. */
  deps?: CodingSessionTopicFoundingHostDeps;
}) {
  const [steps, setSteps] = React.useState<CodingSessionCrewLaunchStep[]>([]);
  const [isFounding, setIsFounding] = React.useState(false);
  const [result, setResult] =
    React.useState<CodingSessionTopicFoundingResult | null>(null);

  const settings = React.useRef(input);
  settings.current = input;
  const inFlight =
    React.useRef<Promise<CodingSessionTopicFoundingResult> | null>(null);

  const found = React.useCallback(
    (
      foundingInput: CodingSessionTopicFoundingInput,
    ): Promise<CodingSessionTopicFoundingResult> => {
      if (inFlight.current !== null) return inFlight.current;
      const current = settings.current;
      const deps = current.deps ?? DEFAULT_CODING_SESSION_TOPIC_FOUNDING_DEPS;
      setResult(null);
      setIsFounding(true);
      const run = (async () => {
        try {
          const founded = await deps.runFounding(foundingInput, {
            ensureChannel: current.ensureChannelId ?? undefined,
            newSessionRef: deps.newSessionRef,
            publishGenesis: ({ channelId, sessionRef }) =>
              deps.publishGenesis({ channelId, sessionRef }),
            publishGoal: ({ channelId, content, sessionRef }) =>
              deps.publishGoal({ channelId, content, sessionRef }),
            publishName: ({ channelId, content, sessionRef }) =>
              deps.publishName({ channelId, content, sessionRef }),
            onSteps: setSteps,
          });
          setResult(founded);
          return founded;
        } finally {
          inFlight.current = null;
          setIsFounding(false);
        }
      })();
      inFlight.current = run;
      return run;
    },
    [],
  );

  return { isFounding, found, result, steps };
}
