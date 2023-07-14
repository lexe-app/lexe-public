import 'dart:ui' show FontFeature, FontVariation, TextDecoration;

import 'package:flutter/material.dart'
    show
        Brightness,
        Color,
        ColorScheme,
        IconThemeData,
        MaterialColor,
        TextStyle,
        ThemeData,
        VisualDensity;
import 'package:flutter/services.dart' show SystemUiOverlayStyle;

class LxTheme {
  LxTheme._();

  static const SystemUiOverlayStyle systemOverlayStyleLight =
      SystemUiOverlayStyle(
    // From: SystemUiOverlayStyle.dark
    systemNavigationBarIconBrightness: Brightness.light,
    statusBarIconBrightness: Brightness.dark,
    statusBarBrightness: Brightness.light,
    // Lexe overrides
    statusBarColor: LxColors.background,
    systemNavigationBarColor: LxColors.background,
    systemNavigationBarDividerColor: LxColors.background,
  );

  static const SystemUiOverlayStyle systemOverlayStyleDark =
      SystemUiOverlayStyle(
    // From: SystemUiOverlayStyle.light
    systemNavigationBarIconBrightness: Brightness.light,
    statusBarIconBrightness: Brightness.light,
    statusBarBrightness: Brightness.dark,
    // Lexe overrides
    statusBarColor: LxColors.foreground,
    systemNavigationBarColor: LxColors.foreground,
    systemNavigationBarDividerColor: LxColors.foreground,
  );

  /// The Lexe light theme for `MaterialApp` compatibility
  static ThemeData light() {
    final colorScheme = ColorScheme.fromSwatch(
      primarySwatch: LxColors.greySwatch,
      brightness: Brightness.light,
    );

    final baseTheme = ThemeData.from(
      colorScheme: colorScheme,
      useMaterial3: true,
    );

    const appBarIconTheme = IconThemeData(
      color: LxColors.foreground,
      size: Fonts.size700,
    );

    return baseTheme.copyWith(
      visualDensity: VisualDensity.comfortable,
      scaffoldBackgroundColor: LxColors.background,
      appBarTheme: baseTheme.appBarTheme.copyWith(
        backgroundColor: LxColors.background,
        foregroundColor: LxColors.foreground,

        // Left align the title.
        centerTitle: false,

        // elevation = 0 removes the shadow under the app bar so it blends in
        // with the page background when nothing is scrolled under.
        elevation: 0.0,

        // by default, show line under app bar when content scrolls under
        // still not sure I like how this looks...
        scrolledUnderElevation: 1.0,
        shadowColor: LxColors.background,
        surfaceTintColor: LxColors.clearB0,

        // make the system bar use the same background color as the page
        systemOverlayStyle: LxTheme.systemOverlayStyleLight,

        iconTheme: appBarIconTheme,
        actionsIconTheme: appBarIconTheme,
      ),
      drawerTheme: baseTheme.drawerTheme.copyWith(
        // make the drawer blend with the system bar
        backgroundColor: LxColors.background,
        elevation: 0.0,
        // scrim is the transparent overlay that covers the underlying page to
        // the right of the drawer.
        scrimColor: LxColors.clearB200,
      ),
    );
  }
}

class LxColors {
  LxColors._();

  // A half-transparent red for debugging.

  static const Color debug = Color(0xaaff0000);

  // Reminder: Color hex is in ARGB 0xAARRGGBB

  /// LxColors.grey900
  static const Color background = LxColors.grey900;

  /// LxColors.grey200
  static const Color foreground = LxColors.grey200;

  /// LxColors.grey350
  static const Color fgSecondary = LxColors.grey350;

  /// LxColors.grey650
  static const Color fgTertiary = LxColors.grey650;

  // TODO(phlip9): need green and red swatches
  // TODO(phlip9): moneyGoUp will eventually need to be localized, since
  //               different cultures have different color associations.

  /// 0x14b87d - hsl(158deg 80% 40%)
  static const Color moneyGoUp = Color(0xff14b87d);

  /// 0xe9553e - hsl(8deg 80% 58%)
  static const Color errorText = Color(0xffe9553e);

  // Greys

  static const Color grey0 = Color(0xff000000);
  static const Color grey25 = Color(0xff020303);
  static const Color grey50 = Color(0xff050607);
  static const Color grey75 = Color(0xff090b0c);
  static const Color grey100 = Color(0xff0d1011);
  static const Color grey125 = Color(0xff111415);
  static const Color grey150 = Color(0xff141819);
  static const Color grey175 = Color(0xff181c1e);
  static const Color grey200 = Color(0xff1c2123);
  static const Color grey225 = Color(0xff212628);
  static const Color grey250 = Color(0xff262b2e);
  static const Color grey275 = Color(0xff2b3134);
  static const Color grey300 = Color(0xff31383b);
  static const Color grey325 = Color(0xff373f42);
  static const Color grey350 = Color(0xff3e464a);
  static const Color grey375 = Color(0xff454e52);
  static const Color grey400 = Color(0xff4c565a);
  static const Color grey425 = Color(0xff545e63);
  static const Color grey450 = Color(0xff5c676c);
  static const Color grey475 = Color(0xff647075);
  static const Color grey500 = Color(0xff6c797f);
  static const Color grey525 = Color(0xff748288);
  static const Color grey550 = Color(0xff7d8c92);
  static const Color grey575 = Color(0xff85959c);
  static const Color grey600 = Color(0xff8e9ea5);
  static const Color grey625 = Color(0xff96a7af);
  static const Color grey650 = Color(0xff9eb0b8);
  static const Color grey675 = Color(0xffa6b9c1);
  static const Color grey700 = Color(0xffaec2ca);
  static const Color grey725 = Color(0xffb6cad2);
  static const Color grey750 = Color(0xffc0d1d8);
  static const Color grey775 = Color(0xffcad8de);
  static const Color grey800 = Color(0xffd3dee3);
  static const Color grey825 = Color(0xffdbe4e8);
  static const Color grey850 = Color(0xffe3eaed);
  static const Color grey875 = Color(0xffe9eff1);
  static const Color grey900 = Color(0xffeff3f5);
  static const Color grey925 = Color(0xfff4f7f8);
  static const Color grey950 = Color(0xfff9fafb);
  static const Color grey975 = Color(0xfffcfdfd);
  static const Color grey1000 = Color(0xffffffff);

  /// This object is only used for compatibility with the MaterialApp theme.
  static const MaterialColor greySwatch = MaterialColor(
    0xff6c797f, // LxColors.grey500.value,
    <int, Color>{
      50: LxColors.grey50,
      100: LxColors.grey100,
      200: LxColors.grey200,
      300: LxColors.grey300,
      350: LxColors.grey350,
      400: LxColors.grey400,
      500: LxColors.grey500,
      600: LxColors.grey600,
      700: LxColors.grey700,
      800: LxColors.grey800,
      850: LxColors.grey850,
      900: LxColors.grey900,
    },
  );

  // White with transparency

  static const Color clearW0 = Color(0x00ffffff);
  static const Color clearW100 = Color(0x19ffffff);
  static const Color clearW200 = Color(0x33ffffff);
  static const Color clearW300 = Color(0x4cffffff);
  static const Color clearW400 = Color(0x66ffffff);
  static const Color clearW500 = Color(0x7fffffff);
  static const Color clearW600 = Color(0x99ffffff);
  static const Color clearW700 = Color(0xb2ffffff);
  static const Color clearW800 = Color(0xccffffff);
  static const Color clearW900 = Color(0xe5ffffff);
  static const Color clearW1000 = Color(0xffffffff);

  // Black with transparency

  static const Color clearB0 = Color(0x00000000);
  static const Color clearB50 = Color(0x0a000000);
  static const Color clearB100 = Color(0x19000000);
  static const Color clearB200 = Color(0x33000000);
  static const Color clearB300 = Color(0x4c000000);
  static const Color clearB400 = Color(0x66000000);
  static const Color clearB500 = Color(0x7f000000);
  static const Color clearB600 = Color(0x99000000);
  static const Color clearB700 = Color(0xb2000000);
  static const Color clearB800 = Color(0xcc000000);
  static const Color clearB900 = Color(0xe5000000);
  static const Color clearB1000 = Color(0xff000000);
}

class Space {
  Space._();

  /// 0 px
  static const double s0 = 0.0;

  /// 4 px
  static const double s100 = 4.0;

  /// 8 px
  static const double s200 = 8.0;

  /// 12 px
  static const double s300 = 12.0;

  /// 16 px
  static const double s400 = 16.0;

  /// 24 px
  static const double s500 = 24.0;

  /// 28 px
  static const double s550 = 28.0;

  /// 32 px
  static const double s600 = 32.0;

  /// 40 px
  static const double s650 = 40.0;

  /// 48 px
  static const double s700 = 48.0;

  /// 64 px
  static const double s800 = 64.0;

  /// 72 px
  static const double s825 = 72.0;

  /// 80 px
  static const double s850 = 80.0;

  /// 96 px
  static const double s900 = 96.0;

  /// 144 px
  static const double s1000 = 144.0;

  /// 192 px
  static const double s1100 = 192.0;

  /// 256 px
  static const double s1200 = 256.0;
}

class LxRadius {
  LxRadius._();

  /// 0 px
  static const double r0 = 0.0;

  /// 2 px
  static const double r100 = 2.0;

  /// 6 px
  static const double r200 = 6.0;

  /// 14 px
  static const double r300 = 14.0;

  /// 30 px
  static const double r400 = 30.0;

  /// 62 px
  static const double r500 = 62.0;
}

class Fonts {
  Fonts._();

  /// 12 px
  static const double size100 = 12.0;

  /// 14 px
  static const double size200 = 14.0;

  /// 16 px
  static const double size300 = 16.0;

  /// 18 px
  static const double size400 = 18.0;

  /// 20 px
  static const double size500 = 20.0;

  /// 24 px
  static const double size600 = 24.0;

  /// 30 px
  static const double size700 = 30.0;

  /// 40 px
  static const double size800 = 40.0;

  static const FontVariation weightThin = FontVariation("wght", 100);
  static const FontVariation weightExtraLight = FontVariation("wght", 200);
  static const FontVariation weightLight = FontVariation("wght", 300);
  static const FontVariation weightNormal = FontVariation("wght", 400);
  static const FontVariation weightMedium = FontVariation("wght", 500);
  static const FontVariation weightSemiBold = FontVariation("wght", 600);
  static const FontVariation weightBold = FontVariation("wght", 700);
  static const FontVariation weightExtraBold = FontVariation("wght", 800);
  static const FontVariation weightBlack = FontVariation("wght", 900);

  static const FontVariation widthTight = FontVariation("wdth", 90);

  /// Slashed zero
  static const FontFeature featSlashedZero = FontFeature.slashedZero();

  /// Stylistic set 2: Disambiguation
  ///
  /// Alternate glyph set that increases visual difference between
  /// similar-looking characters.
  ///
  /// <https://rsms.me/inter/#features/ss02>
  static const FontFeature featDisambugation = FontFeature("ss02");

  // static const FontFeature featTabularNumbers = FontFeature.tabularFigures();

  static const TextStyle fontInter = TextStyle(
    debugLabel: "Fonts.fontInter",
    fontFamily: "Inter V",
    decoration: TextDecoration.none,
  );

  static const TextStyle fontHubot = TextStyle(
    debugLabel: "Fonts.fontHubot",
    fontFamily: "Hubot Sans",
    decoration: TextDecoration.none,
  );

  static const TextStyle fontBody = TextStyle(
    debugLabel: "Fonts.fontBody",
    fontFamily: "Inter V",
    fontSize: size300,
    color: LxColors.foreground,
    height: 1.7,
    fontVariations: [weightNormal],
    decoration: TextDecoration.none,
  );

  static const TextStyle fontUI = TextStyle(
    debugLabel: "Fonts.fontUI",
    fontFamily: "Inter V",
    fontSize: size300,
    color: LxColors.foreground,
    height: 1.0,
    // fontFeatures: [slashedZero],
    fontVariations: [weightNormal],
    decoration: TextDecoration.none,
  );

  static const TextStyle fontHero = TextStyle(
    debugLabel: "Fonts.fontHero",
    fontFamily: "Hubot Sans",
    fontSize: size800,
    color: LxColors.foreground,
    height: 1.5,
    fontVariations: [weightBold, widthTight],
    decoration: TextDecoration.none,
  );
}
