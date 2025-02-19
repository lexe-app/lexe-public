/// Interact with the system Clipboard in a consistent way.
library;

import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard, ClipboardData;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';

abstract final class LxClipboard {
  const LxClipboard._();

  /// Just copy text to the system clipboard without telling the user.
  static Future<Result<void, Exception>> copyText(String text) =>
      Result.tryAsync(
        () => Clipboard.setData(ClipboardData(text: text)),
      );

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
        // Android already shows a bottom bar automatically.
        if (Platform.isAndroid) return;

        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(
            "Copied: $text",
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
        ));

      case Err(:final err):
        warn("Clipboard.copyText: error: $err");
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text("Failed to copy to clipboard")));
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
