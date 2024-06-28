import 'dart:async' show unawaited;
import 'dart:math' show max;
import 'dart:ui' as ui;

import 'package:flutter/cupertino.dart' show CupertinoScrollBehavior;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;
import 'package:lexeapp/components.dart'
    show CarouselIndicatorsAndButtons, LxFilledButton, LxOutlinedButton;
import 'package:lexeapp/ffi/ffi_generated_api.dart' show AppHandle, Config;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/route/signup.dart' show SignupApi, SignupPage;
import 'package:lexeapp/route/wallet.dart' show WalletPage;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;
import 'package:lexeapp/uri_events.dart' show UriEvents;

class LandingPage extends StatefulWidget {
  const LandingPage({
    super.key,
    required this.config,
    required this.gdriveAuth,
    required this.signupApi,
    required this.uriEvents,
  });

  final Config config;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;
  final UriEvents uriEvents;

  @override
  State<LandingPage> createState() => _LandingPageState();
}

const List<Widget> landingPages = [
  LandingCalloutText(heroText: "LIGHTNING.\nBITCOIN.\nONE WALLET."),
  LandingCalloutText(heroText: "RECEIVE\nPAYMENTS\n24/7."),
  LandingCalloutText(heroText: "ZERO\nCOMPROMISE\nSELF CUSTODY."),
  LandingCalloutText(heroText: "MAX SECURITY.\nPOWERED BY SGX™."),
];

class _LandingPageState extends State<LandingPage> {
  final PageController carouselScrollController = PageController();
  final ValueNotifier<int> selectedPageIndex = ValueNotifier(0);

  @override
  void dispose() {
    carouselScrollController.dispose();
    selectedPageIndex.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
  }

  /// Start the Signup UI flow. Future resolves when the user has either
  /// (1) completed the flow and signed up or (2) canceled the flow.
  Future<void> doSignupFlow() async {
    info("do signup flow");

    final AppHandle? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (_) => SignupPage(
        config: this.widget.config,
        gdriveAuth: this.widget.gdriveAuth,
        signupApi: this.widget.signupApi,
      ),
    ));

    if (flowResult == null) return;
    if (!this.mounted) return;

    info("successfully signed up!");

    // ignore: use_build_context_synchronously
    unawaited(Navigator.of(this.context).pushReplacement(MaterialPageRoute(
      builder: (_) => WalletPage(
        config: this.widget.config,
        app: flowResult,
        uriEvents: this.widget.uriEvents,
      ),
    )));
  }

  /// Start the Wallet Restore UI flow. Future resolves when the user has either
  /// (1) completed the flow and restored or (2) canceled the flow.
  Future<void> doRestoreFlow() async {
    info("do restore flow");
  }

  void prevPage() {
    unawaited(this.carouselScrollController.previousPage(
          duration: const Duration(milliseconds: 500),
          curve: Curves.ease,
        ));
  }

  void nextPage() {
    unawaited(this.carouselScrollController.nextPage(
          duration: const Duration(milliseconds: 500),
          curve: Curves.ease,
        ));
  }

  @override
  Widget build(BuildContext context) {
    final numPages = landingPages.length;

    // set the SystemUiOverlay bars to transparent so the background shader
    // shows through.
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: LxTheme.systemOverlayStyleLightClearBg,
      child: Scaffold(
        backgroundColor: LxColors.background,
        body: Stack(children: [
          // Background shader.
          InkuShader(
            carouselScrollController: this.carouselScrollController,
            child: const Center(),
          ),

          // Main body content, with max width and height, centered in the
          // viewport.
          LayoutBuilder(
            builder: (BuildContext context, BoxConstraints viewport) {
              final viewportHeight = viewport.maxHeight;

              const maxWidth = 300.0;
              const minHeight = 525.0;
              const verticalBreakpoint = 700.0;

              final maxHeight = max(minHeight, viewportHeight);
              final top = (viewportHeight > verticalBreakpoint) ? 196.0 : 64.0;
              final bottom =
                  (viewportHeight > verticalBreakpoint) ? 64.0 : 32.0;

              return Center(
                child: Container(
                  constraints: BoxConstraints(
                    minHeight: minHeight,
                    maxHeight: maxHeight,
                  ),
                  child: Stack(fit: StackFit.passthrough, children: [
                    // Landing marketing pages.
                    Container(
                      padding: EdgeInsets.only(top: top),
                      child: PageView.builder(
                        controller: this.carouselScrollController,
                        scrollBehavior: const CupertinoScrollBehavior(),
                        onPageChanged: (pageIndex) {
                          if (!this.mounted) return;
                          this.selectedPageIndex.value = pageIndex;
                        },
                        itemBuilder: (context, idx) {
                          if (idx < 0 || idx >= numPages) return null;

                          return Container(
                            alignment: Alignment.topCenter,
                            child: ConstrainedBox(
                              constraints:
                                  const BoxConstraints(maxWidth: maxWidth),
                              child: landingPages[idx],
                            ),
                          );
                        },
                      ),
                    ),

                    // Action buttons (signup, restore) and page indicators.
                    Container(
                      padding: EdgeInsets.only(bottom: bottom),
                      alignment: Alignment.bottomCenter,
                      child: ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: maxWidth),
                        child: LandingButtons(
                          config: this.widget.config,
                          numPages: numPages,
                          selectedPageIndex: this.selectedPageIndex,
                          onSignupPressed: () => unawaited(this.doSignupFlow()),
                          onRecoverPressed: () =>
                              unawaited(this.doRestoreFlow()),
                          onTapPrev: this.prevPage,
                          onTapNext: this.nextPage,
                        ),
                      ),
                    ),
                  ]),
                ),
              );
            },
          ),
        ]),
      ),
    );
  }
}

class LandingCalloutText extends StatelessWidget {
  const LandingCalloutText({super.key, required this.heroText});

  final String heroText;

  @override
  Widget build(BuildContext context) {
    final heroText = Text(
      this.heroText,
      overflow: TextOverflow.clip,
      style: Fonts.fontHero.copyWith(
        color: LxColors.foreground,
      ),
    );

    final lexeText = Row(
      // mainAxisSize: MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.end,
      children: [
        // SizedBox(width: 128.0),
        Text(
          "brought to you by",
          style: Fonts.fontUI.copyWith(
            color: LxColors.clearB700,
            fontSize: Fonts.size100,
            fontVariations: [Fonts.weightExtraLight],
          ),
        ),
        const SizedBox(width: 4.0),
        Text(
          "LEXE™",
          style: Fonts.fontHubot.copyWith(
            color: LxColors.clearB700,
            fontSize: Fonts.size100,
            fontVariations: [Fonts.weightMedium],
            height: 1.0,
          ),
        ),
      ],
    );

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const SizedBox(height: Space.s100),
        heroText,
        const SizedBox(height: Space.s400),
        lexeText,
      ],
    );
  }
}

class LandingButtons extends StatelessWidget {
  const LandingButtons({
    super.key,
    required this.config,
    required this.onSignupPressed,
    required this.onRecoverPressed,
    required this.selectedPageIndex,
    required this.numPages,
    required this.onTapPrev,
    required this.onTapNext,
  });

  final Config config;

  final int numPages;
  final ValueListenable<int> selectedPageIndex;

  final VoidCallback onSignupPressed;
  final VoidCallback onRecoverPressed;
  final VoidCallback onTapPrev;
  final VoidCallback onTapNext;

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
          ),
        ),
        const SizedBox(height: Space.s300),

        // Signup ->
        LxFilledButton(
          onTap: this.onSignupPressed,
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.foreground,
            foregroundColor: LxColors.background,
            fixedSize: const Size(300.0, Space.s800),
          ),
          label: const Text("Create new wallet"),
          icon: const Icon(LxIcons.nextSecondary),
        ),
        const SizedBox(height: Space.s400),

        // Recover Wallet
        LxOutlinedButton(
          onTap: this.onRecoverPressed,
          style: ButtonStyle(
            fixedSize: WidgetStateProperty.all(const Size(300.0, Space.s800)),
          ),
          label: const Text("I have a Lexe wallet"),
        ),
        const SizedBox(height: Space.s400),
      ],
    );
  }
}

class InkuShader extends StatelessWidget {
  const InkuShader({
    super.key,
    required this.carouselScrollController,
    this.child,
  });

  final PageController carouselScrollController;
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
                "Error loading shader: ${snapshot.error}:\n${snapshot.stackTrace}");
            return const SizedBox();
          }
          if (!snapshot.hasData) {
            return const SizedBox();
          }

          return AnimatedShader(
            shader: snapshot.data!,
            carouselScrollController: this.carouselScrollController,
            child: this.child,
          );
        });
  }
}

class AnimatedShader extends StatefulWidget {
  const AnimatedShader({
    super.key,
    required this.shader,
    required this.carouselScrollController,
    this.child,
  });

  final ui.FragmentShader shader;
  final PageController carouselScrollController;
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
    unawaited(animationController.forward(from: 0.0));
  }

  @override
  void dispose() {
    animationController.dispose();
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

    return AnimatedBuilder(
      animation: this.animationController,
      builder: (BuildContext _, Widget? child) {
        // Add some small EMA dampening to scroll offset.
        const a = 0.25;
        final nextOffset = a * this.scrollOffset() + (1.0 - a) * prevOffset;
        prevOffset = nextOffset;

        return CustomPaint(
          painter: ShaderPainter(
            widget.shader,
            this.animationController.value,
            nextOffset,
          ),
          // raster cache probably shouldn't cache this since it changes every frame
          isComplex: false,
          willChange: true,
          child: child,
        );
      },
      child: widget.child,
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
    shader.setFloat(0, size.width);
    // 1 : u_resolution.y
    shader.setFloat(1, size.height);
    // 2 : u_time
    shader.setFloat(2, this.time);
    // 3 : u_scroll_offset
    final double normalizedScrollOffset = this.scrollOffset / size.height;
    shader.setFloat(3, normalizedScrollOffset);

    final screenRect = Rect.fromLTWH(0.0, 0.0, size.width, size.height);
    final paint = Paint()..shader = this.shader;

    canvas.drawRect(screenRect, paint);
  }

  @override
  bool shouldRepaint(covariant ShaderPainter oldDelegate) =>
      this.time != oldDelegate.time || shader != oldDelegate.shader;
}
