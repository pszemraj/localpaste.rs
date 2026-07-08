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
          nav_probe_macos_driver key --pid PID --key-code CODE [--modifier NAME ...] [--modifiers CSV]
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

func postKey(source: CGEventSource, keyCode: CGKeyCode, keyDown: Bool, flags: CGEventFlags) throws {
    guard let event = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: keyDown) else {
        throw DriverError(description: "failed to create CGEvent for key code \(keyCode)")
    }
    event.flags = flags
    event.post(tap: .cghidEventTap)
    usleep(20_000)
}

func postChord(pid: pid_t, keyCode: CGKeyCode, modifiers: [ModifierSpec]) throws {
    try requireAccessibility()
    try activate(pid: pid)

    guard let source = CGEventSource(stateID: .hidSystemState) else {
        throw DriverError(description: "failed to create CGEventSource")
    }

    var currentFlags = CGEventFlags(rawValue: 0)
    for modifier in modifiers {
        currentFlags = CGEventFlags(rawValue: currentFlags.rawValue | modifier.flag.rawValue)
        try postKey(source: source, keyCode: modifier.keyCode, keyDown: true, flags: currentFlags)
    }

    let fullFlags = flagSet(modifiers)
    try postKey(source: source, keyCode: keyCode, keyDown: true, flags: fullFlags)
    try postKey(source: source, keyCode: keyCode, keyDown: false, flags: fullFlags)

    for modifier in modifiers.reversed() {
        try postKey(source: source, keyCode: modifier.keyCode, keyDown: false, flags: currentFlags)
        currentFlags = CGEventFlags(rawValue: currentFlags.rawValue & ~modifier.flag.rawValue)
    }
}

struct ParsedArgs {
    var prompt = false
    var pid: pid_t?
    var keyCode: CGKeyCode?
    var modifiers: [String] = []
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
    case "key":
        try postChord(
            pid: try requirePid(parsed),
            keyCode: try requireKeyCode(parsed),
            modifiers: try normalizeModifiers(parsed.modifiers)
        )
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
