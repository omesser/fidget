// Prints a PNG's median alpha (0-255) and a sharpness: the largest luminance
// step between neighbouring pixels in each row, inside an 8% margin, at the 25th
// percentile of rows. Stripes seen through the panel step in every row; under a
// blur, rows without text step by one or two. Usage: pixel-stats <png>
import CoreGraphics
import Foundation
import ImageIO

guard CommandLine.arguments.count == 2 else {
  FileHandle.standardError.write("usage: pixel-stats <png>\n".data(using: .utf8)!)
  exit(2)
}
let url = URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL
guard let source = CGImageSourceCreateWithURL(url, nil),
  let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
else {
  FileHandle.standardError.write("pixel-stats: cannot read \(CommandLine.arguments[1])\n".data(using: .utf8)!)
  exit(3)
}
let (w, h) = (image.width, image.height)
var pixels = [UInt8](repeating: 0, count: w * h * 4)
let context = CGContext(
  data: &pixels, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
  space: CGColorSpaceCreateDeviceRGB(),
  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))

func percentile(_ values: [Int], _ p: Int) -> Int { values.sorted()[values.count * p / 100] }
func luma(_ i: Int) -> Int {
  (Int(pixels[i]) * 299 + Int(pixels[i + 1]) * 587 + Int(pixels[i + 2]) * 114) / 1000
}
let alphas = (0..<(w * h)).map { Int(pixels[$0 * 4 + 3]) }
let (mx, my) = (w * 8 / 100, h * 8 / 100)
let steps = (my..<(h - my)).map { y in
  (mx..<(w - mx - 1)).map { x in abs(luma((y * w + x + 1) * 4) - luma((y * w + x) * 4)) }.max()!
}
print(percentile(alphas, 50), percentile(steps, 25))
