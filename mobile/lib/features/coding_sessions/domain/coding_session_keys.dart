import 'dart:convert';

/// Domain-separated, UTF-8 byte-length-prefixed key serialization.
///
/// Mirror of the desktop's `encodeStructuredKey`
/// (`desktop/src/features/coding-sessions/lib/codingSessionKeys.ts`). Every
/// identity string in the coding-session observer goes through this, so a
/// field value that happens to contain a delimiter can never make two distinct
/// tuples serialize identically. The provider, the desktop and this observer
/// all mint the same bytes for the same tuple.
String encodeStructuredKey(String domain, List<String> fields) {
  final buffer = StringBuffer(domain)..write('|');
  for (final field in fields) {
    buffer
      ..write(utf8ByteLength(field))
      ..write(':')
      ..write(field);
  }
  return buffer.toString();
}

/// UTF-8 byte length, matching Rust's `str::len()` and JS `TextEncoder`.
int utf8ByteLength(String value) => utf8.encode(value).length;

/// The UTF-8 bytes of [value].
List<int> utf8Bytes(String value) => utf8.encode(value);

/// Decode UTF-8 [bytes], or the replacement-character rendering of invalid
/// input. Never throws: these bytes come off a relay.
String decodeUtf8Bytes(List<int> bytes) =>
    const Utf8Decoder(allowMalformed: true).convert(bytes);

/// The wire domain for a `cs-target` tag value.
const codingSessionTargetKeyDomain = 'coding-session/v1';

/// The wire domain for a 44224 `csl-key` semantic key.
const codingSessionReceiptKeyDomain = 'coding-session-lifecycle-receipt/v1';

/// The wire domain for a 44223 `csm-key` semantic key.
const codingSessionMetadataKeyDomain = 'coding-session-metadata/v1';

/// The wire domain for a 44225 `cst-key` semantic key.
const codingSessionTranscriptKeyDomain = 'coding-session-transcript/v1';
