import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

struct DriverError: Error, CustomStringConvertible {
    let description: String
}

struct ModifierSpec {
    let name: String
    let keyCode: CGKeyCode
    let flag: CGEventFlags
}

let modifierSpecs: [String: ModifierSpec] = [
    "command": ModifierSpec(name: "command", keyCode: 55, flag: .maskCommand),
    "cmd": ModifierSpec(name: "command", keyCode: 55, flag: .maskCommand),
    "option": ModifierSpec(name: "option", keyCode: 58, flag: .maskAlternate),
    "alt": ModifierSpec(name: "option", keyCode: 58, flag: .maskAlternate),
    "shift": ModifierSpec(name: "shift", keyCode: 56, flag: .maskShift),
    "control": ModifierSpec(name: "control", keyCode: 59, flag: .maskControl),
    "ctrl": ModifierSpec(name: "control", keyCode: 59, flag: .maskControl),
]

func writeStderr(_ message: String) {
    FileHandle.standardError.write(Data((message + "\n").utf8))
}

func usage() -> Never {
    writeStderr(
        """
        Usage:
          nav_probe_macos_driver check-accessibility [--prompt]
          nav_probe_macos_driver exists --pid PID
          nav_probe_macos_driver activate --pid PID
          nav_probe_macos_driver frontmost --pid PID
          nav_probe_macos_driver key --pid PID --key-code CODE [--modifier NAME ...] [--modifiers CSV] [--key-delay-ms MS]
          nav_probe_macos_driver type --pid PID --text TEXT
          nav_probe_macos_driver snapshot --pid PID
          nav_probe_macos_driver click --pid PID --label LABEL [--role AXTextField]
          nav_probe_macos_driver clipboard-save --path PRIVATE_FILE
          nav_probe_macos_driver clipboard-set|clipboard-check --text TEXT
          nav_probe_macos_driver clipboard-restore --path PRIVATE_FILE --text EXPECTED_CURRENT_TEXT
        """
    )
    exit(2)
}

func parseInt<T: FixedWidthInteger>(_ value: String, label: String) throws -> T {
    guard let parsed = T(value) else {
        throw DriverError(description: "\(label) must be an integer; got \(value)")
    }
    return parsed
}

func accessibilityTrusted(prompt: Bool) -> Bool {
    if prompt {
        let key = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
        let options = [key: true] as CFDictionary
        return AXIsProcessTrustedWithOptions(options)
    }
    return AXIsProcessTrusted()
}

func requireAccessibility() throws {
    guard accessibilityTrusted(prompt: false) else {
        throw DriverError(
            description: "macOS Accessibility permission is required for native key injection"
        )
    }
}

func runningApplication(pid: pid_t) throws -> NSRunningApplication {
    guard let app = NSRunningApplication(processIdentifier: pid), !app.isTerminated else {
        throw DriverError(description: "process not found for pid \(pid)")
    }
    return app
}

func frontmostApplicationPid() -> pid_t? {
    NSWorkspace.shared.frontmostApplication?.processIdentifier
}

func waitForFrontmost(pid: pid_t, attempts: Int = 5, delayUsec: useconds_t = 20_000) -> Bool {
    for _ in 0..<attempts {
        if frontmostApplicationPid() == pid {
            return true
        }
        usleep(delayUsec)
    }
    return false
}

func setAccessibilityAttribute(
    _ element: AXUIElement,
    _ attribute: CFString,
    _ value: CFTypeRef,
    label: String,
    required: Bool = false
) throws {
    let result = AXUIElementSetAttributeValue(element, attribute, value)
    if result == .success || !required {
        return
    }
    throw DriverError(description: "failed to set \(label) via Accessibility: \(result)")
}

func focusWithAccessibility(pid: pid_t) throws {
    try requireAccessibility()
    let appElement = AXUIElementCreateApplication(pid)
    try setAccessibilityAttribute(
        appElement,
        kAXFrontmostAttribute as CFString,
        kCFBooleanTrue,
        label: "pid \(pid) frontmost",
        required: true
    )

    var windowsValue: CFTypeRef?
    let windowsResult = AXUIElementCopyAttributeValue(
        appElement,
        kAXWindowsAttribute as CFString,
        &windowsValue
    )
    guard windowsResult == .success else {
        return
    }
    guard let windows = windowsValue as? [AXUIElement] else {
        return
    }
    guard let window = windows.first else {
        return
    }
    _ = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    try setAccessibilityAttribute(
        appElement,
        kAXFocusedWindowAttribute as CFString,
        window,
        label: "pid \(pid) focused window"
    )
    try setAccessibilityAttribute(
        window,
        kAXMainAttribute as CFString,
        kCFBooleanTrue,
        label: "pid \(pid) main window"
    )
    try setAccessibilityAttribute(
        window,
        kAXFocusedAttribute as CFString,
        kCFBooleanTrue,
        label: "pid \(pid) focused window flag"
    )
}

func activate(pid: pid_t) throws {
    let app = try runningApplication(pid: pid)
    _ = app.unhide()
    let activated: Bool
    if #available(macOS 14.0, *) {
        activated = app.activate()
    } else {
        activated = app.activate(options: [.activateIgnoringOtherApps])
    }
    try focusWithAccessibility(pid: pid)
    guard waitForFrontmost(pid: pid) else {
        let current = frontmostApplicationPid().map(String.init) ?? "none"
        let activationDetail = activated ? "" : "; app.activate returned false"
        throw DriverError(
            description: "failed to make pid \(pid) frontmost; current frontmost pid is \(current)\(activationDetail)"
        )
    }
    usleep(120_000)
}

func flagSet(_ modifiers: [ModifierSpec]) -> CGEventFlags {
    modifiers.reduce(CGEventFlags(rawValue: 0)) { flags, modifier in
        CGEventFlags(rawValue: flags.rawValue | modifier.flag.rawValue)
    }
}

func postKey(source: CGEventSource, keyCode: CGKeyCode, keyDown: Bool, flags: CGEventFlags, delayMs: Int = 20) throws {
    guard let event = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: keyDown) else {
        throw DriverError(description: "failed to create CGEvent for key code \(keyCode)")
    }
    event.flags = flags
    event.post(tap: .cghidEventTap)
    usleep(useconds_t(delayMs * 1000))
}

func postChord(pid: pid_t, keyCode: CGKeyCode, modifiers: [ModifierSpec], delayMs: Int = 20) throws {
    try requireAccessibility()
    try activate(pid: pid)

    guard let source = CGEventSource(stateID: .hidSystemState) else {
        throw DriverError(description: "failed to create CGEventSource")
    }

    var currentFlags = CGEventFlags(rawValue: 0)
    for modifier in modifiers {
        currentFlags = CGEventFlags(rawValue: currentFlags.rawValue | modifier.flag.rawValue)
        try postKey(source: source, keyCode: modifier.keyCode, keyDown: true, flags: currentFlags, delayMs: delayMs)
    }

    let fullFlags = flagSet(modifiers)
    try postKey(source: source, keyCode: keyCode, keyDown: true, flags: fullFlags, delayMs: delayMs)
    try postKey(source: source, keyCode: keyCode, keyDown: false, flags: fullFlags, delayMs: delayMs)

    for modifier in modifiers.reversed() {
        currentFlags = CGEventFlags(rawValue: currentFlags.rawValue & ~modifier.flag.rawValue)
        try postKey(source: source, keyCode: modifier.keyCode, keyDown: false, flags: currentFlags, delayMs: delayMs)
    }
}

struct ParsedArgs {
    var prompt = false
    var pid: pid_t?
    var keyCode: CGKeyCode?
    var modifiers: [String] = []
    var delayMs = 20
    var text = ""
    var path = ""
    var label = ""
    var role = "AXTextField"
}

func parseArgs(_ args: [String]) throws -> ParsedArgs {
    var parsed = ParsedArgs()
    var index = 0
    while index < args.count {
        let arg = args[index]
        switch arg {
        case "--prompt":
            parsed.prompt = true
            index += 1
        case "--pid":
            guard index + 1 < args.count else {
                throw DriverError(description: "--pid requires a value")
            }
            parsed.pid = try parseInt(args[index + 1], label: "--pid")
            index += 2
        case "--key-code":
            guard index + 1 < args.count else {
                throw DriverError(description: "--key-code requires a value")
            }
            parsed.keyCode = try parseInt(args[index + 1], label: "--key-code")
            index += 2
        case "--modifier":
            guard index + 1 < args.count else {
                throw DriverError(description: "--modifier requires a value")
            }
            parsed.modifiers.append(args[index + 1])
            index += 2
        case "--modifiers":
            guard index + 1 < args.count else {
                throw DriverError(description: "--modifiers requires a value")
            }
            parsed.modifiers.append(
                contentsOf: args[index + 1]
                    .split(separator: ",")
                    .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
                    .filter { !$0.isEmpty }
            )
            index += 2
        case "--key-delay-ms", "--text", "--path", "--label", "--role":
            guard index + 1 < args.count else {
                throw DriverError(description: "\(arg) requires a value")
            }
            let value = args[index + 1]
            switch arg {
            case "--key-delay-ms":
                parsed.delayMs = try parseInt(value, label: arg)
                guard (0...1000).contains(parsed.delayMs) else {
                    throw DriverError(description: "--key-delay-ms must be between 0 and 1000")
                }
            case "--text": parsed.text = value
            case "--path": parsed.path = value
            case "--label": parsed.label = value
            default: parsed.role = value
            }
            index += 2
        case "-h", "--help":
            usage()
        default:
            throw DriverError(description: "unknown argument: \(arg)")
        }
    }
    return parsed
}

func normalizeModifiers(_ names: [String]) throws -> [ModifierSpec] {
    var seen = Set<String>()
    var out: [ModifierSpec] = []
    for rawName in names {
        let key = rawName.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !key.isEmpty else {
            continue
        }
        guard let modifier = modifierSpecs[key] else {
            throw DriverError(description: "unsupported modifier: \(rawName)")
        }
        if seen.insert(modifier.name).inserted {
            out.append(modifier)
        }
    }
    return out
}

func requirePid(_ parsed: ParsedArgs) throws -> pid_t {
    guard let pid = parsed.pid else {
        throw DriverError(description: "--pid is required")
    }
    return pid
}

func requireKeyCode(_ parsed: ParsedArgs) throws -> CGKeyCode {
    guard let keyCode = parsed.keyCode else {
        throw DriverError(description: "--key-code is required")
    }
    return keyCode
}

// Keep all representations private, including non-text clipboard items.
struct ClipboardRepresentation: Codable, Equatable {
    let type: String
    let data: Data
}
typealias ClipboardSnapshot = [[ClipboardRepresentation]]

func clipboardSnapshot() -> ClipboardSnapshot {
    (NSPasteboard.general.pasteboardItems ?? []).map { item in
        item.types.compactMap { type in
            item.data(forType: type).map { ClipboardRepresentation(type: type.rawValue, data: $0) }
        }
    }
}

func clipboardFile(_ parsed: ParsedArgs) throws -> URL {
    guard !parsed.path.isEmpty else { throw DriverError(description: "--path is required") }
    return URL(fileURLWithPath: parsed.path)
}

func axValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value
}

func axElements(_ pid: pid_t) -> [AXUIElement] {
    var elements: [AXUIElement] = []
    func visit(_ element: AXUIElement, depth: Int) {
        guard depth < 40, elements.count < 4000 else { return }
        elements.append(element)
        for child in axValue(element, kAXChildrenAttribute) as? [AXUIElement] ?? [] {
            visit(child, depth: depth + 1)
        }
    }
    let app = AXUIElementCreateApplication(pid)
    for window in axValue(app, kAXWindowsAttribute) as? [AXUIElement] ?? [] {
        if (axValue(window, kAXTitleAttribute) as? String) == "LocalPaste.rs" {
            visit(window, depth: 0)
        }
    }
    return elements
}

func snapshotAX(_ pid: pid_t) throws {
    try requireAccessibility()
    let rows: [[String: Any]] = axElements(pid).map { element in
        var row: [String: Any] = [:]
        for (name, attribute) in [("role", kAXRoleAttribute), ("title", kAXTitleAttribute),
                                  ("value", kAXValueAttribute), ("description", kAXDescriptionAttribute)] {
            if let value = axValue(element, attribute) as? String { row[name] = value }
        }
        row["focused"] = (axValue(element, kAXFocusedAttribute) as? Bool) ?? false
        return row
    }
    let data = try JSONSerialization.data(withJSONObject: rows, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}

func clickAX(_ pid: pid_t, label: String, role: String) throws {
    try requireAccessibility()
    try activate(pid: pid)
    let matches = axElements(pid).filter { element in
        (axValue(element, kAXRoleAttribute) as? String) == role &&
        [kAXTitleAttribute, kAXValueAttribute, kAXDescriptionAttribute].contains { attribute in
            (axValue(element, attribute) as? String) == label
        }
    }
    guard matches.count == 1, let element = matches.first,
          let positionValue = axValue(element, kAXPositionAttribute),
          let sizeValue = axValue(element, kAXSizeAttribute) else {
        throw DriverError(description: "expected one \(role) labeled \(label); found \(matches.count)")
    }
    var position = CGPoint.zero
    var size = CGSize.zero
    AXValueGetValue(positionValue as! AXValue, .cgPoint, &position)
    AXValueGetValue(sizeValue as! AXValue, .cgSize, &size)
    let point = CGPoint(x: position.x + size.width / 2, y: position.y + size.height / 2)
    for type in [CGEventType.leftMouseDown, .leftMouseUp] {
        guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left) else {
            throw DriverError(description: "failed to create click")
        }
        event.post(tap: .cghidEventTap)
        usleep(20_000)
    }
}

func typeText(_ pid: pid_t, text: String) throws {
    try requireAccessibility()
    try activate(pid: pid)
    for character in text {
        let units = Array(String(character).utf16)
        for down in [true, false] {
            guard let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down) else {
                throw DriverError(description: "failed to create text event")
            }
            event.flags = []
            event.keyboardSetUnicodeString(stringLength: units.count, unicodeString: units)
            event.post(tap: .cghidEventTap)
        }
        usleep(5_000)
    }
}

let argv = Array(CommandLine.arguments.dropFirst())
guard let command = argv.first else {
    usage()
}

do {
    let parsed = try parseArgs(Array(argv.dropFirst()))
    switch command {
    case "check-accessibility":
        if accessibilityTrusted(prompt: parsed.prompt) {
            print("accessibility=trusted")
            exit(0)
        }
        writeStderr("accessibility=not_trusted")
        exit(1)
    case "exists":
        _ = try runningApplication(pid: try requirePid(parsed))
    case "activate":
        try activate(pid: try requirePid(parsed))
    case "frontmost":
        guard frontmostApplicationPid() == (try requirePid(parsed)) else {
            throw DriverError(description: "unexpected foreground application")
        }
    case "key":
        try postChord(
            pid: try requirePid(parsed),
            keyCode: try requireKeyCode(parsed),
            modifiers: try normalizeModifiers(parsed.modifiers),
            delayMs: parsed.delayMs
        )
    case "type": try typeText(try requirePid(parsed), text: parsed.text)
    case "snapshot": try snapshotAX(try requirePid(parsed))
    case "click": try clickAX(try requirePid(parsed), label: parsed.label, role: parsed.role)
    case "clipboard-save":
        let url = try clipboardFile(parsed)
        try JSONEncoder().encode(clipboardSnapshot()).write(to: url, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    case "clipboard-set":
        NSPasteboard.general.clearContents()
        guard NSPasteboard.general.setString(parsed.text, forType: .string) else {
            throw DriverError(description: "failed to set clipboard")
        }
    case "clipboard-check":
        guard NSPasteboard.general.string(forType: .string) == parsed.text else {
            throw DriverError(description: "clipboard does not match the synthetic fixture")
        }
    case "clipboard-restore":
        // Never replace clipboard activity that occurred outside the test.
        guard NSPasteboard.general.string(forType: .string) == parsed.text else {
            throw DriverError(description: "clipboard changed outside the test; original backup retained")
        }
        let url = try clipboardFile(parsed)
        let saved = try JSONDecoder().decode(ClipboardSnapshot.self, from: Data(contentsOf: url))
        let items = saved.map { representations -> NSPasteboardItem in
            let item = NSPasteboardItem()
            for entry in representations {
                item.setData(entry.data, forType: NSPasteboard.PasteboardType(entry.type))
            }
            return item
        }
        NSPasteboard.general.clearContents()
        if !items.isEmpty { NSPasteboard.general.writeObjects(items) }
        guard clipboardSnapshot() == saved else {
            throw DriverError(description: "clipboard restoration verification failed")
        }
        try FileManager.default.removeItem(at: url)
        print("clipboard_restored=true")
    case "-h", "--help":
        usage()
    default:
        throw DriverError(description: "unknown command: \(command)")
    }
} catch let error as DriverError {
    writeStderr(error.description)
    exit(2)
} catch {
    writeStderr(String(describing: error))
    exit(2)
}
