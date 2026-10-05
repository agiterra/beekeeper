import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import 'coding_session_fold.dart';
import 'coding_session_models.dart';
import 'coding_session_session_decoders.dart';
import 'coding_session_title_wire.dart';

/// The one display-name rule every reader of an umbrella session shares.
///
/// Mirror of `resolve_session_display_name` in
/// `crates/buzz-core/src/coding_session_title.rs`, bound to it by
/// `conformance/session-display-name/` (CONTRACT.md and its vectors). Three
/// tiers, ranked and never compared by time:
///
/// 1. **person** — the latest valid 44229 the founder signed;
/// 2. **generated** — the *earliest* valid 44252 whose signer is the provider
///    authority of the umbrella execution its `cs-target` names, so a title
///    never flips once shown;
/// 3. **fallback** — the founding execution's title, then
///    [codingSessionUntitledName].
///
/// T3 Code keeps one mutable title per thread and lets a rename win by
/// clearing an in-flight marker (`Orchestrator.ts:2858-2864`). Beekeeper's
/// names are immutable signed events from different authors, so the marker
/// becomes this ranking: same outcome — a person's name always wins — with no
/// shared mutable state.

/// Which tier named a session.
enum CodingSessionNameOrigin {
  /// A founder-signed 44229.
  person('person'),

  /// A provider-signed 44252.
  generated('generated'),

  /// The founding execution's title, or [codingSessionUntitledName].
  fallback('fallback');

  const CodingSessionNameOrigin(this.wire);

  /// The string the shared vectors and `bee` print.
  final String wire;
}

/// Events the resolver set aside, so a reader can say so instead of hiding
/// them.
@immutable
class CodingSessionNameDiagnostics {
  /// Valid 44229 revisions for this session not signed by its founder.
  final int foreignNames;

  /// Valid 44252 titles whose signer is not the provider authority of the
  /// umbrella execution their `cs-target` names.
  final int foreignTitles;

  /// 44229/44252 events carrying this session's `h` and `d` that fail their
  /// envelope. Counted only by [resolveCodingSessionDisplayNameFromEvents];
  /// the channel read counts its refusals in the trust gate instead.
  final int malformed;

  const CodingSessionNameDiagnostics({
    this.foreignNames = 0,
    this.foreignTitles = 0,
    this.malformed = 0,
  });
}

/// One execution of the umbrella as the resolver needs it.
@immutable
class CodingSessionTitleAuthority {
  /// The exact `cs-target` key, generation included.
  final String targetKey;

  /// The provider pubkey that signs that generation's facts.
  final String providerAuthorityPubkey;

  const CodingSessionTitleAuthority({
    required this.targetKey,
    required this.providerAuthorityPubkey,
  });
}

/// What the resolver needs to know about the umbrella besides its events.
@immutable
class CodingSessionDisplayNameScope {
  final String channelId;
  final String sessionRef;

  /// The founder, when known. Without one no 44229 is a person's name.
  final String? founderPubkey;

  /// The founding execution's 44223 title, when it has one.
  final String? foundingExecutionTitle;

  /// Every execution generation of the umbrella the reader holds.
  final List<CodingSessionTitleAuthority> executions;

  const CodingSessionDisplayNameScope({
    required this.channelId,
    required this.sessionRef,
    required this.founderPubkey,
    required this.foundingExecutionTitle,
    required this.executions,
  });
}

/// The resolved display name of one umbrella session.
@immutable
class CodingSessionDisplayName {
  /// The text to show, exactly as signed (untrimmed).
  final String name;

  final CodingSessionNameOrigin origin;

  /// The model that generated [name]; [CodingSessionNameOrigin.generated]
  /// only.
  final String? model;

  /// The provider pubkey (lowercase hex) that signed [name]; generated only.
  final String? signerPubkey;

  /// The `cs-target` of the execution whose provider signed [name]; generated
  /// only. Lets a surface name the provider it already shows for that
  /// execution.
  final String? targetKey;

  final CodingSessionNameDiagnostics diagnostics;

  const CodingSessionDisplayName({
    required this.name,
    required this.origin,
    this.model,
    this.signerPubkey,
    this.targetKey,
    this.diagnostics = const CodingSessionNameDiagnostics(),
  });

  /// True when a model's words, not a person's, name the session.
  bool get isGenerated => origin == CodingSessionNameOrigin.generated;
}

/// Resolve one umbrella's display name from decoded 44229 and 44252 records.
///
/// Records for any other `(h, d)` are ignored, not counted. [malformed] is
/// carried through to the diagnostics for a caller that decoded raw events.
CodingSessionDisplayName resolveCodingSessionDisplayName({
  required CodingSessionDisplayNameScope scope,
  Iterable<CodingSessionName> names = const [],
  Iterable<CodingSessionGeneratedTitle> titles = const [],
  int malformed = 0,
}) {
  final founder = scope.founderPubkey?.toLowerCase();
  var foreignNames = 0;
  var foreignTitles = 0;
  CodingSessionName? person;
  CodingSessionGeneratedTitle? generated;

  bool inScope(CodingSessionEventRef ref, String sessionRef) =>
      ref.channelId == scope.channelId && sessionRef == scope.sessionRef;

  for (final record in names) {
    if (!inScope(record.ref, record.sessionRef)) continue;
    if (founder == null || record.ref.signerPubkey.toLowerCase() != founder) {
      foreignNames += 1;
      continue;
    }
    if (person == null || _compareRefs(record.ref, person.ref) > 0) {
      person = record;
    }
  }
  for (final record in titles) {
    if (!inScope(record.ref, record.sessionRef)) continue;
    final signer = record.ref.signerPubkey.toLowerCase();
    final standing = scope.executions.any(
      (execution) =>
          execution.targetKey == record.targetKey &&
          execution.providerAuthorityPubkey.toLowerCase() == signer,
    );
    if (!standing) {
      foreignTitles += 1;
      continue;
    }
    if (generated == null || _compareRefs(record.ref, generated.ref) < 0) {
      generated = record;
    }
  }

  final diagnostics = CodingSessionNameDiagnostics(
    foreignNames: foreignNames,
    foreignTitles: foreignTitles,
    malformed: malformed,
  );
  if (person != null) {
    return CodingSessionDisplayName(
      name: person.content,
      origin: CodingSessionNameOrigin.person,
      diagnostics: diagnostics,
    );
  }
  if (generated != null) {
    return CodingSessionDisplayName(
      name: generated.title,
      origin: CodingSessionNameOrigin.generated,
      model: generated.model,
      signerPubkey: generated.ref.signerPubkey.toLowerCase(),
      targetKey: generated.targetKey,
      diagnostics: diagnostics,
    );
  }
  final founding = scope.foundingExecutionTitle;
  return CodingSessionDisplayName(
    name: founding != null && founding.trim().isNotEmpty
        ? founding
        : codingSessionUntitledName,
    origin: CodingSessionNameOrigin.fallback,
    diagnostics: diagnostics,
  );
}

/// [resolveCodingSessionDisplayName] over raw events, decoding each in-scope
/// 44229 and 44252 with this app's own strict decoders and counting the ones
/// that fail as malformed. The conformance vectors run through this.
///
/// No signature check: a reader verifies signatures before this rule, and the
/// vectors' ids and signers are synthetic labels.
CodingSessionDisplayName resolveCodingSessionDisplayNameFromEvents({
  required CodingSessionDisplayNameScope scope,
  required Iterable<NostrEvent> events,
}) {
  final names = <CodingSessionName>[];
  final titles = <CodingSessionGeneratedTitle>[];
  var malformed = 0;
  for (final event in events) {
    if (event.kind != EventKind.codingSessionName &&
        event.kind != EventKind.codingSessionGeneratedTitle) {
      continue;
    }
    if (!_carriesTag(event, 'h', scope.channelId) ||
        !_carriesTag(event, 'd', scope.sessionRef)) {
      continue;
    }
    if (event.kind == EventKind.codingSessionName) {
      final decoded = decodeCodingSessionName(event, verifier: null);
      if (decoded.value == null) {
        malformed += 1;
      } else {
        names.add(decoded.value!);
      }
      continue;
    }
    final decoded = decodeCodingSessionGeneratedTitle(event, verifier: null);
    if (decoded.value == null) {
      malformed += 1;
    } else {
      titles.add(decoded.value!);
    }
  }
  return resolveCodingSessionDisplayName(
    scope: scope,
    names: names,
    titles: titles,
    malformed: malformed,
  );
}

/// The display name of one folded umbrella.
///
/// Builds the shared scope from what the fold already holds: the founder, the
/// founding execution's title, and every generation's provider authority. An
/// implicit session (no `sessionRef`) has no name record to read and resolves
/// straight to its fallback.
CodingSessionDisplayName resolveCodingSessionUmbrellaName({
  required String channelId,
  required String? sessionRef,
  required String? founderPubkey,
  required List<CodingSessionExecution> executions,
  Iterable<CodingSessionCreate> creates = const [],
  Iterable<CodingSessionName> names = const [],
  Iterable<CodingSessionGeneratedTitle> titles = const [],
}) {
  final scope = CodingSessionDisplayNameScope(
    channelId: channelId,
    sessionRef: sessionRef ?? '',
    founderPubkey: founderPubkey,
    foundingExecutionTitle: foundingCodingSessionExecutionTitle(
      executions,
      creates,
    ),
    executions: [
      for (final execution in executions)
        // Only a verified authority stands: a create this execution's
        // provider confirmed with a lifecycle receipt. The trust gate's
        // fallback (the first-seen metadata signer for a target no readable
        // create claims, `verified: false`) is anyone in the channel, so its
        // 44252 counts as foreign — as desktop requires a lifecycle receipt
        // from the signer (`confirmedExecutions`) and `bee` a confirmed row
        // (`scope_from_rows`).
        if (execution.authority.verified &&
            execution.authority.pubkey.isNotEmpty)
          CodingSessionTitleAuthority(
            targetKey: execution.targetKey,
            providerAuthorityPubkey: execution.authority.pubkey,
          ),
    ],
  );
  if (sessionRef == null) {
    return resolveCodingSessionDisplayName(scope: scope);
  }
  return resolveCodingSessionDisplayName(
    scope: scope,
    names: names,
    titles: titles,
  );
}

/// The founding execution's 44223 title, or `null`.
///
/// The founding execution is the stream attached first — by its create's
/// `created_at` when the create is readable, else by its own activity — ties
/// broken by execution key, as the desktop orders an umbrella's executions
/// (`codingSessionUmbrellaModel.ts`, `attachOrderMs`). Its newest generation
/// that carries metadata speaks for it.
String? foundingCodingSessionExecutionTitle(
  List<CodingSessionExecution> executions,
  Iterable<CodingSessionCreate> creates,
) {
  if (executions.isEmpty) return null;
  final createdAtByCommand = {
    for (final create in creates) create.commandId: create.ref.createdAt,
  };
  final attachByStream = <String, int>{};
  for (final execution in executions) {
    final attach =
        createdAtByCommand[execution.commandId] ?? execution.lastActivityAt;
    final incumbent = attachByStream[execution.executionKey];
    if (incumbent == null || attach < incumbent) {
      attachByStream[execution.executionKey] = attach;
    }
  }
  final founding = attachByStream.entries.reduce(
    (best, candidate) =>
        candidate.value < best.value ||
            (candidate.value == best.value &&
                candidate.key.compareTo(best.key) < 0)
        ? candidate
        : best,
  );
  CodingSessionExecution? speaking;
  for (final execution in executions) {
    if (execution.executionKey != founding.key || execution.metadata == null) {
      continue;
    }
    if (speaking == null ||
        execution.target.generation > speaking.target.generation) {
      speaking = execution;
    }
  }
  return speaking?.metadata?.title;
}

bool _carriesTag(NostrEvent event, String name, String value) => event.tags.any(
  (tag) => tag.length >= 2 && tag[0] == name && tag[1] == value,
);

/// `(created_at, id)` order, id compared as a lowercase string.
int _compareRefs(CodingSessionEventRef left, CodingSessionEventRef right) {
  final byTime = left.createdAt.compareTo(right.createdAt);
  return byTime != 0
      ? byTime
      : left.eventId.toLowerCase().compareTo(right.eventId.toLowerCase());
}
