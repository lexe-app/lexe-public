import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/date_format.dart' as date_format;

void main() {
  test("date_format.formatDateFull", () async {
    DateTime dateTimeFromUnix(int msSinceEpoch) =>
        DateTime.fromMillisecondsSinceEpoch(msSinceEpoch, isUtc: true);

    expect(
      date_format.formatDateFullInner(dateTimeFromUnix(1687385095000)),
      "2023-06-21 22:04:55",
    );
  });

  test("date_format.formatDate", () async {
    await date_format.initializeDateLocaleData();

    DateTime dateTimeFromUnix(int msSinceEpoch) =>
        DateTime.fromMillisecondsSinceEpoch(msSinceEpoch, isUtc: true);

    // now = "Jun 21, 2023"
    final now = dateTimeFromUnix(1687385095000);

    // now = "just now"
    expect(
      date_format.formatDate(now: now, then: now, locale: "en_US"),
      "just now",
    );
    expect(
      date_format.formatDate(now: now, then: now, locale: "nb"),
      "just now",
    );
    expect(
      date_format.formatDate(now: now, then: now, locale: "fr"),
      "just now",
    );

    // -2d 15h 5m 3s = "2 days ago"
    final days2 = dateTimeFromUnix(1687157992000);
    expect(
      date_format.formatDate(now: now, then: days2, locale: "en_US"),
      "2 days ago",
    );
    expect(
      date_format.formatDate(now: now, then: days2, locale: "nb"),
      "2 dager ago",
    );
    expect(
      date_format.formatDate(now: now, then: days2, locale: "fr"),
      "2 jours ago",
    );

    // -15h 5m 3s = "15 hours ago"
    final hours15 = dateTimeFromUnix(1687330792000);
    expect(
      date_format.formatDate(now: now, then: hours15, locale: "en_US"),
      "15 hours ago",
    );
    expect(
      date_format.formatDate(now: now, then: hours15, locale: "nb"),
      "15 timer ago",
    );
    expect(
      date_format.formatDate(now: now, then: hours15, locale: "fr"),
      "15 heures ago",
    );

    // -5m 3s = "5 minutes ago"
    final min5 = dateTimeFromUnix(1687384792000);
    expect(
      date_format.formatDate(now: now, then: min5, locale: "en_US"),
      "5 minutes ago",
    );
    expect(
      date_format.formatDate(now: now, then: min5, locale: "nb"),
      "5 minutter ago",
    );
    expect(
      date_format.formatDate(now: now, then: min5, locale: "fr"),
      "5 minutes ago",
    );

    // -15 secs = "just now"
    final secs15 = dateTimeFromUnix(1687385080000);
    expect(
      date_format.formatDate(now: now, then: secs15, locale: "en_US"),
      "just now",
    );
    expect(
      date_format.formatDate(now: now, then: secs15, locale: "nb"),
      "just now",
    );
    expect(
      date_format.formatDate(now: now, then: secs15, locale: "fr"),
      "just now",
    );

    // +15s (in the future) = "just now"
    final secs15Fut = dateTimeFromUnix(1687385110000);
    expect(
      date_format.formatDate(now: now, then: secs15Fut, locale: "en_US"),
      "just now",
    );
    expect(
      date_format.formatDate(now: now, then: secs15Fut, locale: "nb"),
      "just now",
    );
    expect(
      date_format.formatDate(now: now, then: secs15Fut, locale: "fr"),
      "just now",
    );

    // -5d ish = June 16, 2023 = "Jun 16"
    final jun16 = dateTimeFromUnix(1686938392000);
    expect(
      date_format.formatDate(now: now, then: jun16, locale: "en_US"),
      "Jun 16",
    );
    expect(
      date_format.formatDate(now: now, then: jun16, locale: "nb"),
      "16. juni",
    );
    expect(
      date_format.formatDate(now: now, then: jun16, locale: "fr"),
      "16 juin",
    );

    // -75d = April 7, 2023 = "Apr 7"
    final apr7 = dateTimeFromUnix(1680890392000);
    expect(
      date_format.formatDate(now: now, then: apr7, locale: "en_US"),
      "Apr 7",
    );
    expect(
      date_format.formatDate(now: now, then: apr7, locale: "nb"),
      "7. apr.",
    );
    expect(
      date_format.formatDate(now: now, then: apr7, locale: "fr"),
      "7 avr.",
    );

    // -180d = December 23, 2022 = "Dec 23"
    final dec23 = dateTimeFromUnix(1671818392000);
    expect(
      date_format.formatDate(now: now, then: dec23, locale: "en_US"),
      "Dec 23",
    );
    expect(
      date_format.formatDate(now: now, then: dec23, locale: "nb"),
      "23. des.",
    );
    expect(
      date_format.formatDate(now: now, then: dec23, locale: "fr"),
      "23 déc.",
    );

    // -200d = December 3 2022 = "12/03/22"
    final dec3_22 = dateTimeFromUnix(1670090392000);
    expect(
      date_format.formatDate(now: now, then: dec3_22, locale: "en_US"),
      "12/3/2022",
    );
    expect(
      date_format.formatDate(now: now, then: dec3_22, locale: "nb"),
      "3.12.2022",
    );
    expect(
      date_format.formatDate(now: now, then: dec3_22, locale: "fr"),
      "03/12/2022",
    );

    // -654d = September 5, 2021 = "09/05/21"
    final sep5_22 = dateTimeFromUnix(1630864792000);
    expect(
      date_format.formatDate(now: now, then: sep5_22, locale: "en_US"),
      "9/5/2021",
    );
    expect(
      date_format.formatDate(now: now, then: sep5_22, locale: "nb"),
      "5.9.2021",
    );
    expect(
      date_format.formatDate(now: now, then: sep5_22, locale: "fr"),
      "05/09/2021",
    );
  });

  test("date_format.formatDateCompact", () async {
    await date_format.initializeDateLocaleData();

    DateTime dateTimeFromUnix(int msSinceEpoch) =>
        DateTime.fromMillisecondsSinceEpoch(msSinceEpoch, isUtc: true);

    // now = "Jun 21, 2023"
    final now = dateTimeFromUnix(1687385095000);

    // -2d 15h 5m 3s = "2d"
    final days2 = dateTimeFromUnix(1687157992000);
    expect(
      date_format.formatDateCompact(now: now, then: days2, locale: "en_US"),
      "2d",
    );
    expect(
      date_format.formatDateCompact(now: now, then: days2, locale: "nb"),
      "2d",
    );
    expect(
      date_format.formatDateCompact(now: now, then: days2, locale: "fr"),
      "2j",
    );

    // -15h 5m 3s = "15h"
    final hours15 = dateTimeFromUnix(1687330792000);
    expect(
      date_format.formatDateCompact(now: now, then: hours15, locale: "en_US"),
      "15h",
    );
    expect(
      date_format.formatDateCompact(now: now, then: hours15, locale: "nb"),
      "15t",
    );
    expect(
      date_format.formatDateCompact(now: now, then: hours15, locale: "fr"),
      "15h",
    );

    // -5m 3s = "5min"
    final min5 = dateTimeFromUnix(1687384792000);
    expect(
      date_format.formatDateCompact(now: now, then: min5, locale: "en_US"),
      "5min",
    );
    expect(
      date_format.formatDateCompact(now: now, then: min5, locale: "nb"),
      "5m",
    );
    expect(
      date_format.formatDateCompact(now: now, then: min5, locale: "fr"),
      "5min",
    );

    // -15 secs = "just now"
    final secs15 = dateTimeFromUnix(1687385080000);
    expect(
      date_format.formatDateCompact(now: now, then: secs15, locale: "en_US"),
      "just now",
    );
    expect(
      date_format.formatDateCompact(now: now, then: secs15, locale: "nb"),
      "just now",
    );
    expect(
      date_format.formatDateCompact(now: now, then: secs15, locale: "fr"),
      "just now",
    );

    // -5d ish = June 16, 2023 = "Jun 16"
    final jun16 = dateTimeFromUnix(1686938392000);
    expect(
      date_format.formatDateCompact(now: now, then: jun16, locale: "en_US"),
      "Jun 16",
    );
    expect(
      date_format.formatDateCompact(now: now, then: jun16, locale: "nb"),
      "16. juni",
    );
    expect(
      date_format.formatDateCompact(now: now, then: jun16, locale: "fr"),
      "16 juin",
    );

    // -75d = April 7, 2023 = "Apr 7"
    final apr7 = dateTimeFromUnix(1680890392000);
    expect(
      date_format.formatDateCompact(now: now, then: apr7, locale: "en_US"),
      "Apr 7",
    );
    expect(
      date_format.formatDateCompact(now: now, then: apr7, locale: "nb"),
      "7. apr.",
    );
    expect(
      date_format.formatDateCompact(now: now, then: apr7, locale: "fr"),
      "7 avr.",
    );

    // -180d = December 23, 2022 = "Dec 23"
    final dec23 = dateTimeFromUnix(1671818392000);
    expect(
      date_format.formatDateCompact(now: now, then: dec23, locale: "en_US"),
      "Dec 23",
    );
    expect(
      date_format.formatDateCompact(now: now, then: dec23, locale: "nb"),
      "23. des.",
    );
    expect(
      date_format.formatDateCompact(now: now, then: dec23, locale: "fr"),
      "23 déc.",
    );

    // -200d = December 3 2022 = "12/03/22"
    final dec3_22 = dateTimeFromUnix(1670090392000);
    expect(
      date_format.formatDateCompact(now: now, then: dec3_22, locale: "en_US"),
      "12/3/2022",
    );
    expect(
      date_format.formatDateCompact(now: now, then: dec3_22, locale: "nb"),
      "3.12.2022",
    );
    expect(
      date_format.formatDateCompact(now: now, then: dec3_22, locale: "fr"),
      "03/12/2022",
    );

    // -654d = September 5, 2021 = "09/05/21"
    final sep5_22 = dateTimeFromUnix(1630864792000);
    expect(
      date_format.formatDateCompact(now: now, then: sep5_22, locale: "en_US"),
      "9/5/2021",
    );
    expect(
      date_format.formatDateCompact(now: now, then: sep5_22, locale: "nb"),
      "5.9.2021",
    );
    expect(
      date_format.formatDateCompact(now: now, then: sep5_22, locale: "fr"),
      "05/09/2021",
    );
  });
}
