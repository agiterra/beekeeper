import { LoaderCircle, LogIn } from "lucide-react";
import * as React from "react";

import {
  useAcpAuthMethodsQuery,
  useConnectAcpRuntimeMutation,
} from "@/features/agents/hooks";
import { Button } from "@/shared/ui/button";
import {
  connectableCodingSessionAuthMethods,
  isHeadlessCodingSessionLogin,
} from "../lib/newCodingSessionModel";

/**
 * Launch a signed-out runtime's own sign-in from the create flow.
 *
 * Discovery and launch reuse the agents feature's ACP auth plumbing — the
 * same backend Settings → Harnesses and Onboarding drive — so the login
 * itself is always the vendor CLI's flow: headless browser OAuth for the
 * Claude subscription login, a visible terminal window otherwise. Only
 * CLI-driven methods are offered; adapters advertising other schemes (API
 * keys and the like) get no button here.
 *
 * Renders nothing when the adapter advertises no connectable method — the
 * surrounding remediation copy already tells the person what to run by hand.
 */
export function CodingSessionRuntimeConnect({
  runtime,
  label,
  disabled,
  onLoginLaunched,
}: {
  /** Runtime slug, e.g. "claude" — doubles as the ACP runtime id. */
  runtime: string;
  /** Display label for the runtime, e.g. "Claude Code". */
  label: string;
  disabled?: boolean;
  /** Fired once a login flow actually launched (browser or terminal). */
  onLoginLaunched?: (input: { runtime: string; headless: boolean }) => void;
}) {
  const methodsQuery = useAcpAuthMethodsQuery(runtime);
  const connectMutation = useConnectAcpRuntimeMutation();
  const [launchedGuidance, setLaunchedGuidance] = React.useState<string | null>(
    null,
  );

  const methods = connectableCodingSessionAuthMethods(
    methodsQuery.data?.methods ?? [],
  );
  if (methods.length === 0) return null;

  const errorMessage = connectMutation.error
    ? `Couldn't start the ${label} sign-in: ${
        connectMutation.error instanceof Error
          ? connectMutation.error.message
          : "Connection failed."
      }`
    : null;

  return (
    <div
      className="flex flex-col gap-1.5"
      data-testid={`coding-session-runtime-connect-${runtime}`}
    >
      <div className="flex flex-wrap items-center gap-2">
        {methods.map((method, index) => {
          const isLaunching =
            connectMutation.isPending &&
            connectMutation.variables?.methodId === method.id;
          return (
            <Button
              data-testid={`coding-session-runtime-connect-${runtime}-${method.id}`}
              disabled={disabled || connectMutation.isPending}
              key={method.id}
              onClick={() => {
                setLaunchedGuidance(null);
                connectMutation.mutate(
                  { runtimeId: runtime, methodId: method.id },
                  {
                    onSuccess: (result) => {
                      if (!result.launched) return;
                      const headless = isHeadlessCodingSessionLogin(
                        runtime,
                        method.id,
                      );
                      setLaunchedGuidance(
                        headless
                          ? "Complete the sign-in in your browser — status here updates automatically."
                          : "Finish signing in in the terminal window that opened — status here updates automatically.",
                      );
                      onLoginLaunched?.({ runtime, headless });
                    },
                  },
                );
              }}
              size="sm"
              type="button"
              variant={index === 0 ? "default" : "outline"}
            >
              {isLaunching ? (
                <LoaderCircle className="animate-spin motion-reduce:animate-none" />
              ) : (
                <LogIn />
              )}
              {methods.length === 1 ? `Connect ${label}` : method.name}
            </Button>
          );
        })}
      </div>
      {launchedGuidance ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid={`coding-session-runtime-connect-${runtime}-guidance`}
          role="status"
        >
          {launchedGuidance}
        </p>
      ) : null}
      {errorMessage ? (
        <p
          className="text-xs text-destructive"
          data-testid={`coding-session-runtime-connect-${runtime}-error`}
        >
          {errorMessage}
        </p>
      ) : null}
    </div>
  );
}
