/**
 * Where a project's model registry lives, and what a host with none says.
 *
 * There are two places, in this order (spec § 4.11,
 * `desktop/src-tauri/src/commands/model_registry.rs`):
 *
 * 1. `model-registry.yaml` at the root of the project's agents repository,
 *    `<slug>-beekeeper-agents`, beside `team.yml`. The agents-repository seed
 *    writes it, so it travels with the project.
 * 2. `team/model-registry.yaml` in the project's code checkout — what
 *    Beekeeper's own repository has, and the only place any reader looked
 *    before 2026-09-20.
 *
 * **That single place is why routing did not work off this repository.** On
 * the live Pivot Test run a routed hire (`class builder`, `risk 2,2,2`) was
 * refused `HIRE_NO_ROUTE — registry not readable on this host`; the retry ten
 * seconds later was unrouted and the seat ran the identity's own pin,
 * `opus[1m]`. Andy's eight-seat run did the same thing seven times over
 * (ledger 178(a), 179(b)).
 *
 * There is still no registry compiled into the app. A registry the operator
 * cannot open is a hardcoded opinion wearing one's name, and every routing
 * decision it produced would cite a version that was never on disk. When
 * neither place holds one, the host refuses with a sentence naming both — see
 * [`describeUnreadableModelRegistry`] for the shape that sentence takes.
 */
import type { CodingSessionRegistrySource } from "./codingSessionHireRouting";

/** The registry's path within a project's code checkout. Spelled once. */
export const MODEL_REGISTRY_PROJECT_PATH = "team/model-registry.yaml";

/**
 * The registry's file name at an agents repository's root.
 *
 * Mirrors `buzz_core::model_registry_source::AGENTS_REPO_REGISTRY_FILE`, the
 * name the seed writes and every reader composes.
 */
export const AGENTS_REPO_MODEL_REGISTRY_FILE = "model-registry.yaml";

/**
 * The `unreadable` source for a project with no registry in either place,
 * naming both files.
 *
 * The sentence is deliberately *not* "nothing offered clears that class at
 * that risk tier": that one describes a registry that exists and gated every
 * candidate out, and its remedy is a different class or a different risk. The
 * remedy for this one is a file. Conflating them is what sent the live lead
 * hunting a routing bug that was not there.
 *
 * The host produces this sentence itself, with absolute paths, whenever it can
 * (`read_model_registry`). This function is the answer for the cases that
 * never reach the host — no project resolved, or no recorded directory — where
 * all that can honestly be named is the two relative paths.
 */
export function describeUnreadableModelRegistry(
  checkoutPath: string | null,
): Extract<CodingSessionRegistrySource, { kind: "unreadable" }> {
  const root = checkoutPath === null ? null : checkoutPath.replace(/\/+$/, "");
  const agents =
    root === null
      ? `<agents repository>/${AGENTS_REPO_MODEL_REGISTRY_FILE}`
      : `${root}-beekeeper-agents/${AGENTS_REPO_MODEL_REGISTRY_FILE}`;
  const checkout =
    root === null
      ? MODEL_REGISTRY_PROJECT_PATH
      : `${root}/${MODEL_REGISTRY_PROJECT_PATH}`;
  return {
    kind: "unreadable",
    why:
      `no model registry: looked in ${agents} and ${checkout}. Seed the ` +
      "project's agents repository with a model-registry.yaml, route from a " +
      "checkout with the CLI, or hire without routing.",
  };
}
