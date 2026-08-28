// Downsample the tray-icon source drawing into the raw alpha mask embedded in
// the desktop binary (tray_menu.rs, tray_hat_icon).
//
// macOS template images render from the alpha channel alone — color is
// ignored and the system tints the glyph for light/dark menu bars — so the
// checked-in artifact is just alpha bytes, row-major, one byte per pixel,
// with the dimensions hardcoded next to the include_bytes! in tray_menu.rs.
// The alpha structure is preserved: the opaque crown against the ~62%-alpha
// veil mesh is what distinguishes the glyph from a solid silhouette.
//
// Regenerate:
//   swift desktop/scripts/make-tray-alpha.swift \
//     desktop/src-tauri/icons/tray-hat-source.png \
//     desktop/src-tauri/icons/tray-hat-alpha.bin 37 43
import CoreGraphics
import ImageIO
import Foundation

guard CommandLine.arguments.count == 5,
      let width = Int(CommandLine.arguments[3]),
      let height = Int(CommandLine.arguments[4]) else {
    FileHandle.standardError.write(Data("usage: make-tray-alpha.swift <source.png> <out.bin> <width> <height>\n".utf8))
    exit(1)
}
let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL, nil)!
let img = CGImageSourceCreateImageAtIndex(src, 0, nil)!

// The source alpha is part of the design and must survive as-is: the hat
// crown and brim are fully opaque while the veil mesh sits near 62% — that
// difference is what makes the glyph read as a veiled hat rather than a
// solid ghost. Template rendering shows partial alpha as partial tint, so
// no thresholding: just a high-quality downsample of the alpha channel.
let alphaOnly = CGBitmapInfo(rawValue: CGImageAlphaInfo.alphaOnly.rawValue)
let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width,
                    space: nil, bitmapInfo: alphaOnly)!
ctx.interpolationQuality = .high
ctx.draw(img, in: CGRect(x: 0, y: 0, width: width, height: height))
// CGBitmapContext memory is already top-down row-major — exactly what the
// pixel loop in tray_menu.rs expects — so write the buffer as-is. (Flipping
// here shipped an upside-down menu-bar hat once already.)
let out = Data(bytes: ctx.data!, count: width * height)
try! out.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
