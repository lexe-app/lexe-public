import 'dart:async' show unawaited;

import 'package:app_links/app_links.dart' as app_links;
import 'package:flutter/foundation.dart' show immutable;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/stream_ext.dart';
import 'package:rxdart/rxdart.dart' show BehaviorSubject;

/// An interface for receiving platform URI events. A URI event is when the user
/// taps e.g. a "lightning:" URI in a browser or other app, and then they or the
/// platform selects Lexe to handle this URI. We only receive events for URIs
/// that we are registered to handle.
///
/// See: <app/android/app/src/main/AndroidManifest.xml>
///      <app/ios/Runner/Info.plist>
///      <app/macos/Runner/Info.plist>
abstract interface class UriEvents {
  static Future<UriEvents> prod() => ProdUriEvents.init();

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
  static Future<ProdUriEvents> init() async {
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

    final Stream<String> uriStream;
    switch (listenResult) {
      case Ok(:final ok):
        uriStream = ok;
      case Err(:final err):
        error("UriEvents: failed to init stream: $err");
        uriStream = const Stream.empty(broadcast: true);
    }

    // Spawn a task that pipes all platform URI events into `events`.
    unawaited(uriStream.log(id: "uriStream").pipe(events.sink));

    return ProdUriEvents(initialUri: initialUri, uriStream: events.stream);
  }

  const ProdUriEvents({required this.initialUri, required this.uriStream});

  @override
  final String? initialUri;

  @override
  final Stream<String> uriStream;
}
