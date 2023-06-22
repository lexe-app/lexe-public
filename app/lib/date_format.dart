// Date formatting helpers

import 'dart:core' show DateTime, String;

import 'package:duration/duration.dart' show prettyDuration;
import 'package:duration/locale.dart'
    show DurationLocale, EnglishDurationLocale;

import 'package:intl/intl.dart' show DateFormat;

const DurationLocale defaultLocale = EnglishDurationLocale();

/// Compactly format a `DateTime` that's in the past. Will return `null` if the
/// `DateTime` is in the future.
///
/// * Format spans shorter than 3 days in an abbreviated duration format, e.g.,
///   "10s", "3h", "2d".
///
/// * Format spans shorter than 6 months as an abbreviated date without the
///   year, e.g., "Jun 15", "Feb 3".
///
/// * Format longer spans as a compact date, e.g.,
///   "23/06/15" (formatting depends on the locale)
String? formatDateCompact({
  /// The time in the past that we want to format.
  required DateTime then,

  /// The current time, otherwise `DateTime.now()`. Used for testing.
  DateTime? now,

  /// Use `locale` instead of the current configured locale. Used for testing.
  String? locale,
}) {
  final DateTime now2 = now ?? DateTime.now();

  // Can't format dates in the future
  if (then.isAfter(now2)) {
    return null;
  }

  final span = now2.difference(then);

  if (span.inDays <= 3) {
    return prettyDuration(
      span,
      locale: lookupDurationLocale(locale),
      abbreviated: true,
      // first => only return the first section
      // e.g., "2d 5h 30m 12s" => "2d"
      first: true,
    );
  } else if (span.inDays <= 31 * 6) {
    return DateFormat.MMMd(locale).format(then);
  } else {
    // TODO(phlip9): locale-friendly way to get most compact MM/dd/yy
    return DateFormat('MM/dd/yy', locale).format(then);
  }
}

/// The locale names used by the `duration` dart package are almost all "short"
/// locale names w/o the country code. This lookup function:
///
/// 1. Looks up the exact locale passed in
/// 2. Looks up the first two characters of the locale passed in
/// 3. Otherwise defaults to the english locale
DurationLocale lookupDurationLocale(String? locale) {
  if (locale == null) {
    return defaultLocale;
  }

  final maybeLocale = DurationLocale.fromLanguageCode(locale);
  if (maybeLocale != null) {
    return maybeLocale;
  }

  if (locale.length <= 2) {
    return defaultLocale;
  }

  final shortLocale = locale.substring(0, 2);
  final maybeShortLocale = DurationLocale.fromLanguageCode(shortLocale);
  if (maybeShortLocale != null) {
    return maybeShortLocale;
  }

  return defaultLocale;
}
