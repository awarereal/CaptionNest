// Developer helper: print window id, bounds and title for windows owned by an app.
// Usage: swift scripts/dev/winshot.swift <owner-substring>
// Then: screencapture -o -l <id> out.png
import CoreGraphics
import Foundation

let needle = (CommandLine.arguments.dropFirst().first ?? "captionnest").lowercased()
let options = CGWindowListOption(arrayLiteral: .optionOnScreenOnly, .excludeDesktopElements)
let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
for w in list {
    let owner = (w[kCGWindowOwnerName as String] as? String) ?? ""
    guard owner.lowercased().contains(needle) else { continue }
    let id = w[kCGWindowNumber as String] as? Int ?? 0
    let layer = w[kCGWindowLayer as String] as? Int ?? 0
    let name = (w[kCGWindowName as String] as? String) ?? ""
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    print("\(id)\t\(layer)\t\(owner)\t\(name)\t\(b["X"] ?? 0),\(b["Y"] ?? 0) \(b["Width"] ?? 0)x\(b["Height"] ?? 0)")
}
