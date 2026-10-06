import 'coding_session_commands.dart' show maxCodingSessionNameBytes;
import 'coding_session_keys.dart';
import 'coding_session_target.dart';

/// Wire constants and strict checks for kind 44252, the provider-signed
/// generated title (NIP-CSG § Generated title).
///
/// Mirror of `crates/beekeeper-core/src/coding_session_title.rs`; the shared
/// `conformance/session-display-name/` vectors are what keep the two honest.

/// Exact version carried by the `cstl-v` tag.
const codingSessionTitleTagVersion = 'cstl1-1';

/// Exact `schema` of a generated-title payload.
const codingSessionTitleSchema = 'buzz-coding-session-title/v1';

/// The one `basis` v1 knows: the founder's first message, and nothing else.
const codingSessionTitleBasisFirstMessage = 'first-message';

/// Maximum UTF-8 byte length of a generated-title event's content.
const maxCodingSessionTitleContentBytes = 2048;

/// Maximum UTF-8 byte length of the payload's `model`.
const maxCodingSessionTitleModelBytes = 128;

/// What a reader shows when nothing names the session.
///
/// The shared rule's last fallback (`UNTITLED_SESSION_NAME` in buzz-core); the
/// vectors' `constants.untitled` is asserted against it.
const codingSessionUntitledName = 'Untitled session';

/// Largest identity field a `cs-target` may carry, as a 44220 command's
/// target is bounded in buzz-core (`MAX_IDENTIFIER_BYTES`).
const _maxTargetIdentifierBytes = 256;

/// Largest generation a JSON number can carry exactly (`MAX_SAFE_GENERATION`).
const _maxSafeGeneration = 9007199254740991;

/// True when [value] is a session name under the 44229 content rule: some
/// non-whitespace text, one line, at most 256 UTF-8 bytes.
bool isCodingSessionNameText(Object? value) =>
    value is String &&
    value.trim().isNotEmpty &&
    utf8ByteLength(value) <= maxCodingSessionNameBytes &&
    !value.contains('\n') &&
    !value.contains('\r');

/// True when [value] holds a Unicode control character (category Cc), the
/// set Rust's `char::is_control` answers for.
bool hasControlCharacter(String value) =>
    value.runes.any((rune) => rune <= 0x1f || (rune >= 0x7f && rune <= 0x9f));

final _hyphenatedUuid = RegExp(
  r'^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$',
);
final _simpleUuid = RegExp(r'^[0-9a-fA-F]{32}$');

/// True for any text the `uuid` crate's `Uuid::parse_str` accepts: hyphenated,
/// simple, braced, or `urn:uuid:` — the rule buzz-core applies to an `h` tag.
bool isUuidText(String value) {
  if (_hyphenatedUuid.hasMatch(value) || _simpleUuid.hasMatch(value)) {
    return true;
  }
  if (value.length == 38 && value.startsWith('{') && value.endsWith('}')) {
    return _hyphenatedUuid.hasMatch(value.substring(1, 37));
  }
  if (value.length == 45 && value.startsWith('urn:uuid:')) {
    return _hyphenatedUuid.hasMatch(value.substring(9));
  }
  return false;
}

/// True when [key] is a `cs-target` buzz-core would accept on a 44252: a
/// `coding-session/v1` key that re-encodes to exactly itself, three non-blank
/// identity fields of at most 256 bytes with no control characters, and a
/// positive safe-integer generation.
bool isCodingSessionTitleTargetKey(String key) {
  final target = CodingSessionTarget.fromKey(key);
  if (target == null || target.key != key) return false;
  for (final field in [target.driver, target.instanceId, target.sessionId]) {
    if (field.trim().isEmpty ||
        utf8ByteLength(field) > _maxTargetIdentifierBytes ||
        hasControlCharacter(field)) {
      return false;
    }
  }
  return target.generation > 0 && target.generation <= _maxSafeGeneration;
}
