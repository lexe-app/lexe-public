/// URL helpers
library;

import 'package:lexeapp/prelude.dart';
import 'package:url_launcher/url_launcher.dart' as url_launcher;

/// Open [url] in an external application. If the url is http(s) then this is
/// likely a browser.
///
/// Returns `true` if we opened the URL successfully. Returns `false` if there
/// is no provider available for this URI scheme.
Future<Result<bool, Exception>> open(final String url) async {
  final result = await Result.tryAsync<bool, Exception>(
    () => url_launcher.launchUrl(
      Uri.parse(url),
      mode: url_launcher.LaunchMode.externalApplication,
    ),
  );
  return result.inspectErr(
    (err) => warn("Failed to open URL: '$url', err: $err"),
  );
}
