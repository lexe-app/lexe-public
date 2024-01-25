// Date formatting helpers

import 'package:duration/duration.dart' show prettyDuration;
import 'package:duration/locale.dart'
    show DurationLocale, EnglishDurationLocale;

import 'package:intl/date_symbol_data_local.dart' as date_symbol_data_local;
import 'package:intl/intl.dart' show DateFormat, Intl;

const DurationLocale defaultDurationLocale = EnglishDurationLocale();

/// Initializes locale data (like translated months and days) for ALL locales.
/// If using any locale other than `en_US`, this method MUST be called before
/// calling any date formatting functions.
///
/// This approach is extremely simple but adds a bit of binary size (a few
/// hundred KiB I think). In the future, if we really wanted to squeeze out
/// every drop of wasted space, we could lazily download+cache only the data
/// needed for the  client's specific locale.
Future<void> initializeDateLocaleData() async {
  await date_symbol_data_local.initializeDateFormatting();
}

/// Compactly format a `DateTime` that's in the past. Will return `null` if the
/// `DateTime` is in the future.
///
/// The underlying `Intl` library will throw an error if
/// `initializeDateLocaleData()` hasn't been called yet (and the locale isn't
/// the default `en_US`).
///
/// * Format spans shorter than 3 days in an abbreviated duration format, e.g.,
///   "10s", "3h", "2d".
///
/// * Format spans shorter than 6 months as an abbreviated date without the
///   year, e.g., "Jun 15", "Feb 3".
///
/// * Format longer spans as a compact date, e.g.,
///   "6/15/2023" (formatting depends on the locale)
String? formatDateCompact({
  /// The time in the past that we want to format.
  required DateTime then,

  /// The current time, otherwise `DateTime.now()`. Used for testing.
  DateTime? now,

  /// Use `locale` instead of the current configured locale. Used for testing.
  String? locale,

  // For UI that shows several durations updating dynamically, it can be
  // distracting to show the seconds ticking across many elements. Setting this
  // option to `false` will format short durations (under a minute) as
  // "just now" instead of e.g. "15s".
  bool formatSeconds = true,
}) {
  final DateTime now2 = now ?? DateTime.now();

  // Can't format dates in the future
  if (then.isAfter(now2)) {
    return null;
  }

  final span = now2.difference(then);

  if (!formatSeconds && span.inSeconds < 60) {
    // TODO(phlip9): internationalize
    return "just now";
  } else if (span.inDays <= 3) {
    return formatDurationCompact(span, abbreviated: true, locale: locale);
  } else if (span.inDays <= 31 * 6) {
    return DateFormat.MMMd(locale).format(then);
  } else {
    return DateFormat.yMd(locale).format(then);
  }
}

String formatDurationCompact(
  Duration duration, {
  bool abbreviated = false,
  String? locale,
}) =>
    prettyDuration(
      duration,
      locale: lookupDurationLocale(locale),
      abbreviated: abbreviated,
      first: true,
    );

/// The locale names used by the `duration` dart package are almost all "short"
/// locale names w/o the country code. This lookup function:
///
/// 1. If `locale` is null, then lookup based on `Intl.getCurrentLocale()`
/// 2. Looks up full locale string
/// 3. Looks up first two characters of the locale string
/// 4. Otherwise defaults to the english locale
DurationLocale lookupDurationLocale(String? locale) {
  locale ??= Intl.getCurrentLocale();

  final maybeLocale = DurationLocale.fromLanguageCode(locale);
  if (maybeLocale != null) {
    return maybeLocale;
  }

  if (locale.length < 2) {
    return defaultDurationLocale;
  }

  final shortLocale = locale.substring(0, 2);
  final maybeShortLocale = DurationLocale.fromLanguageCode(shortLocale);
  if (maybeShortLocale != null) {
    return maybeShortLocale;
  }

  return defaultDurationLocale;
}
