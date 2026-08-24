/**
 * Relay push-policy denial token for a repository with no ACL at all —
 * neither a project nor a `buzz-channel` binding. Declared in Rust as
 * `buzz-core::git_perms::GIT_NO_CHANNEL_BINDING_TOKEN`; the relay's denial
 * body starts with it ("no_channel_binding: repository has no channel
 * binding"). The token's name predates project-roster access and is kept
 * byte-identical because relays and desktops already in the field match it;
 * the legacy spaced phrase is a second matcher for relays deployed before
 * the token existed.
 *
 * A repository *inside* a project never produces this denial — the relay
 * reserves it for the no-ACL case, so the copy below can safely name both
 * remedies without misdirecting anyone.
 */
const NO_CHANNEL_BINDING_TOKEN = "no_channel_binding";
const NO_CHANNEL_BINDING_LEGACY_PHRASE = "no channel binding";

const NO_CHANNEL_BINDING_COPY =
  "This repository is not in a project and has no channel binding, so the " +
  "relay cannot authorize access. The repository owner can fix it with: " +
  "bee repos bind --id <repo> --project <30621:owner:project> " +
  "(or --channel <channel-uuid>)";

/** True when a git/relay error text is the unbound-repository denial. */
export function isNoChannelBindingError(message: string): boolean {
  return (
    message.includes(NO_CHANNEL_BINDING_TOKEN) ||
    message.includes(NO_CHANNEL_BINDING_LEGACY_PHRASE)
  );
}

/** Map a thrown branch-operation error to user-facing dialog copy. */
export function projectBranchErrorMessage(
  error: unknown,
  fallback: string,
): string {
  if (!(error instanceof Error)) return fallback;
  if (isNoChannelBindingError(error.message)) {
    return NO_CHANNEL_BINDING_COPY;
  }
  return error.message;
}
