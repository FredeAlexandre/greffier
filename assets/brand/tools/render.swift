// Renders Greffier's brand assets from the three images in `sources/`.
//
//   swift assets/brand/tools/render.swift
//
// Writes, next to `sources/`:
//   app-icon/   Greffier.iconset (every macOS size, each one rendered at its own size) and Greffier.icns
//   logo/       the dotted head (on black and on transparent) and the flat mark (white and black)
//   menubar/    the template images for the menu bar, idle (eyes closed) and recording (eyes open)
//
// The dotted head is a halftone of round dots whose size follows the light of the marble sculpture.

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let brand = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let sources = brand.appendingPathComponent("sources")

// MARK: - Image helpers

func load(_ name: String) -> CGImage {
    let url = sources.appendingPathComponent(name)
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { fatalError("cannot read \(url.path)") }
    return img
}

func rgba(_ w: Int, _ h: Int) -> CGContext {
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    return ctx
}

func save(_ ctx: CGContext, _ path: String) {
    let url = brand.appendingPathComponent(path)
    try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, ctx.makeImage()!, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("cannot write \(url.path)") }
}

/// Grey levels (0…1, row 0 at the top) of `img` drawn into a `size`×`size` square at `rect` (top-left origin, in 0…1).
func grey(_ img: CGImage, _ size: Int, rect: CGRect = CGRect(x: 0, y: 0, width: 1, height: 1)) -> [Float] {
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: size,
                        space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)!
    ctx.interpolationQuality = .high
    ctx.setFillColor(gray: 0, alpha: 1)
    ctx.fill(CGRect(x: 0, y: 0, width: size, height: size))
    let s = CGFloat(size)
    ctx.draw(img, in: CGRect(x: rect.minX * s, y: s - rect.maxY * s, width: rect.width * s, height: rect.height * s))
    let p = ctx.data!.bindMemory(to: UInt8.self, capacity: size * size)
    return (0..<size * size).map { Float(p[$0]) / 255 }
}

/// Separable box blur, three passes (close to a Gaussian).
func blur(_ a: [Float], _ n: Int, _ r: Int) -> [Float] {
    var a = a, b = a
    for _ in 0..<3 {
        for y in 0..<n { var s: Float = 0
            for x in -r...r { s += a[y * n + min(max(x, 0), n - 1)] }
            for x in 0..<n { b[y * n + x] = s / Float(2 * r + 1)
                s += a[y * n + min(x + r + 1, n - 1)] - a[y * n + max(x - r, 0)] } }
        for x in 0..<n { var s: Float = 0
            for y in -r...r { s += b[min(max(y, 0), n - 1) * n + x] }
            for y in 0..<n { a[y * n + x] = s / Float(2 * r + 1)
                s += b[min(y + r + 1, n - 1) * n + x] - b[max(y - r, 0) * n + x] } }
    }
    return a
}

/// Square erosion of the light areas (thickens the dark lines), separable.
func erode(_ a: [Float], _ n: Int, _ r: Int) -> [Float] {
    var b = a, c = a
    for y in 0..<n { for x in 0..<n { var m: Float = 1
        for k in max(0, x - r)...min(n - 1, x + r) { m = min(m, a[y * n + k]) }; b[y * n + x] = m } }
    for y in 0..<n { for x in 0..<n { var m: Float = 1
        for k in max(0, y - r)...min(n - 1, y + r) { m = min(m, b[k * n + x]) }; c[y * n + x] = m } }
    return c
}

func smoothstep(_ a: Float, _ b: Float, _ v: Float) -> Float { let t = min(max((v - a) / (b - a), 0), 1); return t * t * (3 - 2 * t) }

// MARK: - The sculpture's light, cleaned up for the halftone

let R = 480
let tone: [Float] = {
    // The head, a little lower so it is centred once the neck and the pedestal fade out.
    let v = grey(load("head-sculpture.jpg"), R, rect: CGRect(x: 0.04, y: 0.09, width: 0.92, height: 0.92))
    let b = blur(v, R, R / 160)
    return (0..<R * R).map { i in
        var t = v[i] + 1.3 * (v[i] - b[i])                                // local contrast: the carved lines
        t = (t - 0.07) / (0.93 - 0.07)                                   // levels
        let fy = Float(i / R) / Float(R)
        let fade = fy < 0.78 ? 1 : max(0, 1 - (fy - 0.78) / 0.13)        // neck and pedestal into black
        t = pow(min(max(t, 0), 1), 1.55) * fade * fade
        return t < 0.035 ? 0 : t
    }
}()

func toneAverage(_ u0: Double, _ v0: Double, _ s: Double) -> Float {
    var sum: Float = 0
    for j in 0..<5 { for i in 0..<5 {
        let u = u0 + s * (Double(i) + 0.5) / 5, v = v0 + s * (Double(j) + 0.5) / 5
        sum += tone[min(R - 1, Int(v * Double(R))) * R + min(R - 1, Int(u * Double(R)))]
    } }
    return sum / 25
}

let black = CGColor(srgbRed: 0x0a / 255.0, green: 0x0a / 255.0, blue: 0x0a / 255.0, alpha: 1)
let cream = CGColor(srgbRed: 0xEF / 255.0, green: 0xE6 / 255.0, blue: 0xD6 / 255.0, alpha: 1)

/// Round dots on a grid of `cell` px, covering `rect` (CG coordinates, bottom-left origin).
func drawDots(_ ctx: CGContext, in rect: CGRect, cell: CGFloat) {
    ctx.setFillColor(cream)
    let n = Int((rect.width / cell).rounded(.up))
    for j in 0..<n { for i in 0..<n {
        let a = toneAverage(Double(i) * Double(cell / rect.width), Double(j) * Double(cell / rect.width), Double(cell / rect.width))
        let d = cell * CGFloat(min(0.94, pow(a, 0.66)))
        if d < 0.6 { continue }
        let cx = rect.minX + (CGFloat(i) + 0.5) * cell, cy = rect.maxY - (CGFloat(j) + 0.5) * cell
        ctx.fillEllipse(in: CGRect(x: cx - d / 2, y: cy - d / 2, width: d, height: d))
    } }
}

// MARK: - Flat mark (from the closed- or open-eyes drawing)

struct Mark { let alpha: [Float]; let n: Int }

/// The mark as an alpha mask, cropped square around the head with `pad` margin; `thicken` erodes the light (px at 1024).
func mark(_ name: String, thicken: Int = 0, pad: Double = 0.06) -> Mark {
    let N = 1024
    var a = grey(load(name), N).map { smoothstep(0.35, 0.65, $0) }
    if thicken > 0 { a = erode(a, N, thicken) }
    var x0 = N, y0 = N, x1 = 0, y1 = 0
    for y in 0..<N { for x in 0..<N where a[y * N + x] > 0.5 { x0 = min(x0, x); x1 = max(x1, x); y0 = min(y0, y); y1 = max(y1, y) } }
    let side = Int(Double(max(x1 - x0, y1 - y0)) * (1 + 2 * pad)), cx = (x0 + x1) / 2, cy = (y0 + y1) / 2
    var out = [Float](repeating: 0, count: side * side)
    for y in 0..<side { for x in 0..<side {
        let sx = cx - side / 2 + x, sy = cy - side / 2 + y
        if sx >= 0 && sx < N && sy >= 0 && sy < N { out[y * side + x] = a[sy * N + sx] }
    } }
    return Mark(alpha: out, n: side)
}

func maskImage(_ m: Mark) -> CGImage {
    let bytes = m.alpha.map { UInt8(($0 * 255).rounded()) }
    let provider = CGDataProvider(data: Data(bytes) as CFData)!
    return CGImage(width: m.n, height: m.n, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: m.n,
                   space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                   provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)!
}

/// Fills `rect` with `color` through the mark used as a mask.
func drawMark(_ ctx: CGContext, _ m: Mark, in rect: CGRect, color: CGColor) {
    ctx.saveGState()
    ctx.clip(to: rect, mask: maskImage(m))
    ctx.setFillColor(color)
    ctx.fill(rect)
    ctx.restoreGState()
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

let closedMark = mark("head-mark-eyes-closed.jpg")

func appIcon(_ px: Int) -> CGContext {
    let ctx = rgba(px, px), s = CGFloat(px) / 1024
    let body = CGRect(x: 100 * s, y: 100 * s, width: 824 * s, height: 824 * s)
    let path = squircle(body)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10 * s), blur: 24 * s, color: CGColor(gray: 0, alpha: 0.35))
    ctx.addPath(path); ctx.setFillColor(black); ctx.fillPath()
    ctx.restoreGState()
    ctx.addPath(path); ctx.clip()
    if px <= 64 {
        // Too small for dots: the flat mark, so the head stays readable in lists and the Dock's smallest sizes.
        drawMark(ctx, closedMark, in: body.insetBy(dx: body.width * 0.14, dy: body.width * 0.14), color: cream)
    } else {
        drawDots(ctx, in: body, cell: max(3, body.width / 62))
    }
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

do {
    let ctx = rgba(1024, 1024)
    ctx.setFillColor(black); ctx.fill(CGRect(x: 0, y: 0, width: 1024, height: 1024))
    drawDots(ctx, in: CGRect(x: 0, y: 0, width: 1024, height: 1024), cell: 1024 / 62)
    save(ctx, "logo/greffier-dots-on-black-1024.png")
    let clear = rgba(1024, 1024)
    drawDots(clear, in: CGRect(x: 0, y: 0, width: 1024, height: 1024), cell: 1024 / 62)
    save(clear, "logo/greffier-dots-transparent-1024.png")
}
for (name, color) in [("white", CGColor(gray: 1, alpha: 1)), ("black", CGColor(gray: 0, alpha: 1)), ("cream", cream)] {
    let ctx = rgba(1024, 1024)
    drawMark(ctx, closedMark, in: CGRect(x: 0, y: 0, width: 1024, height: 1024), color: color)
    save(ctx, "logo/greffier-mark-\(name)-1024.png")
}

// MARK: - Menu bar template images (18 pt; black + alpha, macOS tints them)

// The lines are thickened before reducing so that they still cover about a pixel at 18 and 36 px.
for (state, source) in [("Idle", "head-mark-eyes-closed.jpg"), ("Recording", "head-mark-eyes-open.jpg")] {
    for (scale, suffix, thicken) in [(1, "", 11), (2, "@2x", 7)] {
        let m = mark(source, thicken: thicken, pad: 0.02)
        let px = 18 * scale, ctx = rgba(px, px)
        drawMark(ctx, m, in: CGRect(x: 0, y: 0, width: px, height: px), color: CGColor(gray: 0, alpha: 1))
        save(ctx, "menubar/Menubar\(state)Template\(suffix).png")
    }
}

print("rendered into \(brand.path)")
