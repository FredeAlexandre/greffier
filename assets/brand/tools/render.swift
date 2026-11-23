// Renders Greffier's brand assets from the SVG marks in `sources/` (written by `marks.py`).
//
//   swift assets/brand/tools/render.swift
//
// Writes, next to `sources/`:
//   app-icon/   Greffier.iconset (every macOS size, each one rendered at its own size) and Greffier.icns
//   logo/       the mark alone in each colour (SVG and 1024 px PNG) and the mark on its blue tile
//   menubar/    the template images for the menu bar, idle (eyes closed) and recording (eyes open)
//
// The SVGs are drawn by AppKit (NSImage reads SVG since macOS 14), recoloured by swapping their fill.

import AppKit
import Foundation

let brand = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let sources = brand.appendingPathComponent("sources")

// MARK: - Colours

struct Colour { let hex: String; let r, g, b: CGFloat
    init(_ hex: String) {
        self.hex = hex
        let v = UInt32(hex.dropFirst(), radix: 16)!
        r = CGFloat((v >> 16) & 255) / 255; g = CGFloat((v >> 8) & 255) / 255; b = CGFloat(v & 255) / 255
    }
    var cg: CGColor { CGColor(srgbRed: r, green: g, blue: b, alpha: 1) }
}

let blue = Colour("#3D4FD8"), blueTop = Colour("#4A5CE4"), blueBottom = Colour("#3242C6")
let cream = Colour("#F4EFE6"), ink = Colour("#14161F")

// MARK: - Helpers

func svg(_ name: String, colour: Colour) -> String {
    let text = try! String(contentsOf: sources.appendingPathComponent("\(name).svg"), encoding: .utf8)
    precondition(text.contains("fill=\"#000000\""), "\(name).svg: expected a single black fill")
    return text.replacingOccurrences(of: "fill=\"#000000\"", with: "fill=\"\(colour.hex)\"")
}

func image(_ name: String, colour: Colour) -> NSImage {
    guard let img = NSImage(data: Data(svg(name, colour: colour).utf8)) else { fatalError("cannot read \(name).svg") }
    return img
}

func canvas(_ w: Int, _ h: Int) -> CGContext {
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    return ctx
}

/// Draws the 256 × 256 SVG canvas into `rect` (CG coordinates, bottom-left origin).
func draw(_ img: NSImage, in rect: CGRect, on ctx: CGContext) {
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
    img.draw(in: rect)
    NSGraphicsContext.restoreGraphicsState()
}

func save(_ ctx: CGContext, _ path: String) {
    let url = brand.appendingPathComponent(path)
    try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

// MARK: - App icon (macOS grid: 824 px body with continuous corners in a 1024 px canvas)

func squircle(_ r: CGRect) -> CGPath {
    let p = CGMutablePath(), n = 5.0, steps = 360
    for k in 0...steps {
        let t = Double(k) / Double(steps) * 2 * .pi, c = cos(t), s = sin(t)
        let x = r.midX + r.width / 2 * CGFloat(copysign(pow(abs(c), 2 / n), c))
        let y = r.midY + r.height / 2 * CGFloat(copysign(pow(abs(s), 2 / n), s))
        k == 0 ? p.move(to: CGPoint(x: x, y: y)) : p.addLine(to: CGPoint(x: x, y: y))
    }
    p.closeSubpath()
    return p
}

/// The blue tile with the cream mark; `body` is the tile's square, `markScale` the SVG canvas side / tile side.
func tile(_ ctx: CGContext, body: CGRect, path: CGPath?, mark: NSImage, markScale: CGFloat = 0.69) {
    ctx.saveGState()
    if let path { ctx.addPath(path); ctx.clip() }
    let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
                              colors: [blueTop.cg, blueBottom.cg] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: CGPoint(x: body.midX, y: body.maxY), end: CGPoint(x: body.midX, y: body.minY), options: [])
    let side = body.width * markScale
    draw(mark, in: CGRect(x: body.midX - side / 2, y: body.midY - side / 2, width: side, height: side), on: ctx)
    ctx.restoreGState()
}

let markLarge = image("greffier-mark", colour: cream)
let markSmall = image("greffier-mark-small", colour: cream)

func appIcon(_ px: Int) -> CGContext {
    let ctx = canvas(px, px), s = CGFloat(px) / 1024
    let body = CGRect(x: 100 * s, y: 100 * s, width: 824 * s, height: 824 * s)
    let path = squircle(body)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10 * s), blur: 24 * s, color: CGColor(gray: 0, alpha: 0.35))
    ctx.addPath(path); ctx.setFillColor(blue.cg); ctx.fillPath()
    ctx.restoreGState()
    // At 32 px and below, the small-size cut: wider gap under the head, thicker eyes.
    tile(ctx, body: body, path: path, mark: px <= 32 ? markSmall : markLarge, markScale: px <= 32 ? 0.76 : 0.69)
    return ctx
}

let iconset = [(16, "16x16"), (32, "16x16@2x"), (32, "32x32"), (64, "32x32@2x"), (128, "128x128"),
               (256, "128x128@2x"), (256, "256x256"), (512, "256x256@2x"), (512, "512x512"), (1024, "512x512@2x")]
for (px, name) in iconset { save(appIcon(px), "app-icon/Greffier.iconset/icon_\(name).png") }
save(appIcon(1024), "app-icon/greffier-app-icon-1024.png")

let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", brand.appendingPathComponent("app-icon/Greffier.iconset").path,
                      "-o", brand.appendingPathComponent("app-icon/Greffier.icns").path]
try! iconutil.run(); iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else { fatalError("iconutil failed") }

// MARK: - Logo

for (name, colour) in [("blue", blue), ("ink", ink), ("cream", cream), ("white", Colour("#FFFFFF")), ("black", Colour("#000000"))] {
    let text = svg("greffier-mark", colour: colour)
    try! text.write(to: brand.appendingPathComponent("logo/greffier-mark-\(name).svg"), atomically: true, encoding: .utf8)
    let ctx = canvas(1024, 1024)
    draw(image("greffier-mark", colour: colour), in: CGRect(x: 0, y: 0, width: 1024, height: 1024), on: ctx)
    save(ctx, "logo/greffier-mark-\(name)-1024.png")
}
do {
    let ctx = canvas(1024, 1024)
    tile(ctx, body: CGRect(x: 0, y: 0, width: 1024, height: 1024), path: nil, mark: markLarge, markScale: 0.62)
    save(ctx, "logo/greffier-tile-1024.png")
}

// MARK: - Menu bar template images (18 pt; black + alpha, macOS tints them)

// The small-size cut, scaled so that the mark's width (216 of the 256 canvas) fills the 18 pt square.
for (state, source) in [("Idle", "greffier-mark-small"), ("Recording", "greffier-mark-small-recording")] {
    for (scale, suffix) in [(1, ""), (2, "@2x")] {
        let px = 18 * scale, ctx = canvas(px, px)
        let side = CGFloat(px) * 0.98 * 256 / 216
        // The mark spans y 32…216 of the canvas: centre it vertically (CG y grows upwards).
        let centreY = CGFloat(256 - 124) / 256 * side
        draw(image(source, colour: Colour("#000000")),
             in: CGRect(x: CGFloat(px) / 2 - side / 2, y: CGFloat(px) / 2 - centreY, width: side, height: side), on: ctx)
        save(ctx, "menubar/Menubar\(state)Template\(suffix).png")
    }
}

print("rendered into \(brand.path)")
