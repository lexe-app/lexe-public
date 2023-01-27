import Cocoa
import FlutterMacOS

@NSApplicationMain
class AppDelegate: FlutterAppDelegate {
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    // reference a dummy method that references all our exported ffi symbols so
    // Xcode won't strip our symbols.
    print(dummy_method_to_enforce_bundling())

    return true
  }
}
