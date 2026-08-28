// Compose the raw app-icon artwork onto the macOS rounded-rect tile, then
// feed the result to `pnpm tauri icon` to regenerate every checked-in size.
//
// The raw master is icons/beekeeper-source.png — a full-bleed 1024×1024
// square. macOS does NOT mask Dock icons (unlike iOS/Android launchers), so
// the squircle must be baked into the artwork: Apple's grid is a 1024 canvas
// with an 824×824 tile centered (100px margins) and ~185px corner radius.
// Tauri's generator only resizes, so without this step the Dock shows a
// hard-cornered full-bleed square.
//
// Regenerate the icon set:
//   swift desktop/scripts/make-icon-tile.swift \
//     desktop/src-tauri/icons/beekeeper-source.png /tmp/beekeeper-tile.png
//   cd desktop && pnpm tauri icon /tmp/beekeeper-tile.png -o src-tauri/icons
import CoreGraphics
import ImageIO
import Foundation
import UniformTypeIdentifiers

guard CommandLine.arguments.count == 3 else {
    FileHandle.standardError.write(Data("usage: make-icon-tile.swift <source.png> <out.png>\n".utf8))
    exit(1)
}
let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL, nil)!
let img = CGImageSourceCreateImageAtIndex(src, 0, nil)!
let ctx = CGContext(data: nil, width: 1024, height: 1024, bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
ctx.addPath(CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil))
ctx.clip()
ctx.interpolationQuality = .high
ctx.draw(img, in: tile)
let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[2]) as CFURL,
                                           UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, ctx.makeImage()!, nil)
guard CGImageDestinationFinalize(dest) else { exit(1) }
