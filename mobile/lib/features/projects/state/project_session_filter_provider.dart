import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme_provider.dart';
import '../domain/project_session_filter.dart';

/// Per-project session filters, persisted on this device keyed by
/// `(pubkey, relay)` — the desktop keeps them in localStorage the same way,
/// so a filter is a per-device, per-identity preference and nothing on the
/// wire.
@immutable
class ProjectSessionFilters {
  final Map<String, ProjectSessionFilter> byProject;

  const ProjectSessionFilters(this.byProject);

  ProjectSessionFilter forProject(String address) =>
      byProject[address] ?? ProjectSessionFilter.defaults;
}

/// Reads and writes the filters for the active identity on this relay.
class ProjectSessionFiltersNotifier extends Notifier<ProjectSessionFilters> {
  String? _key;

  @override
  ProjectSessionFilters build() {
    final config = ref.watch(relayConfigProvider);
    final pubkey = ref.watch(myPubkeyProvider);
    _key = 'project-session-filters:${pubkey ?? 'anon'}:${config.baseUrl}';
    return _read();
  }

  ProjectSessionFilters _read() {
    final key = _key;
    if (key == null) return const ProjectSessionFilters({});
    try {
      final raw = ref.read(savedPrefsProvider).getString(key);
      if (raw == null) return const ProjectSessionFilters({});
      final decoded = jsonDecode(raw);
      if (decoded is! Map) return const ProjectSessionFilters({});
      return ProjectSessionFilters({
        for (final entry in decoded.entries)
          if (entry.key is String)
            entry.key as String: ProjectSessionFilter.fromJson(entry.value),
      });
    } catch (error) {
      debugPrint('[ProjectSessionFilters] unreadable, using defaults: $error');
      return const ProjectSessionFilters({});
    }
  }

  /// Set [filter] for the project at [address] and persist it.
  Future<void> setFilter(String address, ProjectSessionFilter filter) async {
    final next = {...state.byProject, address: filter};
    state = ProjectSessionFilters(Map.unmodifiable(next));
    final key = _key;
    if (key == null) return;
    try {
      await ref
          .read(savedPrefsProvider)
          .setString(
            key,
            jsonEncode({
              for (final entry in next.entries) entry.key: entry.value.toJson(),
            }),
          );
    } catch (error) {
      debugPrint('[ProjectSessionFilters] save failed: $error');
    }
  }
}

/// The session filter per project, for the active identity on this device.
final projectSessionFiltersProvider =
    NotifierProvider<ProjectSessionFiltersNotifier, ProjectSessionFilters>(
      ProjectSessionFiltersNotifier.new,
    );
