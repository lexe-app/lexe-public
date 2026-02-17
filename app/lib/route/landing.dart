import 'dart:async' show unawaited;
import 'dart:math' show max;
import 'dart:ui' as ui;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show Config, RootSeed;
import 'package:flutter/cupertino.dart' show CupertinoScrollBehavior;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart'
    show MarkdownBody, MarkdownStyleSheet;
import 'package:lexeapp/app_data.dart' show LxAppData;
import 'package:lexeapp/components.dart'
    show CarouselIndicatorsAndButtons, LxFilledButton, LxOutlinedButton;
import 'package:lexeapp/double_ext.dart';
import 'package:lexeapp/feature_flags.dart' show FeatureFlags;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/route/restore.dart' show RestoreApi, RestorePage;
import 'package:lexeapp/route/signup.dart'
    show SignupApi, SignupCtx, SignupPage;
import 'package:lexeapp/route/wallet.dart' show WalletPage;
import 'package:lexeapp/settings.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, LxTheme, Space;
import 'package:lexeapp/uri_events.dart' show UriEvents;
import 'package:lexeapp/url.dart' as url;

const double landingButtonsWidth = 300.0;
const double landingPageDefaultMaxWidth = 300.0;
const double landingPageDefaultHorizontalPadding = Space.s400;

class LandingPage extends StatefulWidget {
  const LandingPage({
    super.key,
    required this.config,
    required this.rootSeed,
    required this.gdriveAuth,
    required this.signupApi,
    required this.restoreApi,
    required this.uriEvents,
    required this.fixedShaderTime,
  });

  final Config config;
  final RootSeed rootSeed;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;
  final RestoreApi restoreApi;
  final UriEvents uriEvents;

  /// If non-null, the background shader will not vary with time and instead
  /// stay at a fixed time offset. Used for tests and screenshots.
  final double? fixedShaderTime;

  @override
  State<LandingPage> createState() => _LandingPageState();
}

class _LandingPageState extends State<LandingPage>
    with SingleTickerProviderStateMixin {
  final PageController carouselScrollController = PageController();
  final ValueNotifier<int> selectedPageIndex = ValueNotifier(0);

  // LandingPage carousel auto-advance
  //
  // From user feedback, it's not 100% clear that the landing carousel is
  // scrollable. So to help fix that, we'll show an auto-advance progress
  // indicator and auto-advance the carousel every few seconds. If the user
  // interacts with the carousel, we'll stop auto-advancing.

  // Whether the auto-advance progress indicator is enabled.
  final ValueNotifier<bool> showAutoAdvanceProgress = ValueNotifier(true);
  // The animation controller for the auto-advance progress circle animation.
  late final AnimationController autoAdvanceProgressController;
  // Whether an auto-advance initiated scroll is currently in-flight.
  bool autoAdvanceInFlight = false;
  // Idx of the last page in the landing carousel. Since we make the pages list
  // `build()`, this needs to be computed there.
  int landingLastPageIndex = 0;

  @override
  void dispose() {
    this.autoAdvanceProgressController.dispose();
    this.showAutoAdvanceProgress.dispose();
    this.selectedPageIndex.dispose();
    this.carouselScrollController.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();

    this.autoAdvanceProgressController = AnimationController(
      vsync: this,
      duration: Duration(seconds: 5),
      value: 1.0,
    );

    this.autoAdvanceProgressController.addStatusListener(
      this.onAutoAdvanceStatusChanged,
    );

    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!this.mounted) return;
      this.startAutoAdvanceCountdown();
    });
  }

  /// Start the Signup UI flow. Future resolves when the user has either
  /// (1) completed the flow and signed up or (2) canceled the flow.
  Future<void> doSignupFlow() async {
    info("landing: begin signup flow");

    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => SignupPage(
          ctx: SignupCtx(
            this.widget.config,
            this.widget.rootSeed,
            this.widget.gdriveAuth,
            this.widget.signupApi,
          ),
        ),
      ),
    );

    if (flowResult == null) return;
    if (!this.mounted) return;

    info("landing: successfully signed up");

    final app = flowResult;
    final settings = LxSettings(app.settingsDb());
    final appData = LxAppData(app.appDataDb());
    final featureFlags = FeatureFlags(
      deployEnv: this.widget.config.deployEnv,
      userPk: app.walletUser().userPk,
    );

    unawaited(
      Navigator.of(this.context).pushReplacement(
        MaterialPageRoute(
          builder: (_) => WalletPage(
            config: this.widget.config,
            app: app,
            settings: settings,
            appData: appData,
            featureFlags: featureFlags,
            uriEvents: this.widget.uriEvents,
            gdriveAuth: this.widget.gdriveAuth,
          ),
        ),
      ),
    );
  }

  /// Start the Wallet Restore UI flow. Future resolves when the user has either
  /// (1) completed the flow and restored or (2) canceled the flow.
  Future<void> doRestoreFlow() async {
    info("landing: begin restore flow");

    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => RestorePage(
          config: this.widget.config,
          gdriveAuth: this.widget.gdriveAuth,
          restoreApi: this.widget.restoreApi,
        ),
      ),
    );

    if (flowResult == null) return;
    if (!this.mounted) return;

    info("landing: successfully restored");

    final app = flowResult;
    final settings = LxSettings(app.settingsDb());
    final appData = LxAppData(app.appDataDb());
    final featureFlags = FeatureFlags(
      deployEnv: this.widget.config.deployEnv,
      userPk: app.walletUser().userPk,
    );

    unawaited(
      Navigator.of(this.context).pushReplacement(
        MaterialPageRoute(
          builder: (_) => WalletPage(
            config: this.widget.config,
            app: app,
            settings: settings,
            appData: appData,
            featureFlags: featureFlags,
            uriEvents: this.widget.uriEvents,
            gdriveAuth: this.widget.gdriveAuth,
          ),
        ),
      ),
    );
  }

  void onTapPrev() {
    this.stopAutoAdvance(); // stop auto-advancing on user interaction
    this.prevPage();
  }

  void onTapNext() {
    this.stopAutoAdvance(); // stop auto-advancing on user interaction
    this.nextPage();
  }

  void prevPage() {
    unawaited(
      this.carouselScrollController.previousPage(
        duration: const Duration(milliseconds: 500),
        curve: Curves.ease,
      ),
    );
  }

  void nextPage() {
    unawaited(
      this.carouselScrollController.nextPage(
        duration: const Duration(milliseconds: 500),
        curve: Curves.ease,
      ),
    );
  }

  /// Stop auto-advancing the carousel.
  void stopAutoAdvance() {
    if (!this.mounted) return;
    this.showAutoAdvanceProgress.value = false;
    this.autoAdvanceProgressController.stop();
    this.autoAdvanceInFlight = false;
  }

  /// Start the next auto-advance countdown animation cycle.
  void startAutoAdvanceCountdown() {
    if (!this.mounted || !this.showAutoAdvanceProgress.value) return;

    // Stop auto-advancing once we hit the last page.
    if (this.selectedPageIndex.value >= this.landingLastPageIndex) {
      this.stopAutoAdvance();
      return;
    }

    this.autoAdvanceProgressController
      ..stop()
      ..value = 1.0
      ..reverse(from: 1.0);
  }

  /// Called when the auto-advance progress animation completes a cycle or
  /// is dismissed.
  void onAutoAdvanceStatusChanged(AnimationStatus status) {
    if (status != AnimationStatus.dismissed) return;
    if (!this.mounted || !this.showAutoAdvanceProgress.value) return;
    if (!this.carouselScrollController.hasClients) return;
    if (this.selectedPageIndex.value >= this.landingLastPageIndex) return;

    // Start scrolling to the next page.
    this.autoAdvanceInFlight = true;
    unawaited(
      this.carouselScrollController.nextPage(
        duration: const Duration(milliseconds: 500),
        curve: Curves.ease,
      ),
    );
  }

  /// Get notified when the user manually scrolls the carousel, so we can
  /// stop auto-advancing.
  bool onCarouselScrollNotification(ScrollNotification notification) {
    final userStartedDrag =
        notification is ScrollStartNotification &&
        notification.dragDetails != null;
    final userIsDragging =
        notification is ScrollUpdateNotification &&
        notification.dragDetails != null;
    if (userStartedDrag || userIsDragging) {
      this.stopAutoAdvance();
    }
    return false;
  }

  void onCarouselPageChanged(int pageIndex) {
    if (!this.mounted) return;
    this.selectedPageIndex.value = pageIndex;

    // Auto-advance

    if (!this.showAutoAdvanceProgress.value) return;

    // Must have been a user scroll. Stop auto-advance.
    if (!this.autoAdvanceInFlight) {
      this.stopAutoAdvance();
      return;
    }

    // Stop auto-advancing once we hit the last page.
    if (this.selectedPageIndex.value >= this.landingLastPageIndex) {
      this.stopAutoAdvance();
      return;
    }

    // Done scrolling to next page and still have more pages to go.
    if (this.autoAdvanceInFlight) {
      this.autoAdvanceInFlight = false;
      this.startAutoAdvanceCountdown();
    }
  }

  @override
  Widget build(BuildContext context) {
    // Each page in the carousel.
    final List<_LandingPageSpec> landingPages = [
      // Keyword pills showcase ticker
      _LandingPageSpec(
        // Let the keyword pills ticker go full-bleed to the edges of the screen
        horizontalPadding: 0.0,
        maxContentWidth: null,
        child: const _LandingKeywordPage(
          labels: [
            "Instant payments",
            "BOLT12 offers",
            "\u20bfme@lexe.app",
            "Self-custodial",
            "Lightning Address",
            "Free hosting",
            "Managed liquidity",
            "Private",
            "Open-source",
            "24/7 uptime",
          ],
        ),
      ),

      //
      _LandingPageSpec(
        child: LandingMarkdownBody('''
## RECEIVE PAYMENTS 24/7.

Your Lightning node is **always available** to receive payments.

Get paid **anytime, anywhere**. Even when your phone goes offline.

[Learn more](https://docs.lexe.app/how-lexe-works/#lexes-solution-your-node-in-the-cloud)
      '''),
      ),

      // TODO(phlip9): tap to clarify what a "Secure Enclave" is?
      _LandingPageSpec(
        child: LandingMarkdownBody('''
## YOUR BITCOIN'S SAFE, EVEN FROM US.

We run your node in a **Secure Enclave** so your funds are protected, even if we get hacked.

With LEXE, **only you control your funds**. Let us handle the infrastructure.

[Learn more](https://docs.lexe.app/how-lexe-works/#what-is-a-secure-enclave)
      '''),
      ),

      // TODO(phlip9): add this page after we actually implement paid liquidity.
      //       //
      //       LandingMarkdownBody('''
      // ## AUTOMATIC INBOUND LIQUIDITY.
      //
      // Your node can automatically top-up liquidity so you **never miss a payment again**.
      //       '''),

      //
      _LandingPageSpec(
        child: LandingMarkdownBody('''
## DON'T TRUST, VERIFY.

The LEXE Lightning node is [open-source](https://github.com/lexe-app/lexe-public) and fully reproducible.

Your wallet always verifies your node's software before sharing any keys.

[Learn more](https://docs.lexe.app/how-lexe-works/#how-you-stay-in-control)
      '''),
      ),

      //
      _LandingPageSpec(
        child: LandingMarkdownBody('''
## SIMPLE, TRANSPARENT PRICING.

**Node hosting is free**, forever. No subscriptions, no hidden fees.

**Up to 0.5% fee** to send and receive Lightning payments*

[Learn more](https://docs.lexe.app/fees-and-pricing/)*
      '''),
      ),
    ];

    final numPages = landingPages.length;
    this.landingLastPageIndex = (numPages > 0) ? numPages - 1 : 0;

    // set the SystemUiOverlay bars to transparent so the background shader
    // shows through.
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: LxTheme.systemOverlayStyleLightClearBg,
      child: Scaffold(
        backgroundColor: LxColors.background,
        body: Stack(
          children: [
            // Background shader.
            InkuShader(
              carouselScrollController: this.carouselScrollController,
              fixedShaderTime: this.widget.fixedShaderTime,
              child: const Center(),
            ),

            // Main body content, with max width and height, centered in the
            // viewport.
            LayoutBuilder(
              builder: (BuildContext context, BoxConstraints viewport) {
                final viewportHeight = viewport.maxHeight;

                const minHeight = 525.0;
                const verticalBreakpoint = 725.0;

                final maxHeight = max(minHeight, viewportHeight);
                final top = (viewportHeight > verticalBreakpoint)
                    ? 196.0
                    : 64.0;
                final bottom = (viewportHeight > verticalBreakpoint)
                    ? 64.0
                    : 32.0;

                return Center(
                  child: Container(
                    constraints: BoxConstraints(
                      minHeight: minHeight,
                      maxHeight: maxHeight,
                    ),
                    child: Stack(
                      fit: StackFit.passthrough,
                      children: [
                        // Landing marketing pages.
                        Container(
                          padding: EdgeInsets.only(top: top),
                          child: NotificationListener<ScrollNotification>(
                            onNotification: this.onCarouselScrollNotification,
                            child: PageView.builder(
                              controller: this.carouselScrollController,
                              scrollBehavior: const CupertinoScrollBehavior(),
                              onPageChanged: this.onCarouselPageChanged,
                              itemBuilder: (context, idx) {
                                if (idx < 0 || idx >= numPages) return null;

                                final page = landingPages[idx];
                                final pageChild = (page.maxContentWidth == null)
                                    ? page.child
                                    : ConstrainedBox(
                                        constraints: BoxConstraints(
                                          maxWidth: page.maxContentWidth!,
                                        ),
                                        child: page.child,
                                      );

                                return Container(
                                  alignment: Alignment.topCenter,
                                  padding: EdgeInsets.symmetric(
                                    horizontal: page.horizontalPadding,
                                  ),
                                  child: pageChild,
                                );
                              },
                            ),
                          ),
                        ),

                        // Action buttons (signup, restore) and page indicators.
                        Container(
                          padding: EdgeInsets.fromLTRB(
                            landingPageDefaultHorizontalPadding,
                            0,
                            landingPageDefaultHorizontalPadding,
                            bottom,
                          ),
                          alignment: Alignment.bottomCenter,
                          child: ConstrainedBox(
                            constraints: const BoxConstraints(
                              maxWidth: landingButtonsWidth,
                            ),
                            child: LandingButtons(
                              config: this.widget.config,
                              numPages: numPages,
                              selectedPageIndex: this.selectedPageIndex,
                              onSignupPressed: () =>
                                  unawaited(this.doSignupFlow()),
                              onRestorePressed: () =>
                                  unawaited(this.doRestoreFlow()),
                              onTapPrev: this.onTapPrev,
                              onTapNext: this.onTapNext,
                              autoAdvanceProgressAnimation:
                                  this.autoAdvanceProgressController,
                              showAutoAdvanceProgress:
                                  this.showAutoAdvanceProgress,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                );
              },
            ),
          ],
        ),
      ),
    );
  }
}

final MarkdownStyleSheet _landingStyleSheet = MarkdownStyleSheet(
  h1: Fonts.fontHero,
  h1Padding: const EdgeInsets.only(bottom: Fonts.size800 * 0.5),
  h2: Fonts.fontHero.copyWith(fontSize: Fonts.size700, height: 1.3),
  h2Padding: const EdgeInsets.only(bottom: Fonts.size700 * 0.25),
  p: Fonts.fontBody.copyWith(
    fontSize: Fonts.size300,
    color: LxColors.foreground,
    letterSpacing: -0.5,
  ),
  pPadding: const EdgeInsets.symmetric(vertical: Fonts.size300 * 0.25),
  strong: const TextStyle(fontVariations: [Fonts.weightBold]),
  a: const TextStyle(
    color: LxColors.foreground,
    decoration: TextDecoration.underline,
  ),
);

/// Called when a user hits a `[text](href)`.
/// Currently just opens any https:// links in the browser.
Future<void> _onTapLink(String _text, String? href, String _title) async {
  if (href == null || !href.startsWith("https://")) {
    return;
  }
  await url.open(href);
}

/// [MarkdownBody] but styled for the landing page.
class LandingMarkdownBody extends MarkdownBody {
  LandingMarkdownBody(final String data, {super.key})
    : super(data: data, styleSheet: _landingStyleSheet, onTapLink: _onTapLink);
}

/// A single page in the carousel.
///
/// The [maxContentWidth] and [horizontalPadding] are configurable to support
/// the [_LandingKeywordPage], which needs to span the full horizontal width
/// of the viewport.
class _LandingPageSpec {
  const _LandingPageSpec({
    required this.child,
    this.maxContentWidth = landingPageDefaultMaxWidth,
    this.horizontalPadding = landingPageDefaultHorizontalPadding,
  });

  final Widget child;
  final double? maxContentWidth;
  final double horizontalPadding;
}

/// NEXT-GEN LIGHTNING + keyword pills showcase ticker. Let users quickly
/// pattern match on what features we offer.
class _LandingKeywordPage extends StatelessWidget {
  const _LandingKeywordPage({required this.labels});

  final List<String> labels;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(
            horizontal: landingPageDefaultHorizontalPadding,
          ),
          child: Align(
            alignment: Alignment.topCenter,
            child: SizedBox(
              width: landingPageDefaultMaxWidth,
              child: LandingMarkdownBody("## NEXT-GEN BITCOIN WALLET."),
            ),
          ),
        ),

        const SizedBox(height: Space.s300),
        _LandingKeywordPills(labels: this.labels),
      ],
    );
  }
}

/// Greedily measure and fill pills into rows, then animate each row like a
/// ticker.
class _LandingKeywordPills extends StatelessWidget {
  const _LandingKeywordPills({required this.labels});

  final List<String> labels;

  static const _spaceBetween = Space.s200;
  static const _baseSpeedPxPerSecond = 14.0;
  static const _speedDeltaPerRow = 2.0;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final maxWidth = constraints.maxWidth;
        if (!maxWidth.isFinite || maxWidth <= 0.0) {
          return const SizedBox();
        }

        final textScaler = MediaQuery.textScalerOf(context);
        final rowHeight = this.measurePillRowHeight(textScaler);
        final rows = this._assignToRows(maxWidth, textScaler);

        return Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final (i, row) in rows.indexed) ...[
              if (i > 0) const SizedBox(height: _spaceBetween),
              _LandingKeywordTickerRow(
                pills: row,
                viewportWidth: maxWidth,
                spaceBetween: _spaceBetween,
                speedPxPerSecond: _baseSpeedPxPerSecond + i * _speedDeltaPerRow,
                phaseOffsetFraction: (i * 0.27).fract(),
                scrollLeft: i.isEven,
                rowHeight: rowHeight,
              ),
            ],
          ],
        );
      },
    );
  }

  /// Measure each pill's natural width with [TextPainter], then greedily
  /// pack pills into rows that fit within [maxWidth].
  List<List<(String, double)>> _assignToRows(
    double maxWidth,
    TextScaler textScaler,
  ) {
    // Measure natural pill widths. Must use the same font + textScaler as the
    // actual Text widget, otherwise the measurement will be off.
    final tp = TextPainter(
      textDirection: ui.TextDirection.ltr,
      textScaler: textScaler,
      maxLines: 1,
    );
    final List<(String, double)> pills;
    try {
      pills = this.labels.map((label) {
        final layout = tp
          ..text = TextSpan(text: label, style: _LandingKeywordPill._textStyle)
          ..layout();
        final naturalWidth =
            layout.width +
            _LandingKeywordPill._hPadding * 2 +
            _LandingKeywordPill._borderWidth * 2;
        return (label, naturalWidth);
      }).toList();
    } finally {
      // Ensure we always dispose the TextPainter
      tp.dispose();
    }

    // Greedy row assignment.
    final rows = <List<(String, double)>>[];
    var row = <(String, double)>[];
    var rowWidth = 0.0;

    for (final pill in pills) {
      final needed = row.isEmpty ? pill.$2 : pill.$2 + _spaceBetween;
      if (row.isNotEmpty && rowWidth + needed > maxWidth) {
        rows.add(row);
        row = [pill];
        rowWidth = pill.$2;
      } else {
        row.add(pill);
        rowWidth += needed;
      }
    }
    if (row.isNotEmpty) rows.add(row);

    return rows;
  }

  double measurePillRowHeight(TextScaler textScaler) {
    final tp = TextPainter(
      textDirection: ui.TextDirection.ltr,
      textScaler: textScaler,
      maxLines: 1,
      text: TextSpan(text: "Hg", style: _LandingKeywordPill._textStyle),
    );
    try {
      tp.layout();
      return tp.height +
          _LandingKeywordPill._vPadding * 2 +
          _LandingKeywordPill._borderWidth * 2;
    } finally {
      tp.dispose();
    }
  }
}

/// A single horizontally scrolling ticker row of keyword pills.
class _LandingKeywordTickerRow extends StatefulWidget {
  const _LandingKeywordTickerRow({
    required this.pills,
    required this.viewportWidth,
    required this.spaceBetween,
    required this.speedPxPerSecond,
    required this.phaseOffsetFraction,
    required this.rowHeight,
    required this.scrollLeft,
  });

  /// The pill labels and their natural widths, as measured by [TextPainter].
  final List<(String, double)> pills;

  final double viewportWidth;
  final double spaceBetween;
  final double speedPxPerSecond;
  final double phaseOffsetFraction;
  final double rowHeight;
  final bool scrollLeft;

  @override
  State<_LandingKeywordTickerRow> createState() =>
      _LandingKeywordTickerRowState();
}

class _LandingKeywordTickerRowState extends State<_LandingKeywordTickerRow>
    with SingleTickerProviderStateMixin {
  late final AnimationController tickerController;

  static const _edgeFadeWidth = 36.0;

  @override
  void initState() {
    super.initState();
    this.tickerController = AnimationController(vsync: this);
    this.syncTickerAnimation();
  }

  @override
  void didUpdateWidget(covariant _LandingKeywordTickerRow oldWidget) {
    super.didUpdateWidget(oldWidget);
    this.syncTickerAnimation();
  }

  @override
  void dispose() {
    this.tickerController.dispose();
    super.dispose();
  }

  void syncTickerAnimation() {
    final trackWidth = this.trackWidth();
    if (trackWidth <= 0.0 || this.widget.speedPxPerSecond <= 0.0) {
      this.tickerController.stop();
      this.tickerController.value = 0.0;
      return;
    }

    final msPerCycle = max(
      1,
      (1000.0 * trackWidth / this.widget.speedPxPerSecond).round(),
    );
    this.tickerController.repeat(
      period: Duration(milliseconds: msPerCycle),
      min: 0.0,
      max: 1.0,
    );
  }

  double trackWidth() {
    if (this.widget.pills.isEmpty) return 0.0;

    final totalPillWidth = this.widget.pills.fold(
      0.0,
      (sum, pill) => sum + pill.$2,
    );

    // Keep one trailing spacer so wrapping from end -> start has the same gap.
    final totalSpacing = this.widget.spaceBetween * this.widget.pills.length;

    return totalPillWidth + totalSpacing;
  }

  Widget edgeFadeMask({required Widget child}) {
    return ShaderMask(
      blendMode: ui.BlendMode.dstIn,
      shaderCallback: (bounds) {
        final edgeFraction = (bounds.width <= 0.0)
            ? 0.0
            : (_edgeFadeWidth / bounds.width).clamp(0.0, 0.5).toDouble();
        return LinearGradient(
          begin: Alignment.centerLeft,
          end: Alignment.centerRight,
          colors: const [
            Color(0x00FFFFFF),
            Color(0xFFFFFFFF),
            Color(0xFFFFFFFF),
            Color(0x00FFFFFF),
          ],
          stops: [0.0, edgeFraction, 1.0 - edgeFraction, 1.0],
        ).createShader(bounds);
      },
      child: child,
    );
  }

  Widget buildTrack() {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final (i, (label, naturalWidth)) in this.widget.pills.indexed) ...[
          if (i > 0) SizedBox(width: this.widget.spaceBetween),
          SizedBox(width: naturalWidth, child: _LandingKeywordPill(label)),
        ],
        SizedBox(width: this.widget.spaceBetween),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final trackWidth = this.trackWidth();
    if (this.widget.pills.isEmpty || trackWidth <= 0.0) {
      return const SizedBox();
    }

    final numTrackCopies = (this.widget.viewportWidth / trackWidth).ceil() + 2;
    final repeatedTrack = Row(
      mainAxisSize: MainAxisSize.min,
      children: [for (var i = 0; i < numTrackCopies; i++) this.buildTrack()],
    );
    final repeatedTrackWidth = numTrackCopies * trackWidth;
    final cycleDx = trackWidth / repeatedTrackWidth;
    final phaseDx = this.widget.phaseOffsetFraction * cycleDx;
    // For rightward motion, keep the whole sweep in negative X so we never
    // expose empty space at the left edge of the clipped viewport.
    final beginDx = this.widget.scrollLeft ? -phaseDx : -2 * cycleDx + phaseDx;
    final endDx = this.widget.scrollLeft
        ? beginDx - cycleDx
        : beginDx + cycleDx;
    final slideAnimation = Tween<Offset>(
      begin: Offset(beginDx, 0.0),
      end: Offset(endDx, 0.0),
    ).animate(this.tickerController);

    return ClipRect(
      // Fade in the left and right edges to make the edges less jarring when
      // scrolling to the next/prev page.
      child: this.edgeFadeMask(
        child: SizedBox(
          width: this.widget.viewportWidth,
          height: this.widget.rowHeight,
          child: OverflowBox(
            alignment: Alignment.centerLeft,
            minWidth: 0.0,
            maxWidth: double.infinity,
            minHeight: this.widget.rowHeight,
            maxHeight: this.widget.rowHeight,
            child: SlideTransition(
              position: slideAnimation,
              child: repeatedTrack,
            ),
          ),
        ),
      ),
    );
  }
}

/// A single keyword pill on the "NEXT-GEN LIGHTNING" page
class _LandingKeywordPill extends StatelessWidget {
  const _LandingKeywordPill(this.label);

  final String label;

  static const _borderWidth = 1.0;
  static const _hPadding = Space.s400;
  static const _vPadding = Space.s200;

  /// NOTE: use a complete TextStyle here, without inherited properties, to
  /// ensure that the TextPainter measurements in _assignToRows are accurate
  static final _textStyle = Fonts.fontUI.copyWith(
    fontSize: Fonts.size200,
    fontVariations: [Fonts.weightSemiBold],
    overflow: TextOverflow.ellipsis,
  );

  @override
  Widget build(BuildContext context) => DecoratedBox(
    decoration: const BoxDecoration(
      borderRadius: BorderRadius.all(
        Radius.elliptical(LxRadius.r400, LxRadius.r400),
      ),
      // 45-deg internal lighting
      gradient: LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: [LxColors.clearW800, LxColors.clearW600],
      ),
      // thin glass-edge border highlight
      border: Border.fromBorderSide(
        BorderSide(color: LxColors.clearW200, width: _borderWidth),
      ),
      // soft shadow to add some slight contrast against the background
      boxShadow: [
        BoxShadow(
          color: LxColors.clearB50,
          blurRadius: 8.0,
          offset: Offset(0, 2.0),
        ),
      ],
    ),
    child: Padding(
      padding: const EdgeInsets.symmetric(
        vertical: _vPadding,
        // For some reason the pill text ellipsizes without this small tweak...
        horizontal: _hPadding - 2.0,
      ),
      child: Text(
        this.label,
        textAlign: TextAlign.center,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: _textStyle,
      ),
    ),
  );
}

class LandingButtons extends StatelessWidget {
  const LandingButtons({
    super.key,
    required this.config,
    required this.onSignupPressed,
    required this.onRestorePressed,
    required this.selectedPageIndex,
    required this.numPages,
    required this.onTapPrev,
    required this.onTapNext,
    required this.showAutoAdvanceProgress,
    this.autoAdvanceProgressAnimation,
  });

  final Config config;

  final int numPages;
  final ValueListenable<int> selectedPageIndex;

  final VoidCallback onSignupPressed;
  final VoidCallback onRestorePressed;
  final VoidCallback onTapPrev;
  final VoidCallback onTapNext;
  final ValueListenable<bool> showAutoAdvanceProgress;
  final Animation<double>? autoAdvanceProgressAnimation;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        // Indicator dots to show which page we're on.
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 7.0),
          child: CarouselIndicatorsAndButtons(
            numPages: this.numPages,
            selectedPageIndex: this.selectedPageIndex,
            onTapPrev: this.onTapPrev,
            onTapNext: this.onTapNext,
            showAutoAdvanceProgress: this.showAutoAdvanceProgress,
            autoAdvanceProgressAnimation: this.autoAdvanceProgressAnimation,
          ),
        ),
        const SizedBox(height: Space.s300),

        // Signup ->
        LxFilledButton(
          onTap: this.onSignupPressed,
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.foreground,
            foregroundColor: LxColors.background,
            iconColor: LxColors.background,
            fixedSize: const Size(landingButtonsWidth, Space.s800),
          ),
          label: const Text("Create wallet"),
          icon: const Icon(LxIcons.nextSecondary),
        ),
        const SizedBox(height: Space.s400),

        // Recover Wallet
        LxOutlinedButton(
          onTap: this.onRestorePressed,
          style: ButtonStyle(
            fixedSize: WidgetStateProperty.all(
              const Size(landingButtonsWidth, Space.s800),
            ),
          ),
          label: const Text("Restore wallet"),
        ),
      ],
    );
  }
}

class InkuShader extends StatelessWidget {
  const InkuShader({
    super.key,
    required this.carouselScrollController,
    required this.fixedShaderTime,
    this.child,
  });

  final PageController carouselScrollController;
  final double? fixedShaderTime;
  final Widget? child;

  static Future<ui.FragmentShader> load() async {
    final program = await ui.FragmentProgram.fromAsset("shaders/inku.frag");
    return program.fragmentShader();
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder(
      future: InkuShader.load(),
      builder: (context, snapshot) {
        if (snapshot.hasError) {
          error(
            "Error loading shader: ${snapshot.error}:\n${snapshot.stackTrace}",
          );
          return const SizedBox();
        }
        if (!snapshot.hasData) {
          return const SizedBox();
        }

        return AnimatedShader(
          shader: snapshot.data!,
          carouselScrollController: this.carouselScrollController,
          fixedShaderTime: this.fixedShaderTime,
          child: this.child,
        );
      },
    );
  }
}

class AnimatedShader extends StatefulWidget {
  const AnimatedShader({
    super.key,
    required this.shader,
    required this.carouselScrollController,
    required this.fixedShaderTime,
    this.child,
  });

  final ui.FragmentShader shader;
  final PageController carouselScrollController;
  final double? fixedShaderTime;
  final Widget? child;

  @override
  AnimatedShaderState createState() => AnimatedShaderState();
}

class AnimatedShaderState extends State<AnimatedShader>
    with SingleTickerProviderStateMixin {
  late final AnimationController animationController;

  @override
  void initState() {
    super.initState();
    this.animationController = AnimationController(
      vsync: this,
      upperBound: 10000.0,
      duration: const Duration(seconds: 10000), // why no infinite animation??
    );
    unawaited(this.animationController.forward(from: 0.0));
  }

  @override
  void dispose() {
    this.animationController.dispose();
    super.dispose();
  }

  /// phlip9: saw an error once where the scroll controller wasn't yet attached
  /// to the carousel by the time the shader rendered, so this method is just
  /// a super defensive way to get the scroll offset, but defaults to 0.0 if
  /// something is weird.
  double scrollOffset() {
    final controller = this.widget.carouselScrollController;
    final positions = controller.positions;

    if (positions.isEmpty) return 0.0;

    final position = positions.first;
    if (!position.hasPixels) return 0.0;

    return position.pixels;
  }

  @override
  Widget build(BuildContext context) {
    double prevOffset = this.scrollOffset();
    double? fixedShaderTime = this.widget.fixedShaderTime;

    return AnimatedBuilder(
      animation: this.animationController,
      builder: (BuildContext _, Widget? child) {
        // Add small EMA dampening to scroll offset.
        const a = 0.25;
        final nextOffset = a * this.scrollOffset() + (1.0 - a) * prevOffset;
        prevOffset = nextOffset;

        // Current time offset, passed to the shader. Can be configured as a
        // fixed value for tests/screenshots.
        final time = fixedShaderTime ?? this.animationController.value;

        return CustomPaint(
          painter: ShaderPainter(this.widget.shader, time, nextOffset),
          // raster cache probably shouldn't cache this since it changes every frame
          isComplex: false,
          willChange: true,
          child: child,
        );
      },
      child: this.widget.child,
    );
  }
}

class ShaderPainter extends CustomPainter {
  const ShaderPainter(this.shader, this.time, this.scrollOffset);

  final ui.FragmentShader shader;
  final double time;

  // The offset of the carousel in pixels
  // (first page = 0, second page = +screen width, ...)
  final double scrollOffset;

  @override
  void paint(ui.Canvas canvas, ui.Size size) {
    // set shader uniforms
    // 0 : u_resolution.x
    this.shader.setFloat(0, size.width);
    // 1 : u_resolution.y
    this.shader.setFloat(1, size.height);
    // 2 : u_time
    this.shader.setFloat(2, this.time);
    // 3 : u_scroll_offset
    final double normalizedScrollOffset = this.scrollOffset / size.height;
    this.shader.setFloat(3, normalizedScrollOffset);

    final screenRect = Rect.fromLTWH(0.0, 0.0, size.width, size.height);
    final paint = Paint()..shader = this.shader;

    canvas.drawRect(screenRect, paint);
  }

  @override
  bool shouldRepaint(covariant ShaderPainter oldDelegate) =>
      this.time != oldDelegate.time || this.shader != oldDelegate.shader;
}
