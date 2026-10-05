import 'dart:async';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter_test/flutter_test.dart';

/// Runs before every test file under `test/`: swaps the golden comparator for
/// one that ignores glyph anti-aliasing noise between Macs.
///
/// The goldens are compared on macOS only, and two Macs on the same pinned
/// Flutter still rasterise glyph edges differently: SV-31's and SV-36's six
/// goldens differed on a second machine by 0.30–0.48% of pixels, every one
/// on a glyph edge, no channel off by more than 40 of 255, and at most 63
/// pixels per image off by more than 16. Neither bound alone separates that
/// from a real change: swapping a colour for its neighbouring theme token
/// moves each pixel about as far as the noise does, but moves hundreds of
/// them.
Future<void> testExecutable(FutureOr<void> Function() testMain) async {
  final base = goldenFileComparator;
  if (base is LocalFileComparator) {
    goldenFileComparator = RasterTolerantComparator(
      // LocalFileComparator resolves goldens against the test file's
      // directory; any file name in that directory reproduces it.
      base.basedir.resolve('golden_test.dart'),
    );
  }
  await testMain();
}

/// A [LocalFileComparator] that passes when no pixel differs from the golden
/// by more than [channelTolerance] in any channel, and no more than
/// [noisyPixelLimit] pixels differ by more than [noiseFloor].
///
/// On a failure it defers to [LocalFileComparator.compare], so the failure
/// message and the `failures/` diff images are the stock ones.
class RasterTolerantComparator extends LocalFileComparator {
  RasterTolerantComparator(super.testFile);

  /// Largest per-channel difference (0–255) treated as rasterisation noise.
  /// Measured noise peaks at 40; a glyph against its background differs by
  /// well over 100.
  static const int channelTolerance = 64;

  /// Per-channel difference above which a pixel counts toward
  /// [noisyPixelLimit].
  static const int noiseFloor = 16;

  /// Most pixels allowed above [noiseFloor]. Measured noise peaks at 63; a
  /// label's text colour or a chip's fill moves several hundred.
  static const int noisyPixelLimit = 150;

  @override
  Future<bool> compare(Uint8List imageBytes, Uri golden) async {
    final goldenBytes = Uint8List.fromList(await getGoldenBytes(golden));
    final test = await _rgba(imageBytes);
    final master = await _rgba(goldenBytes);
    if (test.width == master.width &&
        test.height == master.height &&
        _withinTolerance(test.pixels, master.pixels)) {
      return true;
    }
    return super.compare(imageBytes, golden);
  }

  static bool _withinTolerance(Uint8List a, Uint8List b) {
    var noisy = 0;
    for (var px = 0; px < a.length; px += 4) {
      var worst = 0;
      for (var c = px; c < px + 4; c++) {
        final d = (a[c] - b[c]).abs();
        if (d > worst) worst = d;
      }
      if (worst > channelTolerance) return false;
      if (worst > noiseFloor && ++noisy > noisyPixelLimit) return false;
    }
    return true;
  }

  static Future<({int width, int height, Uint8List pixels})> _rgba(
    Uint8List png,
  ) async {
    final codec = await ui.instantiateImageCodec(png);
    final image = (await codec.getNextFrame()).image;
    final data = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
    final result = (
      width: image.width,
      height: image.height,
      pixels: data!.buffer.asUint8List(),
    );
    image.dispose();
    codec.dispose();
    return result;
  }
}
