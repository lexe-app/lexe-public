/// Interact with the system Clipboard in a consistent way.
library;

import 'dart:io' show Platform;

import 'package:device_info_plus/device_info_plus.dart'
    show AndroidDeviceInfo, DeviceInfoPlugin;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard, ClipboardData;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';

abstract final class LxClipboard {
  const LxClipboard._();

  /// Just copy text to the system clipboard without telling the user.
  static Future<Result<void, Exception>> copyText(String text) =>
      Result.tryAsync(() => Clipboard.setData(ClipboardData(text: text)));

  /// Copy text to the system clipboard and show a toast message notifying the
  /// user about what we copied.
  static Future<void> copyTextWithFeedback(
    BuildContext context,
    String text,
  ) async {
    final result = await LxClipboard.copyText(text);
    if (!context.mounted) return;

    switch (result) {
      case Ok():
        // Certain platforms already show UI feedback on copy-to-clipboard, so
        // we don't have to do anything.
        if (await LxClipboard.platformUIShowsCopyFeedback()) {
          return;
        }
        if (!context.mounted) return;

        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            // Make this shorter than default 4s
            duration: const Duration(milliseconds: 2000),
            content: Text(
              "Copied: $text",
              // TODO(phlip9): fix: breaks ellipsis for multiline.
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        );

      case Err(:final err):
        warn("Clipboard.copyText: error: $err");
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text("Failed to copy to clipboard")),
        );
    }
  }

  /// Returns true if the underlying platform automatically shows some kind of
  /// UI feedback when text gets copied into the user's clipboard.
  ///
  /// This is only true on Android 13+ (SDK 33+).
  static Future<bool> platformUIShowsCopyFeedback() async {
    // Only Android does this atm
    if (!Platform.isAndroid) {
      return false;
    }

    // Get current Android OS SDK version
    // NOTE: device_info_plus already caches the result
    final res = await Result.tryAsync<AndroidDeviceInfo, Exception>(
      () => DeviceInfoPlugin().androidInfo,
    );
    switch (res) {
      case Ok(:final ok):
        // Only Android 13+ (SDK 33+) supports this
        return ok.version.sdkInt >= 33;
      case Err(:final err):
        warn("Failed to get Android device info: $err");
        return false;
    }
  }

  /// Get current clipboard text
  static Future<String?> getText() async {
    // TODO(phlip9): if flutter ever supports clipboard images, we could try to
    // QR decode here as well
    final res = await Result.tryAsync<ClipboardData?, Exception>(
      () => Clipboard.getData(Clipboard.kTextPlain),
    );
    switch (res) {
      case Ok(:final ok):
        if (ok == null) return null;
        return ok.text;
      case Err(:final err):
        warn("Failed to get text from clipboard: $err");
        return null;
    }
  }
}
