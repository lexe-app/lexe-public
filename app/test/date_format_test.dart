import 'package:flutter_test/flutter_test.dart' show expect, test;

import 'package:lexeapp/date_format.dart' as date_format;

void main() {
  test("date_format.formatDateCompact", () async {
    await date_format.initializeDateLocaleData();

    // now = "Jun 21, 2023"
    final now = DateTime.fromMillisecondsSinceEpoch(1687385095000);

    // -2d 15h 5m 3s = "2d"
    final days2 = DateTime.fromMillisecondsSinceEpoch(1687157992000);
    expect(
        date_format.formatDateCompact(now: now, then: days2, locale: "en_US"),
        "2d");
    expect(date_format.formatDateCompact(now: now, then: days2, locale: "nb"),
        "2d");
    expect(date_format.formatDateCompact(now: now, then: days2, locale: "fr"),
        "2j");

    // -15h 5m 3s = "2d"
    final hours15 = DateTime.fromMillisecondsSinceEpoch(1687330792000);
    expect(
        date_format.formatDateCompact(now: now, then: hours15, locale: "en_US"),
        "15h");
    expect(date_format.formatDateCompact(now: now, then: hours15, locale: "nb"),
        "15t");
    expect(date_format.formatDateCompact(now: now, then: hours15, locale: "fr"),
        "15h");

    // -5m 3s = "5min"
    final min5 = DateTime.fromMillisecondsSinceEpoch(1687384792000);
    expect(date_format.formatDateCompact(now: now, then: min5, locale: "en_US"),
        "5min");
    expect(date_format.formatDateCompact(now: now, then: min5, locale: "nb"),
        "5m");
    expect(date_format.formatDateCompact(now: now, then: min5, locale: "fr"),
        "5min");

    // -15 secs = "15s"
    final secs15 = DateTime.fromMillisecondsSinceEpoch(1687385080000);
    expect(
        date_format.formatDateCompact(now: now, then: secs15, locale: "en_US"),
        "15s");
    expect(date_format.formatDateCompact(now: now, then: secs15, locale: "nb"),
        "15s");
    expect(date_format.formatDateCompact(now: now, then: secs15, locale: "fr"),
        "15s");

    // -5d ish = June 16, 2023 = "Jun 16"
    final jun16 = DateTime.fromMillisecondsSinceEpoch(1686938392000);
    expect(
        date_format.formatDateCompact(now: now, then: jun16, locale: "en_US"),
        "Jun 16");
    expect(date_format.formatDateCompact(now: now, then: jun16, locale: "nb"),
        "16. jun.");
    expect(date_format.formatDateCompact(now: now, then: jun16, locale: "fr"),
        "16 juin");

    // -75d = April 7, 2023 = "Apr 7"
    final apr7 = DateTime.fromMillisecondsSinceEpoch(1680890392000);
    expect(date_format.formatDateCompact(now: now, then: apr7, locale: "en_US"),
        "Apr 7");
    expect(date_format.formatDateCompact(now: now, then: apr7, locale: "nb"),
        "7. apr.");
    expect(date_format.formatDateCompact(now: now, then: apr7, locale: "fr"),
        "7 avr.");

    // -180d = December 23, 2022 = "Dec 23"
    final dec23 = DateTime.fromMillisecondsSinceEpoch(1671818392000);
    expect(
        date_format.formatDateCompact(now: now, then: dec23, locale: "en_US"),
        "Dec 23");
    expect(date_format.formatDateCompact(now: now, then: dec23, locale: "nb"),
        "23. des.");
    expect(date_format.formatDateCompact(now: now, then: dec23, locale: "fr"),
        "23 déc.");

    // -200d = December 3 2022 = "12/03/22"
    final dec3_22 = DateTime.fromMillisecondsSinceEpoch(1670090392000);
    expect(
        date_format.formatDateCompact(now: now, then: dec3_22, locale: "en_US"),
        "12/3/2022");
    expect(date_format.formatDateCompact(now: now, then: dec3_22, locale: "nb"),
        "3.12.2022");
    expect(date_format.formatDateCompact(now: now, then: dec3_22, locale: "fr"),
        "03/12/2022");

    // -654d = September 5, 2021 = "09/05/21"
    final sep5_22 = DateTime.fromMillisecondsSinceEpoch(1630864792000);
    expect(
        date_format.formatDateCompact(now: now, then: sep5_22, locale: "en_US"),
        "9/5/2021");
    expect(date_format.formatDateCompact(now: now, then: sep5_22, locale: "nb"),
        "5.9.2021");
    expect(date_format.formatDateCompact(now: now, then: sep5_22, locale: "fr"),
        "05/09/2021");
  });
}
