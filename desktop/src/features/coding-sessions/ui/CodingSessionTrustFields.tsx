/**
 * Editor for the coding-session provider allowlist.
 *
 * The coding-session consumer is fail-closed — it renders only events signed by
 * a key listed here — so this is the surface where a user grants or revokes the
 * right to write the record of what a session did. Until it existed the list
 * was a hand-edited JSON file, which meant revoking a compromised provider
 * required a text editor and an app restart.
 *
 * Trust is global governed config, never derived from event content, tags,
 * first-seen signers, or channel membership. Names are display metadata only
 * and are never consulted for an authority decision.
 */
import { Plus, X } from "lucide-react";
import * as React from "react";

import {
  isLocalProviderTrustEntry,
  trustEntriesFromRows,
  trustEntriesSignature,
  trustRowsFromEntries,
  validateTrustRows,
  type TrustedProviderKeyEntry,
  type TrustedProviderKeyRow,
} from "@/features/coding-sessions/lib/codingSessionTrust";
import {
  PERSONA_FIELD_CONTROL_CLASS,
  PERSONA_FIELD_SHELL_CLASS,
} from "@/features/agents/ui/agentConfigOptions";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

type CodingSessionTrustFieldsProps = {
  disabled?: boolean;
  /** Current persisted allowlist (`allowed-bridge-pubkeys` on the wire). */
  entries: readonly TrustedProviderKeyEntry[];
  /** Emits the normalized allowlist on every edit. */
  onChange: (next: TrustedProviderKeyEntry[]) => void;
  /** Reports whether the current rows would survive the backend validator. */
  onValidityChange?: (valid: boolean) => void;
};

export function CodingSessionTrustFields({
  disabled = false,
  entries,
  onChange,
  onValidityChange,
}: CodingSessionTrustFieldsProps) {
  const [rows, setRows] = React.useState<TrustedProviderKeyRow[]>(() =>
    trustRowsFromEntries(entries),
  );
  // Signature (not reference) of the last list this editor emitted. A save
  // round-trip hands back a fresh, backend-normalized array that is
  // semantically the same list — resyncing on it would discard whatever the
  // user typed while the IPC was in flight.
  const lastEmitted = React.useRef(trustEntriesSignature(entries));

  React.useEffect(() => {
    const incoming = trustEntriesSignature(entries);
    if (incoming === lastEmitted.current) return;
    lastEmitted.current = incoming;
    setRows(trustRowsFromEntries(entries));
  }, [entries]);

  const validation = React.useMemo(() => validateTrustRows(rows), [rows]);
  const isValid = validation.isValid;
  React.useEffect(() => {
    onValidityChange?.(isValid);
  }, [isValid, onValidityChange]);

  function emit(next: TrustedProviderKeyRow[]) {
    setRows(next);
    const nextEntries = trustEntriesFromRows(next);
    lastEmitted.current = trustEntriesSignature(nextEntries);
    onChange(nextEntries);
  }

  function updateRow(
    id: string,
    patch: Partial<Pick<TrustedProviderKeyRow, "pubkey" | "label">>,
  ) {
    emit(rows.map((row) => (row.id === id ? { ...row, ...patch } : row)));
  }

  function removeRow(id: string) {
    emit(rows.filter((row) => row.id !== id));
  }

  function addRow() {
    emit([...rows, { id: crypto.randomUUID(), pubkey: "", label: "" }]);
  }

  return (
    <div className="space-y-2" data-testid="coding-session-trust-fields">
      <div>
        <div className="text-sm font-medium">Coding session providers</div>
        <p className="mt-0.5 text-xs text-muted-foreground">
          Trusted provider keys. Sessions signed by any other key are ignored —
          not hidden, never rendered. An empty list disables coding sessions
          entirely.
        </p>
      </div>
      <p
        className="text-xs text-muted-foreground"
        data-testid="coding-session-trust-explainer"
      >
        An entry is added automatically for this computer's provider when coding
        sessions are provisioned. Removing that entry stops this app from
        ingesting its sessions until the provider is provisioned again.
      </p>
      <div className="space-y-2">
        {rows.length === 0 ? (
          <p
            className="text-xs italic text-muted-foreground"
            data-testid="coding-session-trust-empty"
          >
            No providers trusted yet.
          </p>
        ) : null}
        {rows.map((row) => {
          const error = validation.rowErrors[row.id];
          const isLocal = isLocalProviderTrustEntry(row);
          return (
            <div className="space-y-1" key={row.id}>
              <div className="flex items-center gap-2">
                <div
                  className={cn(
                    "flex min-h-11 flex-[2] items-center px-3",
                    PERSONA_FIELD_SHELL_CLASS,
                    error ? "border-destructive/50" : undefined,
                  )}
                >
                  <Input
                    aria-label="Provider public key"
                    className={cn(
                      "h-8 px-0 py-0 font-mono leading-6",
                      PERSONA_FIELD_CONTROL_CLASS,
                    )}
                    data-testid="coding-session-trust-pubkey"
                    disabled={disabled}
                    onChange={(event) =>
                      updateRow(row.id, { pubkey: event.target.value })
                    }
                    placeholder="64-character hex public key"
                    value={row.pubkey}
                  />
                </div>
                <div
                  className={cn(
                    "flex min-h-11 flex-1 items-center px-3",
                    PERSONA_FIELD_SHELL_CLASS,
                  )}
                >
                  <Input
                    aria-label="Provider name"
                    className={cn(
                      "h-8 px-0 py-0 leading-6",
                      PERSONA_FIELD_CONTROL_CLASS,
                    )}
                    data-testid="coding-session-trust-label"
                    disabled={disabled}
                    onChange={(event) =>
                      updateRow(row.id, { label: event.target.value })
                    }
                    placeholder="Name (optional)"
                    value={row.label}
                  />
                </div>
                <Button
                  aria-label="Remove trusted provider"
                  data-testid="coding-session-trust-remove"
                  disabled={disabled}
                  onClick={() => removeRow(row.id)}
                  size="icon"
                  type="button"
                  variant="ghost"
                >
                  <X className="h-4 w-4" />
                </Button>
              </div>
              {error ? (
                <p
                  className="ml-1 text-xs text-destructive"
                  data-testid="coding-session-trust-error"
                >
                  {error}
                </p>
              ) : isLocal ? (
                <p
                  className="ml-1 text-xs text-muted-foreground"
                  data-testid="coding-session-trust-local-note"
                >
                  This computer's provider. Removing it disables session ingest
                  until it is provisioned again.
                </p>
              ) : null}
            </div>
          );
        })}
        <Button
          data-testid="coding-session-trust-add"
          disabled={disabled}
          onClick={addRow}
          size="sm"
          type="button"
          variant="outline"
        >
          <Plus className="mr-1 h-4 w-4" />
          Add provider key
        </Button>
      </div>
    </div>
  );
}
