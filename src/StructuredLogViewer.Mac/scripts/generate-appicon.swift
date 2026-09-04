#!/usr/bin/env swift
//
// Regenerates App/Assets.xcassets/AppIcon.appiconset from source.
//
//   swift scripts/generate-appicon.swift
//
// The motif is the project's brand mark (docs/StructuredLogger.png, and the
// Windows/Avalonia icons): a parent node with a trunk down to two rows of
// child nodes — the structured log tree. Those only exist at 32-64px, far too
// small for a Mac app icon, so this redraws them at full resolution with the
// colours sampled from the original.
//
// Two tiles are defined, dark and light; the dark one ships. To see the
// other, change `shipping` below and re-run.
import AppKit
import Foundation

// Brand motif, sampled from docs/StructuredLogger.png: a parent node with a
// trunk down to two rows of child nodes — the structured log tree.
let parent  = NSColor(srgbRed: 216/255, green: 178/255, blue: 254/255, alpha: 1)
let blue    = NSColor(srgbRed:  35/255, green: 127/255, blue: 254/255, alpha: 1)
let green   = NSColor(srgbRed: 122/255, green: 230/255, blue: 145/255, alpha: 1)
let cyan    = NSColor(srgbRed:  25/255, green: 177/255, blue: 254/255, alpha: 1)
let yellow  = NSColor(srgbRed: 228/255, green: 254/255, blue:  44/255, alpha: 1)

struct Theme {
    let name: String
    let top: NSColor
    let bottom: NSColor
    let line: NSColor
    let outline: NSColor?
}

let themes = [
    Theme(name: "dark",
          top: NSColor(srgbRed: 0.239, green: 0.267, blue: 0.325, alpha: 1),
          bottom: NSColor(srgbRed: 0.098, green: 0.114, blue: 0.149, alpha: 1),
          line: NSColor(srgbRed: 0.85, green: 0.87, blue: 0.92, alpha: 1),
          outline: nil),
    Theme(name: "light",
          top: NSColor(srgbRed: 1.0, green: 1.0, blue: 1.0, alpha: 1),
          bottom: NSColor(srgbRed: 0.898, green: 0.914, blue: 0.941, alpha: 1),
          line: NSColor(srgbRed: 0.09, green: 0.10, blue: 0.13, alpha: 1),
          outline: NSColor(srgbRed: 0.09, green: 0.10, blue: 0.13, alpha: 1)),
]

/// Draws the icon at `size` points into a bitmap.
func render(theme: Theme, size: CGFloat) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    ctx.setShouldAntialias(true)
    ctx.interpolationQuality = .high

    let u = size / 1024   // everything below is authored in 1024-point space

    // macOS icon geometry: an 824pt tile centred in a 1024pt canvas, so the
    // grid alignment and drop shadow match every other app in the Dock.
    let tile = NSRect(x: 100 * u, y: 100 * u, width: 824 * u, height: 824 * u)
    let tilePath = NSBezierPath(roundedRect: tile, xRadius: 185 * u, yRadius: 185 * u)

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10 * u), blur: 22 * u,
                  color: NSColor.black.withAlphaComponent(0.28).cgColor)
    NSColor.black.setFill()
    tilePath.fill()
    ctx.restoreGState()

    ctx.saveGState()
    tilePath.addClip()
    NSGradient(starting: theme.top, ending: theme.bottom)?
        .draw(in: tile, angle: -90)
    ctx.restoreGState()

    // Motif: authored in a 72-unit box, scaled to fill 512pt of the tile.
    let motif: CGFloat = 512 * u
    let scale = motif / 72
    let originX = tile.midX - motif / 2
    let originY = tile.midY + motif / 2   // flip: author top-down

    func box(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> NSRect {
        NSRect(x: originX + x * scale, y: originY - (y + h) * scale,
               width: w * scale, height: h * scale)
    }

    // Trunk and the two stubs reaching each row, drawn under the nodes.
    theme.line.setFill()
    let lineW: CGFloat = 4
    NSBezierPath(roundedRect: box(6, 18, lineW, 46), xRadius: 1 * scale, yRadius: 1 * scale).fill()
    NSBezierPath(roundedRect: box(6, 34, 20, lineW), xRadius: 1 * scale, yRadius: 1 * scale).fill()
    NSBezierPath(roundedRect: box(6, 60, 20, lineW), xRadius: 1 * scale, yRadius: 1 * scale).fill()

    // Nodes: parent, then the 2x2 grid of children.
    let sq: CGFloat = 20
    let nodes: [(NSRect, NSColor)] = [
        (box(0, 0, sq, sq), parent),
        (box(26, 26, sq, sq), blue),
        (box(52, 26, sq, sq), green),
        (box(26, 52, sq, sq), cyan),
        (box(52, 52, sq, sq), yellow),
    ]

    for (rect, color) in nodes {
        let path = NSBezierPath(roundedRect: rect, xRadius: 3.5 * scale, yRadius: 3.5 * scale)
        // A slight vertical lift, like the original's shaded squares.
        NSGradient(starting: color.blended(withFraction: 0.18, of: .white) ?? color,
                   ending: color.blended(withFraction: 0.10, of: .black) ?? color)?
            .draw(in: path, angle: -90)
        if let outline = theme.outline {
            outline.setStroke()
            path.lineWidth = 2.2 * scale
            path.stroke()
        }
    }

    NSGraphicsContext.restoreGraphicsState()
    return rep
}

/// Which of `themes` ships.
let shipping = "dark"

// scripts/ -> the app target's asset catalog.
let scriptDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let iconset = scriptDir
    .deletingLastPathComponent()
    .appendingPathComponent("App/Assets.xcassets/AppIcon.appiconset")
try! FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

// One PNG per slot. actool deduplicates by filename, so slots that share a
// file (16x16@2x and 32x32, say) are not reliably both emitted — hence the
// duplicated pixel sizes under different names.
let slots: [(pt: Int, scale: String, px: CGFloat)] = [
    (16, "1x", 16), (16, "2x", 32),
    (32, "1x", 32), (32, "2x", 64),
    (128, "1x", 128), (128, "2x", 256),
    (256, "1x", 256), (256, "2x", 512),
    (512, "1x", 512), (512, "2x", 1024),
]

func write(_ rep: NSBitmapImageRep, to url: URL) {
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

guard let theme = themes.first(where: { $0.name == shipping }) else {
    fatalError("no theme named \(shipping)")
}

var entries: [String] = []
for slot in slots {
    let name = "icon_\(slot.pt)x\(slot.pt)\(slot.scale == "2x" ? "@2x" : "").png"
    write(render(theme: theme, size: slot.px), to: iconset.appendingPathComponent(name))
    entries.append("""
        {
          "idiom" : "mac",
          "size" : "\(slot.pt)x\(slot.pt)",
          "scale" : "\(slot.scale)",
          "filename" : "\(name)"
        }
    """)
}

let contents = """
{
  "images" : [
\(entries.joined(separator: ",\n"))
  ],
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}

"""
try! contents.write(to: iconset.appendingPathComponent("Contents.json"), atomically: true, encoding: .utf8)

print("wrote \(slots.count) slots to \(iconset.path) using the \(shipping) tile")
