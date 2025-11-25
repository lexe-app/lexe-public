/// Platform interface for native "share" functionality.
library;

import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:lexeapp/prelude.dart';
import 'package:share_plus/share_plus.dart'
    show Share, ShareResult, ShareResultStatus;
import 'package:url_launcher/url_launcher.dart' as url_launcher;

abstract final class LxShare {
  /// Share/open a payment URI (i.e., "bitcoin:" or "lightning:" URI).
  ///
  /// Ideally, this would show a share bubble on-screen that asks what the user
  /// would like to do with it. From this bubble, users should be able to
  /// (1) open it in _another_ capable wallet app
  /// (2) send it to a group chat
  /// (3) share it to their X timeline
  /// etc...
  ///
  /// Sadly the current flutter packages (`share_plus` and `url_launcher`) don't
  /// quite support all of this in one bubble. We'd probably need to write our
  /// own particular native handler for iOS and Android for this to work as
  /// I want. Instead, we'll just try sharing as plaintext.
  ///
  /// The `context` parameter is for macOS and iPad, so they can draw the share
  /// popup bubble above that widget.
  static Future<void> sharePaymentUri(BuildContext context, Uri uri) async {
    // TODO(phlip9): if Lexe is the only wallet registered as a handler, tapping
    // "share payment" in Lexe will immediately open... Lexe, again, to handle
    // it... Definitely not what we want.
    //
    // Ideally, `url_launcher` would let us see _which_ apps can handle the URIs,
    // so we can filter out Lexe as an option.

    // // First try opening the payment URI in another app (if there are any that
    // // support it):
    // final openResult = await LxShare._tryOpenPaymentUriInOtherApp(uri);
    // if (!context.mounted) return;
    //
    // switch (openResult) {
    //   case ShareResultStatus.success || ShareResultStatus.dismissed:
    //     return;
    //   case ShareResultStatus.unavailable:
    // }

    // Otherwise fallback to sharing as plaintext:

    // The box around the widget associated with `context`. `share_plus` uses
    // this on some platforms (macOS, iPad) to draw the share dialog above
    // wherever the user tapped.
    final box = context.findRenderObject() as RenderBox?;
    final origin = box!.localToGlobal(Offset.zero) & box.size;

    final result = await LxShare._trySharePaymentUriAsPlaintext(uri, origin);
    if (!context.mounted) return;

    switch (result) {
      case ShareResultStatus.success || ShareResultStatus.dismissed:
        return;
      case ShareResultStatus.unavailable:
    }

    // Tell the user we can't get anything to work
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text("Lexe doesn't support sharing on this platform yet!"),
      ),
    );
    return;
  }

  /// Share a payment address as a plain text message.
  /// Supported on all platforms.
  static Future<void> sharePaymentAddress(
    BuildContext context,
    String address,
  ) async {
    final box = context.findRenderObject() as RenderBox?;
    final origin = box!.localToGlobal(Offset.zero) & box.size;

    final result = await LxShare._trySharePaymentAddressAsPlaintext(
      address,
      origin,
    );
    if (!context.mounted) return;

    switch (result) {
      case ShareResultStatus.success || ShareResultStatus.dismissed:
        return;
      case ShareResultStatus.unavailable:
    }

    // Tell the user we can't get anything to work
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(
        content: Text("Lexe doesn't support sharing on this platform yet!"),
      ),
    );
    return;
  }

  // TODO(phlip9): right now, this will show Lexe as an option to handle a
  // payment URI, which is super confusing. We'll need to somehow filter
  // ourselves out of the list of handlers.
  // ignore: unused_element
  static Future<ShareResultStatus> _tryOpenPaymentUriInOtherApp(Uri uri) async {
    // Try to query if there's any apps that can handle it.
    final canLaunch = await Result.tryAsync<bool, Exception>(
      () => url_launcher.canLaunchUrl(uri),
    );
    info("LxShare: open payment uri: can launch: $canLaunch");
    switch (canLaunch) {
      case Ok(:final ok):
        if (!ok) return ShareResultStatus.unavailable;
      case Err():
        return ShareResultStatus.unavailable;
    }

    // Try to actually open it.
    final result = await Result.tryAsync<bool, Exception>(
      () => url_launcher.launchUrl(
        uri,
        mode: url_launcher.LaunchMode.externalNonBrowserApplication,
      ),
    );
    info("LxShare: open payment uri: launch result: $result");

    return switch (result) {
      Ok(:final ok) =>
        (ok) ? ShareResultStatus.success : ShareResultStatus.unavailable,
      Err() => ShareResultStatus.unavailable,
    };
  }

  static Future<ShareResultStatus> _trySharePaymentUriAsPlaintext(
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
        info("LxShare: share payment uri: ok: $ok");
        return ok.status;
      case Err(:final err):
        warn("LxShare: share payment uri: err: $err");
        return ShareResultStatus.unavailable;
    }
  }

  static Future<ShareResultStatus> _trySharePaymentAddressAsPlaintext(
    String address,
    Rect origin,
  ) async {
    final result = await Result.tryAsync<ShareResult, Exception>(() async {
      final shareMessage = "Pay me at $address";
      if (Platform.isIOS ||
          Platform.isAndroid ||
          Platform.isMacOS ||
          Platform.isWindows ||
          Platform.isLinux) {
        return Share.share(shareMessage, sharePositionOrigin: origin);
      } else {
        return const ShareResult("", ShareResultStatus.unavailable);
      }
    });

    switch (result) {
      case Ok(:final ok):
        info("LxShare: share payment address: ok: $ok");
        return ok.status;
      case Err(:final err):
        warn("LxShare: share payment address: err: $err");
        return ShareResultStatus.unavailable;
    }
  }
}
