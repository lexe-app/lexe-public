import 'dart:async' show unawaited;
import 'dart:io' show Platform;

import 'package:app_links/app_links.dart' as app_links;
import 'package:flutter/foundation.dart' show immutable;
import 'package:flutter/services.dart' show MethodChannel;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/stream_ext.dart';
import 'package:rxdart/rxdart.dart' show BehaviorSubject, Rx;

/// An interface for receiving platform URI events. A URI event is when the user
/// taps e.g. a "lightning:" URI in a browser or other app, and then they or the
/// platform selects Lexe to handle this URI. We only receive events for URIs
/// that we are registered to handle.
///
/// See: app/android/app/src/main/AndroidManifest.xml
///      app/ios/Runner/Info.plist
///      app/macos/Runner/Info.plist
///
/// Testing on iOS simulator:
///
/// ```bash
/// $ /usr/bin/xcrun simctl openurl booted "bitcoin:bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk"
/// $ /usr/bin/xcrun simctl openurl booted "bitcoin:?lno=lno1zrxq8pjw7qjlm68mtp7e3yvxee4y5xrgjhhyf2fxhlphpckrvevh50u0qdp2nyl5lh362fu4r6ycw59tul97ptq57j9mhusk4dyqed0nytnzyqsz0qduahca4eryls267a72a4rtcnk4p6ululyvg7a7pdczg8ha8e6qqval7cremj65ut2k087xdhay6qvv0dtljppyd80zyj68f748jt569nutyznpf9qms39a06ecl0tw9w6ky9xpqd4k7hl4phttq9lkdrhjffv08tc04yxf4pfexypwt0e8zlmdeuf4qqqsdt4qevd84nlmks62nzzz9swwpu"
/// ```
///
/// Testing on Android:
///
/// ```bash
/// $ PATH="$ANDROID_HOME/platform-tools:$PATH" adb shell am start -a android.intent.action.VIEW \
///     -d "bitcoin:bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk"
/// $ PATH="$ANDROID_HOME/platform-tools:$PATH" adb shell am start -a android.intent.action.VIEW \
///     -d "bitcoin:?lno=lno1zrxq8pjw7qjlm68mtp7e3yvxee4y5xrgjhhyf2fxhlphpckrvevh50u0qdp2nyl5lh362fu4r6ycw59tul97ptq57j9mhusk4dyqed0nytnzyqsz0qduahca4eryls267a72a4rtcnk4p6ululyvg7a7pdczg8ha8e6qqval7cremj65ut2k087xdhay6qvv0dtljppyd80zyj68f748jt569nutyznpf9qms39a06ecl0tw9w6ky9xpqd4k7hl4phttq9lkdrhjffv08tc04yxf4pfexypwt0e8zlmdeuf4qqqsdt4qevd84nlmks62nzzz9swwpu"
/// ```
abstract interface class UriEvents {
  /// With [fromInstallReferrer], also replays the LexeConnect request (if any)
  /// that led the user to install the app; see [_installReferrerConnectUri].
  static Future<UriEvents> prod({bool fromInstallReferrer = false}) =>
      ProdUriEvents.init(fromInstallReferrer: fromInstallReferrer);

  /// If the app was started to handle this URI, then this will be non-null.
  String? get initialUri;

  /// While the app is running (either in the foreground or still alive in the
  /// background), any platform URI events will pop up here.
  ///
  /// The latest URI event will be cached, even if there is no active listener.
  ///
  /// The stream is technically a broadcast stream, so it supports multiple
  /// listeners, though ideally we should only have one listener.
  Stream<String> get uriStream;
}

@immutable
final class ProdUriEvents implements UriEvents {
  ///
  static Future<ProdUriEvents> init({required bool fromInstallReferrer}) async {
    final appLinks = app_links.AppLinks();

    // The plugin handler for initial link just returns a value immediately, but
    // since it calls across a plugin channel, it makes everything async.
    final result = await Result.tryAsync<String?, Exception>(
      appLinks.getInitialLinkString,
    );

    final String? initialUri;
    switch (result) {
      case Ok(:final ok):
        initialUri = ok;
      case Err(:final err):
        error("UriEvents: failed to init: $err");
        initialUri = null;
    }

    // We'll use `BehaviorSubject` to expose a stream that will cache the last
    // URI event, even if no-one is listening at the time.
    //
    // This stream should be open for the duration of the process.
    final events = BehaviorSubject<String>();

    final Result<Stream<String>, Exception> listenResult = Result.try_(
      () => appLinks.stringLinkStream,
    );

    final Stream<String> linkStream;
    switch (listenResult) {
      case Ok(:final ok):
        linkStream = ok;
      case Err(:final err):
        error("UriEvents: failed to init stream: $err");
        linkStream = const Stream.empty(broadcast: true);
    }

    // The install referrer yields to any link the platform delivered first.
    final referrerStream = fromInstallReferrer
        ? Stream.fromFuture(
            _installReferrerConnectUri(),
          ).where((uri) => uri != null && !events.hasValue).cast<String>()
        : const Stream<String>.empty();
    final uriStream = Rx.merge([linkStream, referrerStream]);

    // Spawn a task that pipes all URI events into `events`.
    unawaited(uriStream.log(id: "uriStream").pipe(events.sink));

    return ProdUriEvents(initialUri: initialUri, uriStream: events.stream);
  }

  const ProdUriEvents({required this.initialUri, required this.uriStream});

  @override
  final String? initialUri;

  @override
  final Stream<String> uriStream;
}

/// The LexeConnect request that led the user to install the app, if any.
///
/// Without Lexe installed, a `https://lexe.app/connect?...` link opens the
/// lexe.app fallback page, whose Android install button sends the link along
/// as the Play Store `referrer`. Play hands it back here after install.
Future<String?> _installReferrerConnectUri() async {
  // Only Play Store installs have one
  if (!Platform.isAndroid) return null;

  // Keep in sync with <app/android/app/src/main/kotlin/app/lexe/lexeapp/MainActivity.kt>
  const channel = MethodChannel("app.lexe/install_referrer");
  final result = await Result.tryAsync<String?, Exception>(
    () => channel
        .invokeMethod<String>("getInstallReferrer")
        .timeout(const Duration(seconds: 10)),
  );
  switch (result) {
    // Other installs carry Play's own referrer, e.g. "utm_medium=organic".
    case Ok(:final ok):
      return (ok != null && ok.startsWith("https://lexe.app/connect?"))
          ? ok
          : null;
    case Err(:final err):
      warn("UriEvents: failed to get install referrer: $err");
      return null;
  }
}
