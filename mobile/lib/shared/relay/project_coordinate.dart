// Project (kind:30621) coordinates as the relay spells them.
//
// A port of `beekeeper_core::kind::normalize_project_coordinate`, shared by
// every feature that folds project-scoped ops (to-dos, agents-repository
// drafts) so no feature imports another for it.

final _anyHex64 = RegExp(r'^[0-9a-fA-F]{64}$');

bool _isControl(int rune) => rune < 0x20 || (rune >= 0x7f && rune <= 0x9f);

/// The canonical `30621:<lowercase-hex>:<dtag>` spelling of a project
/// coordinate, or `null` when [value] is not one. Hex case is folded; the
/// dtag is kept as written, must be non-empty, at most 64 characters, and
/// free of control characters.
String? normalizeProjectCoordinate(String value) {
  final first = value.indexOf(':');
  if (first < 0) return null;
  final second = value.indexOf(':', first + 1);
  if (second < 0) return null;
  final kind = value.substring(0, first);
  final pubkey = value.substring(first + 1, second);
  final dtag = value.substring(second + 1);
  if (kind != '30621') return null;
  if (!_anyHex64.hasMatch(pubkey)) return null;
  if (dtag.isEmpty || dtag.runes.length > 64 || dtag.runes.any(_isControl)) {
    return null;
  }
  return '30621:${pubkey.toLowerCase()}:$dtag';
}
