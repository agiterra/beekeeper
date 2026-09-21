import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import '../../../shared/relay/relay_nip98.dart';
import '../domain/agents_repo_draft_op.dart';

/// One entry of a tree listing.
@immutable
class AgentsRepoEntry {
  final String path;

  /// `blob`, `tree` or `commit`.
  final String kind;
  final String oid;
  final int? size;

  const AgentsRepoEntry({
    required this.path,
    required this.kind,
    required this.oid,
    required this.size,
  });
}

/// The listing of a ref.
@immutable
class AgentsRepoListing {
  /// The commit the ref resolved to.
  final String commit;
  final List<AgentsRepoEntry> entries;

  /// When this device read it.
  final DateTime fetchedAt;

  const AgentsRepoListing({
    required this.commit,
    required this.entries,
    required this.fetchedAt,
  });

  AgentsRepoEntry? entryAt(String path) {
    for (final entry in entries) {
      if (entry.path == path) return entry;
    }
    return null;
  }
}

/// One file as `main` has it.
@immutable
class AgentsRepoFile {
  final String path;

  /// The text; `null` when the file is not on `main`, is not UTF-8 text,
  /// or is over the draft cap — [state] says which.
  final String? text;

  /// `on-main`, `not-on-main`, `not-text`, `too-large`.
  final String state;
  final String? blob;
  final String? commit;
  final DateTime fetchedAt;

  const AgentsRepoFile({
    required this.path,
    required this.text,
    required this.state,
    required this.blob,
    required this.commit,
    required this.fetchedAt,
  });
}

/// A failed read, with the relay's status and words.
class AgentsRepoReadException implements Exception {
  final int statusCode;
  final String message;
  const AgentsRepoReadException(this.statusCode, this.message);

  @override
  String toString() => 'relay $statusCode: $message';
}

/// Reads a relay-hosted repository's `main` through the relay's `tree` and
/// `raw` routes (NIP-AD § Reading the tip without git), signed with a
/// NIP-98 token bound to the repository root — the same `u` the git
/// credential helper signs, which is what the relay's git extractor
/// verifies every git route against.
class AgentsRepoHttpClient {
  final String baseUrl;
  final String? nsec;
  final http.Client _client;
  final DateTime Function() _now;

  AgentsRepoHttpClient({
    required this.baseUrl,
    required this.nsec,
    http.Client? client,
    DateTime Function()? now,
  }) : _client = client ?? http.Client(),
       _now = now ?? DateTime.now;

  String _root(String owner, String id) =>
      Uri.parse(baseUrl).resolve('/git/$owner/$id').toString();

  Future<http.Response> _get(String owner, String id, String tail) async {
    final root = _root(owner, id);
    final url = '$root/$tail';
    final response = await _client
        .get(
          Uri.parse(url),
          headers: {
            'Authorization': buildNip98AuthHeader(
              method: 'GET',
              url: root,
              bodyBytes: const [],
              nsec: nsec,
            ),
          },
        )
        .timeout(const Duration(seconds: 20));
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw AgentsRepoReadException(response.statusCode, response.body);
    }
    return response;
  }

  /// Every blob under `main`, recursively.
  Future<AgentsRepoListing> listMain(String owner, String id) async {
    final response = await _get(owner, id, 'tree/refs/heads/main');
    final decoded = jsonDecode(utf8.decode(response.bodyBytes));
    if (decoded is! Map<String, dynamic>) {
      throw const FormatException('tree listing is not an object');
    }
    final entries = <AgentsRepoEntry>[
      for (final raw in (decoded['entries'] as List<dynamic>? ?? const []))
        if (raw is Map<String, dynamic>)
          AgentsRepoEntry(
            path: raw['path'] as String,
            kind: raw['kind'] as String,
            oid: raw['oid'] as String,
            size: (raw['size'] as num?)?.toInt(),
          ),
    ];
    return AgentsRepoListing(
      commit: decoded['commit'] as String,
      entries: entries,
      fetchedAt: _now(),
    );
  }

  /// One file at `main`'s tip.
  Future<AgentsRepoFile> readMain(String owner, String id, String path) async {
    http.Response response;
    try {
      response = await _get(owner, id, 'raw/refs/heads/main/$path');
    } on AgentsRepoReadException catch (error) {
      if (error.statusCode == 404 && error.message.contains('path not found')) {
        return AgentsRepoFile(
          path: path,
          text: null,
          state: 'not-on-main',
          blob: null,
          commit: null,
          fetchedAt: _now(),
        );
      }
      rethrow;
    }
    final bytes = response.bodyBytes;
    final commit = response.headers['x-git-commit'];
    final blob = response.headers['x-git-blob'];
    if (bytes.length > maxAgentsRepoDraftTextBytes) {
      return AgentsRepoFile(
        path: path,
        text: null,
        state: 'too-large',
        blob: blob,
        commit: commit,
        fetchedAt: _now(),
      );
    }
    String text;
    try {
      text = utf8.decode(bytes);
    } on FormatException {
      return AgentsRepoFile(
        path: path,
        text: null,
        state: 'not-text',
        blob: blob,
        commit: commit,
        fetchedAt: _now(),
      );
    }
    return AgentsRepoFile(
      path: path,
      text: text,
      state: 'on-main',
      blob: blob,
      commit: commit,
      fetchedAt: _now(),
    );
  }
}
