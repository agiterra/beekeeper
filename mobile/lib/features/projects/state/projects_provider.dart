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
    _poll = Timer.periodic(projectsPollInterval, (_) => _fetch(epoch));
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
      final heads = await session.fetchHistory(NostrFilters.projects());
      if (_stale(epoch)) return;
      final tombstones = await session.fetchHistory(
        NostrFilters.projectTombstones(),
      );
      if (_stale(epoch)) return;
      final projects = projectsFromEvents(heads, tombstones);
      final wanted = <String>{
        for (final project in projects) ...project.channelIds,
      };
      var referenced = <String, ChannelData>{};
      if (wanted.isNotEmpty) {
        final metadata = await session.fetchHistory(
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
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[Projects] read failed: $error');
      state = state.copyWith(
        connection: ProjectsConnection.error,
        lastError: 'Project list read failed: $error',
      );
    }
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch;

  void _teardown() {
    _poll?.cancel();
    _poll = null;
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
  }
}

/// Every project this reader may see, with the channels its heads name.
final projectsProvider = NotifierProvider<ProjectsNotifier, ProjectsRead>(
  ProjectsNotifier.new,
);
