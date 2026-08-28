// Downsample the tray-icon source drawing into the raw alpha mask embedded in
// the desktop binary (tray_menu.rs, tray_hat_icon).
//
// macOS template images render from the alpha channel alone — color is
// ignored and the system tints the glyph for light/dark menu bars — so the
// checked-in artifact is just alpha bytes, row-major, one byte per pixel,
// with the dimensions hardcoded next to the include_bytes! in tray_menu.rs.
// The source's outline detail is deliberately dropped: at menu-bar size the
// line art collapses, so the mask is the solid silhouette.
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

// The source drawing's fill is only partially opaque, which downsamples to a
// washed-out, blotchy glyph. Binarize alpha at full resolution — any visible
// coverage becomes fully opaque — so after downsampling only the silhouette
// edges carry partial alpha (antialiasing), not the interior.
let alphaOnly = CGBitmapInfo(rawValue: CGImageAlphaInfo.alphaOnly.rawValue)
let full = CGContext(data: nil, width: img.width, height: img.height, bitsPerComponent: 8,
                     bytesPerRow: img.width, space: nil, bitmapInfo: alphaOnly)!
full.draw(img, in: CGRect(x: 0, y: 0, width: img.width, height: img.height))
let fullBuf = full.data!.assumingMemoryBound(to: UInt8.self)
for i in 0..<(img.width * img.height) { fullBuf[i] = fullBuf[i] >= 40 ? 255 : 0 }
let solid = full.makeImage()!

let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width,
                    space: nil, bitmapInfo: alphaOnly)!
ctx.interpolationQuality = .high
ctx.draw(solid, in: CGRect(x: 0, y: 0, width: width, height: height))
// CGBitmapContext memory is already top-down row-major — exactly what the
// pixel loop in tray_menu.rs expects — so write the buffer as-is. (Flipping
// here shipped an upside-down menu-bar hat once already.)
let out = Data(bytes: ctx.data!, count: width * height)
try! out.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
