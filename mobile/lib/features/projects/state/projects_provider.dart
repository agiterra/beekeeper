import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/project_models.dart';

/// How the projects read is doing, as the pages may state it.
enum ProjectsConnection { idle, connecting, open, error }

/// Every project head the relay showed this reader, minus tombstones.
@immutable
class ProjectsRead {
  final List<Project> projects;

  /// Channel metadata fetched for the channel ids project heads name that
  /// this device's own channel list did not carry (a transport channel
  /// admits members without a 39002 row). Keyed by channel id.
  final Map<String, ChannelData> referencedChannels;
  final ProjectsConnection connection;
  final String? lastError;
  final bool hasRead;

  const ProjectsRead({
    required this.projects,
    required this.referencedChannels,
    required this.connection,
    required this.lastError,
    required this.hasRead,
  });

  const ProjectsRead.initial({
    this.connection = ProjectsConnection.idle,
    this.lastError,
  }) : projects = const [],
       referencedChannels = const {},
       hasRead = false;

  /// The project at [address], or `null`.
  Project? byAddress(String address) {
    for (final project in projects) {
      if (project.address == address) return project;
    }
    return null;
  }

  ProjectsRead copyWith({
    ProjectsConnection? connection,
    String? lastError,
    bool clearError = false,
  }) => ProjectsRead(
    projects: projects,
    referencedChannels: referencedChannels,
    connection: connection ?? this.connection,
    lastError: clearError ? null : (lastError ?? this.lastError),
    hasRead: hasRead,
  );
}

/// The poll fallback for a missed live head.
const projectsPollInterval = Duration(seconds: 30);

/// Reads kind:30621 heads and their kind:5 tombstones, follows live heads,
/// and resolves the channel metadata each head names.
class ProjectsNotifier extends Notifier<ProjectsRead> {
  final List<void Function()> _unsubscribes = [];

  /// The live tombstone subscription, kept apart from [_unsubscribes] because
  /// it is replaced whenever the watched address set changes rather than only
  /// on teardown.
  void Function()? _tombstoneUnsubscribe;

  /// The addresses [_tombstoneUnsubscribe] currently names, so an unchanged
  /// set does not churn the REQ on every poll.
  List<String> _watchedAddresses = const [];
  Timer? _poll;
  int _epoch = 0;
  bool _disposed = false;
  bool _refetchScheduled = false;

  @override
  ProjectsRead build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _teardown();
    });
    if (status != SessionStatus.connected) {
      return (stateOrNull ?? const ProjectsRead.initial()).copyWith(
        connection: status == SessionStatus.connecting
            ? ProjectsConnection.connecting
            : ProjectsConnection.idle,
      );
    }
    Future.microtask(_start);
    return (stateOrNull ?? const ProjectsRead.initial()).copyWith(
      connection: ProjectsConnection.connecting,
    );
  }

  /// Read the heads again.
  Future<void> refresh() async {
    if (ref.read(relaySessionProvider).status != SessionStatus.connected) {
      return;
    }
    await _fetch(_epoch);
  }

  Future<void> _start() async {
    _teardown();
    final epoch = ++_epoch;
    if (_disposed) return;
    final session = ref.read(relaySessionProvider.notifier);
    try {
      final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final unsubscribe = await session.subscribe(
        NostrFilters.projectsLive(now),
        (_) => _scheduleRefetch(epoch),
      );
      if (_stale(epoch)) {
        unsubscribe();
        return;
      }
      _unsubscribes.add(unsubscribe);
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[Projects] live subscribe failed: $error');
    }
    await _fetch(epoch);
    if (_stale(epoch)) return;
    _armPoll(epoch);
  }

  /// First tick at the shared wall-clock phase, then every period.
  void _armPoll(int epoch) {
    _poll?.cancel();
    final delay = alignedPollDelay(
      key: 'mobile-index-poll',
      period: projectsPollInterval,
      pubkey: ref.read(myPubkeyProvider) ?? '',
      now: DateTime.now(),
    );
    _poll = Timer(delay, () {
      if (_stale(epoch)) return;
      _fetch(epoch);
      _poll = Timer.periodic(projectsPollInterval, (_) => _fetch(epoch));
    });
  }

  void _scheduleRefetch(int epoch) {
    if (_refetchScheduled || _stale(epoch)) return;
    _refetchScheduled = true;
    scheduleMicrotask(() {
      _refetchScheduled = false;
      _fetch(epoch);
    });
  }

  Future<void> _fetch(int epoch) async {
    if (_stale(epoch)) return;
    final session = ref.read(relaySessionProvider.notifier);
    try {
      // Heads and tombstones in the same tick: one POST /query carries both.
      final pages = await Future.wait([
        session.query(NostrFilters.projects()),
        session.query(NostrFilters.projectTombstones()),
      ]);
      if (_stale(epoch)) return;
      final projects = projectsFromEvents(pages[0], pages[1]);
      final wanted = <String>{
        for (final project in projects) ...project.channelIds,
      };
      var referenced = <String, ChannelData>{};
      if (wanted.isNotEmpty) {
        final metadata = await session.query(
          NostrFilters.channelMetadata(wanted.toList()),
        );
        if (_stale(epoch)) return;
        referenced = {
          for (final event in metadata)
            if (event.kind == 39000)
              ChannelData.fromEvent(event).id: ChannelData.fromEvent(event),
        };
        referenced.remove('');
      }
      state = ProjectsRead(
        projects: List.unmodifiable(projects),
        referencedChannels: Map.unmodifiable(referenced),
        connection: ProjectsConnection.open,
        lastError: null,
        hasRead: true,
      );
      // The heads just read are the addresses worth watching for a tombstone.
      // Without this a deleted project waits for the 30 s poll; the live head
      // filter cannot see the delete, because a tombstone is a kind:5.
      await _watchTombstones(epoch, [
        for (final project in projects) project.address,
      ]);
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[Projects] read failed: $error');
      state = state.copyWith(
        connection: ProjectsConnection.error,
        lastError: 'Project list read failed: $error',
      );
    }
  }

  /// Follow tombstones for exactly [addresses], replacing any earlier watch.
  ///
  /// Re-subscribing is the cost of scoping by `#a`, and it is the right
  /// trade: the alternative is an unscoped kind:5 REQ carrying every message
  /// deletion in the community, and tombstones for private projects this
  /// reader cannot see.
  Future<void> _watchTombstones(int epoch, List<String> addresses) async {
    if (_stale(epoch)) return;
    final wanted = addresses.toSet().toList()..sort();
    if (_listEquals(wanted, _watchedAddresses)) return;
    _tombstoneUnsubscribe?.call();
    _tombstoneUnsubscribe = null;
    _watchedAddresses = const [];
    if (wanted.isEmpty) return;
    final session = ref.read(relaySessionProvider.notifier);
    try {
      final unsubscribe = await session.subscribe(
        NostrFilters.projectTombstonesLive(wanted),
        (_) => _scheduleRefetch(epoch),
      );
      if (_stale(epoch)) {
        unsubscribe();
        return;
      }
      _tombstoneUnsubscribe = unsubscribe;
      _watchedAddresses = wanted;
    } catch (error) {
      // The poll still catches it, one tick later.
      debugPrint('[Projects] tombstone subscribe failed: \$error');
    }
  }

  static bool _listEquals(List<String> a, List<String> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch;

  void _teardown() {
    _poll?.cancel();
    _poll = null;
    _tombstoneUnsubscribe?.call();
    _tombstoneUnsubscribe = null;
    _watchedAddresses = const [];
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
  }
}

/// When the next poll should fire so that every 30 s index poll on this
/// device ticks at the same wall-clock instant.
///

/// Every project this reader may see, with the channels its heads name.
final projectsProvider = NotifierProvider<ProjectsNotifier, ProjectsRead>(
  ProjectsNotifier.new,
);
