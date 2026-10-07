import 'package:beekeeper/features/channels/channel_management_provider.dart';
import 'package:beekeeper/shared/relay/relay_provider.dart';
import 'package:flutter_test/flutter_test.dart';

/// Compile-time defines cannot be changed inside one test run, so this test
/// asserts against what the run was given. The plain `flutter test` run proves
/// the default. The legacy and precedence cases need their own runs:
///
///   flutter test test/shared/relay/env_legacy_define_test.dart \
///     --dart-define=BUZZ_RELAY_URL=http://legacy:1 \
///     --dart-define=BUZZ_MOCK_DM_DIRECTORY=true
///   flutter test test/shared/relay/env_legacy_define_test.dart \
///     --dart-define=BUZZ_RELAY_URL=http://legacy:1 \
///     --dart-define=BEEKEEPER_RELAY_URL=http://new:2 \
///     --dart-define=BUZZ_MOCK_DM_DIRECTORY=true \
///     --dart-define=BEEKEEPER_MOCK_DM_DIRECTORY=false
const _given = String.fromEnvironment('BEEKEEPER_RELAY_URL');
const _legacy = String.fromEnvironment('BUZZ_RELAY_URL');
const _givenMock = String.fromEnvironment('BEEKEEPER_MOCK_DM_DIRECTORY');
const _legacyMock = String.fromEnvironment('BUZZ_MOCK_DM_DIRECTORY');

void main() {
  test('relay URL: BEEKEEPER_ wins, then BUZZ_, then the default', () {
    final expected = _given.isNotEmpty
        ? _given
        : _legacy.isNotEmpty
        ? _legacy
        : 'http://localhost:3000';
    expect(Env.relayUrl, expected);
  });

  test('mock DM directory: BEEKEEPER_ wins, then BUZZ_', () {
    final raw = _givenMock.isNotEmpty ? _givenMock : _legacyMock;
    // Debug-only switch: flutter test runs in debug mode.
    expect(mockDmDirectoryEnabled, raw == 'true');
  });
}
