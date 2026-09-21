import 'dart:math';

import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/agents_repo_draft_op.dart';
import 'agents_repo_drafts_provider.dart';

/// A save whose head moved since the editor opened: refused here, before
/// signing, naming who saved. The text stays with the caller.
class AgentsRepoHeadConflict implements Exception {
  final String message;
  const AgentsRepoHeadConflict(this.message);

  @override
  String toString() => message;
}

/// A write the writer's own clock rule could not make.
class AgentsRepoClockError implements Exception {
  final String message;
  const AgentsRepoClockError(this.message);

  @override
  String toString() => message;
}

/// Publishes kind 44249 ops for one project. There is no commit here:
/// mobile has no git, and a draft becomes real only when someone commits
/// it from a desktop or with `bee`. Every method signs and sends one op
/// and waits for the relay's OK; a refusal arrives as an [Exception]
/// carrying the relay's message verbatim.
class AgentsRepoActions {
  final String address;
  final String repo;
  final SignedEventRelay _relay;
  final AgentsRepoDraftsNotifier _read;
  final DateTime Function() _now;
  final Map<String, int> _sent = {};

  AgentsRepoActions({
    required this.address,
    required this.repo,
    required SignedEventRelay relay,
    required AgentsRepoDraftsNotifier read,
    DateTime Function()? now,
  }) : _relay = relay,
       _read = read,
       _now = now ?? DateTime.now;

  int _stamp(List<String> paths) {
    final now = _now().millisecondsSinceEpoch ~/ 1000;
    int? latest;
    for (final path in paths.isEmpty ? const [''] : paths) {
      for (final candidate in [_read.latestSeenFor(path), _sent[path]]) {
        if (candidate == null) continue;
        latest = latest == null ? candidate : max(latest, candidate);
      }
    }
    final stamped = latest == null ? now : max(now, latest + 1);
    if (stamped - now > agentsRepoClockSkew.inSeconds) {
      throw const AgentsRepoClockError(
        'This file was last changed too far in the future for this device '
        'to write after it; try again shortly.',
      );
    }
    return stamped;
  }

  /// Refuse when the head of [path] is not [openedOn] (the head the editor
  /// opened on; `null` = none).
  void _checkHead(String path, String? openedOn, String Function(String) name) {
    final head = _read.headOf(path);
    if ((head?.id) == openedOn) return;
    if (head == null) {
      throw const AgentsRepoHeadConflict(
        'The draft you were editing was committed or withdrawn; reload to '
        'start from what is there now.',
      );
    }
    throw AgentsRepoHeadConflict(
      '${name(head.author)} saved a newer draft; reload to see it. Your text '
      'is kept here until you do.',
    );
  }

  Future<void> _publish(AgentsRepoDraftOp op) async {
    // The wire validator's own refusals, before a round trip.
    if (decodeAgentsRepoDraftOp(op.toContent(), repo) == null) {
      throw const FormatException('the draft does not pass the wire grammar');
    }
    final paths = op.namedPaths;
    final createdAt = _stamp(paths);
    await _relay.submit(
      kind: EventKind.agentsRepoDraftOp,
      content: op.toContent(),
      tags: op.tags(address),
      createdAt: createdAt,
      onSigned: _read.addLocal,
    );
    for (final path in paths.isEmpty ? const [''] : paths) {
      _sent[path] = createdAt;
    }
  }

  /// Draft the whole new text of [path].
  Future<void> saveDraft({
    required String path,
    required String text,
    required String? base,
    required String? baseCommit,
    required String? openedOn,
    required String? message,
    required String Function(String pubkey) authorName,
  }) async {
    final klass = draftPathClass(path);
    if (klass == null) throw FormatException('$path is outside the layout');
    final textError = draftTextError(text);
    if (textError != null) throw FormatException(textError);
    if (message != null) {
      final messageError = draftMessageError(message);
      if (messageError != null) throw FormatException(messageError);
    }
    _checkHead(path, openedOn, authorName);
    await _publish(
      AgentsRepoDraftOp.filePut(
        repo: repo,
        path: path,
        text: text,
        base: base,
        baseCommit: baseCommit,
        prev: _read.headOf(path)?.id,
        message: message,
      ),
    );
  }

  /// Draft moving a role or plan to or from `archive/`.
  Future<void> moveDraft({
    required String path,
    required String base,
    required String? baseCommit,
    required String? openedOn,
    required String? message,
    required String Function(String pubkey) authorName,
  }) async {
    final to = archiveCounterpart(path);
    if (to == null) {
      throw FormatException('$path is not a role or plan that can be archived');
    }
    _checkHead(path, openedOn, authorName);
    await _publish(
      AgentsRepoDraftOp.fileMove(
        repo: repo,
        path: path,
        to: to,
        base: base,
        baseCommit: baseCommit,
        prev: _read.headOf(path)?.id,
        message: message,
      ),
    );
  }

  /// Withdraw a draft this key wrote (NIP-09).
  Future<void> withdraw(String draftId) async {
    await _relay.submit(
      kind: EventKind.deletion,
      content: '',
      tags: [
        ['e', draftId],
      ],
    );
    _read.removeLocal(draftId);
  }
}

/// The publisher for one project and repository.
final agentsRepoActionsProvider =
    Provider.family<AgentsRepoActions, ({String address, String repo})>((
      ref,
      key,
    ) {
      final config = ref.watch(relayConfigProvider);
      final session = ref.read(relaySessionProvider.notifier);
      return AgentsRepoActions(
        address: key.address,
        repo: key.repo,
        relay: SignedEventRelay(session: session, nsec: config.nsec),
        read: ref.read(agentsRepoDraftsProvider(key).notifier),
      );
    });
