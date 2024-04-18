import 'package:flutter/foundation.dart' as foundation;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;
import 'package:lexeapp/logger.dart';
import 'package:material_symbols_icons/symbols.dart';

/// Our global flutter theme overrides.
///
/// Ideally, most of our components inherit solid default styling from here, to
/// reduce per-component style drift and copy-paste errors.
///
/// While modifying or debugging these global stylings during development, it's
/// helpful to wrap the page or component in a
/// `Theme(data: LxTheme.light(), child: ...)` so that hot-reloading works.
class LxTheme {
  LxTheme._();

  // These [SystemUiOverlayStyle] define the colors for the system top-bar and
  // bottom-bar while our app is open. These are different than e.g. the
  // [AppBar] in that we don't define and render these in the app, only describe
  // how they should be styled to the OS.

  // theme: light, icons: dark, background: light
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

  // theme: light, icons: dark, background: clear
  static const SystemUiOverlayStyle systemOverlayStyleLightClearBg =
      SystemUiOverlayStyle(
    // From: SystemUiOverlayStyle.dark
    systemNavigationBarIconBrightness: Brightness.light,
    statusBarIconBrightness: Brightness.dark,
    statusBarBrightness: Brightness.light,
    // Lexe overrides
    statusBarColor: LxColors.clearW0,
    systemNavigationBarColor: LxColors.clearW0,
    systemNavigationBarDividerColor: LxColors.clearW0,
  );

  // theme: dark, icons: light, background: dark
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

  // theme: dark, icons: light, background: clear
  static const SystemUiOverlayStyle systemOverlayStyleDarkClearBg =
      SystemUiOverlayStyle(
    // From: SystemUiOverlayStyle.light
    systemNavigationBarIconBrightness: Brightness.light,
    statusBarIconBrightness: Brightness.light,
    statusBarBrightness: Brightness.dark,
    // Lexe overrides
    statusBarColor: LxColors.clearB0,
    systemNavigationBarColor: LxColors.clearB0,
    systemNavigationBarDividerColor: LxColors.clearB0,
  );

  /// The global, Lexe-specific light theme.
  static ThemeData light() {
    // Derive a basic colorscheme from our grey colors.
    final colorScheme = ColorScheme.fromSwatch(
      primarySwatch: LxColors.greySwatch,
      brightness: Brightness.light,
    );

    // Text styling
    final typography = Typography.material2021(
      platform: foundation.defaultTargetPlatform,
      colorScheme: colorScheme,
    );

    // TODO(phlip9): need to tweak these...
    // https://m3.material.io/styles/typography/type-scale-tokens

    final textTheme = typography.black
        .apply(
          fontFamily: "Inter V",
          displayColor: LxColors.foreground,
          bodyColor: LxColors.foreground,
        )
        .copyWith(
          headlineSmall: Fonts.fontHeadlineSmall,
        );

    // Start with a basic theme generated from our greyscale colors. This will
    // provide somewhat reasonable default styling for things that we haven't
    // explicitly styled ourselves.
    final baseTheme = ThemeData.from(
      colorScheme: colorScheme,
      useMaterial3: true,
      textTheme: textTheme,
    );

    const appBarIconTheme = IconThemeData(
      color: LxColors.foreground,
      size: Fonts.size700,
    );

    return baseTheme.copyWith(
      visualDensity: VisualDensity.comfortable,
      scaffoldBackgroundColor: LxColors.background,
      brightness: Brightness.light,

      iconTheme: baseTheme.iconTheme.copyWith(
        color: LxColors.foreground,
        weight: LxIcons.weightSemiBold,
      ),

      // [AppBar]
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

      // [Drawer]
      drawerTheme: baseTheme.drawerTheme.copyWith(
        // make the drawer blend with the system bar
        backgroundColor: LxColors.background,
        elevation: 0.0,
        // scrim is the transparent overlay that covers the underlying page to
        // the right of the drawer.
        scrimColor: LxColors.clearB200,
      ),

      // [OutlinedButton]
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: LxColors.foreground,
          backgroundColor: LxColors.clearB0,
          disabledForegroundColor: LxColors.fgTertiary,
          disabledBackgroundColor: LxColors.clearB0,
          padding: const EdgeInsets.all(Space.s450),
          minimumSize: const Size.square(Fonts.size400 + 2 * Space.s450),
          maximumSize: const Size.fromHeight(Fonts.size400 + 2 * Space.s450),
          textStyle: Fonts.fontButton,
        ).copyWith(
          // Place dynamic styles here, i.e., styles that should change in
          // different button states (ex: normal, focused, disabled, hover, ...)

          // deemphasize disabled button border
          side: MaterialStateProperty.resolveWith((Set<MaterialState> states) {
            // disabled => deemphasized border
            if (states.contains(MaterialState.disabled)) {
              return const BorderSide(color: LxColors.fgTertiary, width: 2.0);
            }
            // normal
            return const BorderSide(color: LxColors.foreground, width: 2.0);
          }),
        ),
      ),

      // [FilledButton]
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          foregroundColor: LxColors.foreground,
          backgroundColor: LxColors.grey1000,
          disabledForegroundColor: LxColors.fgTertiary,
          disabledBackgroundColor: LxColors.grey850,
          padding: const EdgeInsets.all(Space.s450),
          minimumSize: const Size.square(Fonts.size400 + 2 * Space.s450),
          maximumSize: const Size.fromHeight(Fonts.size400 + 2 * Space.s450),
          textStyle: Fonts.fontButton,
          side: const BorderSide(color: LxColors.clearB0, width: 0.0),
        ),
      ),

      // [Radio] button
      radioTheme: RadioThemeData(
        fillColor: MaterialStateProperty.resolveWith((states) =>
            (states.contains(MaterialState.disabled))
                ? LxColors.fgTertiary
                : LxColors.foreground),
      ),

      // [ListTile]
      listTileTheme: ListTileThemeData(
        minVerticalPadding: Space.s200,
        titleTextStyle: Fonts.fontUI.copyWith(
          fontSize: Fonts.size300,
          height: 1.5,
          fontVariations: [Fonts.weightMedium],
        ),
        subtitleTextStyle: Fonts.fontUI.copyWith(
          fontSize: Fonts.size200,
          height: 1.25,
          color: LxColors.grey450,
        ),
        leadingAndTrailingTextStyle: Fonts.fontUI.copyWith(
          fontSize: Fonts.size300,
        ),
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

  // TODO(phlip9): I think our fonts are too low contrast w/ e.g. fgTertiary.
  // Accessibility-wise, we should probably limit to min. grey500 for headings
  // and grey450 for smaller.

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

  // static const Color errorText = Color(0xffe9553e); // < looks good w/ Failed
  // static const Color errorText = Color(0xffd3302f); // < default material
  // static const Color errorText = Color(0xff994133);
  static const Color errorText = Color(0xffb82a14);

  /// hsl(257deg 95% 68%)
  static const Color linkText = Color(0xff8d60fb);

  /// hsl(158deg 95% 40%)
  // static Color linkText = HSLColor.fromAHSL(1.0, 158.0, 0.95, 0.35).toColor();
  /// hsl(214deg 94% 50%)
  // static const Color linkText = Color(0xff0870f7);

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

  /// 64 px
  static const double appBarLeadingWidth = 64.0;

  /// 16 px
  static const double appBarTrailingPadding = Space.s400;

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

  /// 20 px
  static const double s450 = 20.0;

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

  /// 56 px
  static const double s750 = 56.0;

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

  /// 36 px
  static const double size800 = 36.0;

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

  static const FontVariation italic = FontVariation("ital", 10);

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
    fontSize: Fonts.size300,
    color: LxColors.foreground,
    height: 1.5,
    fontVariations: [Fonts.weightNormal],
    decoration: TextDecoration.none,
  );

  static const TextStyle fontUI = TextStyle(
    debugLabel: "Fonts.fontUI",
    fontFamily: "Inter V",
    fontSize: Fonts.size300,
    color: LxColors.foreground,
    height: 1.0,
    // fontFeatures: [slashedZero],
    fontVariations: [Fonts.weightNormal],
    decoration: TextDecoration.none,
  );

  static const TextStyle fontButton = TextStyle(
    debugLabel: "Fonts.fontButton",
    fontFamily: "Inter V",
    fontSize: Fonts.size400,
    height: 1.0,
    fontVariations: [Fonts.weightMedium],
    decoration: TextDecoration.none,
  );

  static const TextStyle fontHeadlineSmall = TextStyle(
    debugLabel: "Fonts.fontHeadlineSmall",
    fontFamily: "Inter V",
    fontSize: Fonts.size600,
    height: 1.2,
    fontVariations: [Fonts.weightMedium],
    decoration: TextDecoration.none,
    letterSpacing: -0.5,
  );

  static const TextStyle fontHero = TextStyle(
    debugLabel: "Fonts.fontHero",
    fontFamily: "Hubot Sans",
    fontSize: Fonts.size800,
    color: LxColors.foreground,
    height: 1.5,
    fontVariations: [Fonts.weightBold, Fonts.widthTight],
    decoration: TextDecoration.none,
  );

  static const TextStyle fontLogo = TextStyle(
    debugLabel: "Fonts.fontHero",
    fontFamily: "Hubot Sans",
    fontSize: Fonts.size800,
    color: LxColors.foreground,
    height: 1.0,
    fontVariations: [Fonts.weightBold, Fonts.italic],
    letterSpacing: -0.6,
    decoration: TextDecoration.none,
  );
}

/// All icons Lexe app uses.
final class LxIcons {
  const LxIcons._();

  //
  // Icon weights (correspond w/ Fonts.weightXXX)
  //

  /// 100
  static const double weightThin = 100;

  /// 200
  static const double weightExtraLight = 200;

  /// 300
  static const double weightLight = 300;

  /// 400
  static const double weightNormal = 400;

  /// 500
  static const double weightMedium = 500;

  /// 600
  static const double weightSemiBold = 600;

  /// 700
  static const double weightBold = 700;

  /// 800
  static const double weightExtraBold = 800;

  /// 900
  static const double weightBlack = 900;

  //
  // Grade
  //
  // Both grade and weight affect icon thickness. Use grade for fine-grained
  // adjustments.

  /// -25
  static const double gradeLight = -25;

  /// 0
  static const double gradeNormal = 0;

  /// 25
  static const double gradeMedium = 25;

  //
  // Optical size
  //
  // Use optical size to ensure an icon has the same perceived weight at
  // different sizes.

  /// 20dp
  static const double opszDense = 20;

  /// 28dp
  static const double opszSemiDense = 28;

  /// 40dp
  static const double opszSemiComfort = 40;

  /// 48dp
  static const double opszComfort = 48;

  //
  // Standard Lexe icons
  //

  /// Menu icon (≡ hamburger menu)
  static const IconData menu = Symbols.menu_rounded;

  /// Page or dialogue close icon (x icon)
  static const IconData close = Symbols.close_rounded;

  /// <- back icon (left arrow)
  static const IconData back = Symbols.arrow_back_rounded;

  /// < more subdued back icon (left caret)
  static const IconData backSecondary = Symbols.chevron_left_rounded;

  /// -> next icon (right arrow)
  static const IconData next = Symbols.arrow_forward_rounded;

  /// > more subdued next icon (right caret)
  static const IconData nextSecondary = Symbols.chevron_right_rounded;

  /// refresh icon (spin arrow)
  static const IconData refresh = Symbols.refresh_rounded;

  /// Receive payment icon (down arrow)
  static const IconData receive = Symbols.arrow_downward_rounded;

  /// Send payment icon (up arrow)
  static const IconData send = Symbols.arrow_upward_rounded;

  /// Expand up (up arrow)
  static const IconData expandUp = Symbols.arrow_upward_rounded;

  /// Empty scanner (scan box)
  static const IconData scan = Symbols.crop_free_rounded;

  /// Scan box with QR code inside (scan box with qr inside)
  static const IconData scanDetailed = Symbols.qr_code_scanner_rounded;

  /// Edit icon (pen in square)
  static const IconData edit = Symbols.edit_square_rounded;

  /// Share icon (network thing)
  static const IconData share = Symbols.share_rounded;

  /// Copy icon (stacked boxes)
  static const IconData copy = Symbols.content_copy_rounded;

  /// Add icon (+ icon)
  static const IconData add = Symbols.add_rounded;

  /// More actions icon (3 horizontal dots)
  static const IconData moreHoriz = Symbols.more_horiz_rounded;

  /// Settings icon (gear)
  static const IconData settings = Symbols.settings_rounded;

  /// Backup icon (cloud with up arrow)
  static const IconData backup = Symbols.backup_rounded;

  /// Security icon (outlined lock)
  static const IconData security = Symbols.lock_outline_rounded;

  /// Support/help icon (? in circle)
  static const IconData support = Symbols.help_outline_rounded;

  /// Debug symbol (bug)
  static const IconData debug = Symbols.bug_report_rounded;

  /// Success symbol, used inside a small badge (checkmark)
  static const IconData completedBadge = Symbols.check_rounded;

  /// Pending symbol, used inside a small badge (two circle arrows / syncing)
  static const IconData pendingBadge = Symbols.sync_rounded;

  /// Error symbol, used inside a small badge (x icon)
  static const IconData failedBadge = Symbols.close_rounded;

  /// Bitcoin symbol icon (₿/B currency symbol)
  static const IconData bitcoin = Symbols.currency_bitcoin_rounded;

  /// Lightning symbol icon (lightning bolt)
  static const IconData lightning = Symbols.bolt_rounded;
}
