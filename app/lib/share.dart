/// Platform interface for native "share" functionality.
library;

import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:share_plus/share_plus.dart'
    show Share, ShareResult, ShareResultStatus;

abstract final class LxShare {
  ///
  /// macOS and iPad need `context` so they can draw a popup bubble above that
  /// widget.
  static Future<void> sharePaymentUri(
    BuildContext context,
    Uri uri,
  ) async {
    // case Android: -> shareUri(uri)

    // The box around the widget associated with `context`. `share_plus` uses
    // this on some platforms (macOS, iPad) to draw the share dialog above
    // wherever the user tapped.
    final box = context.findRenderObject() as RenderBox?;
    final origin = box!.localToGlobal(Offset.zero) & box.size;

    final result = await LxShare._sharePaymentUriInner(uri, origin);
    if (!context.mounted) return;

    switch (result) {
      case ShareResultStatus.success || ShareResultStatus.dismissed:
        return;
      case ShareResultStatus.unavailable:
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
          content: Text("Lexe doesn't support sharing on this platform yet!"),
        ));
    }

    return;
  }

  static Future<ShareResultStatus> _sharePaymentUriInner(
    Uri uri,
    Rect origin,
  ) async {
    final result = await Result.tryAsync<ShareResult, Exception>(() async {
      if (Platform.isIOS || Platform.isAndroid) {
        return Share.shareUri(uri, sharePositionOrigin: origin);
      } else if (Platform.isMacOS || Platform.isWindows || Platform.isLinux) {
        // `Share.shareUri` is only supported on mobile...
        return Share.share(uri.toString(), sharePositionOrigin: origin);
      } else {
        return const ShareResult("", ShareResultStatus.unavailable);
      }
    });

    switch (result) {
      case Ok(:final ok):
        info("share: payment uri: ok: $ok");
        return ok.status;
      case Err(:final err):
        warn("share: payment uri: err: $err");
        return ShareResultStatus.unavailable;
    }
  }
}
