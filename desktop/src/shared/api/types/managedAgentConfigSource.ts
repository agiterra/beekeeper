/**
 * The tier a managed agent's resolved field came from — `ConfigSource` on the
 * host, snake-cased (`effective_config/mod.rs`). `"global"` arrives on
 * `runtimeSource` only with a null value: there is no global tier for
 * runtime, so it is the host saying no tier pins one (ledger 165).
 */
export type ManagedAgentConfigSource =
  | "instance"
  | "definition"
  | "global"
  | "instance_legacy";
