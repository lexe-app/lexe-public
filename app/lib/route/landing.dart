import 'dart:async' show unawaited;
import 'dart:math' show max;
import 'dart:ui' as ui;

import 'package:flutter/cupertino.dart' show CupertinoScrollBehavior;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;

import '../bindings_generated_api.dart' show AppHandle, Config;
import '../components.dart' show LxOutlinedButton;
import '../gdrive_auth.dart' show GDriveAuth;
import '../logger.dart' show error, info;
import '../style.dart' show Fonts, LxColors, LxTheme, Space;
import 'signup.dart' show SignupApi, SignupPage;
import 'wallet.dart' show WalletPage;

class LandingPage extends StatefulWidget {
  const LandingPage({
    super.key,
    required this.config,
    required this.gdriveAuth,
    required this.signupApi,
  });

  final Config config;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;

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
      ),
    )));
  }

  /// Start the Wallet Restore UI flow. Future resolves when the user has either
  /// (1) completed the flow and restored or (2) canceled the flow.
  Future<void> doRestoreFlow() async {
    info("do restore flow");
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
                        ),
                      ),
                    ),
                  ]),
                ),
              );
            },
          ),

          // Next page button.
          Align(
            alignment: Alignment.centerRight,
            child: ValueListenableBuilder(
              valueListenable: this.selectedPageIndex,
              builder: (context, selectedPageIndex, child) => IconButton(
                icon: const Icon(Icons.chevron_right_rounded),
                iconSize: Fonts.size800,
                color: LxColors.clearB300,
                disabledColor: LxColors.clearB100,
                onPressed:
                    (selectedPageIndex != numPages - 1) ? this.nextPage : null,
              ),
            ),
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

class LandingCarouselIndicators extends StatelessWidget {
  const LandingCarouselIndicators({
    super.key,
    required this.selectedPageIndex,
    required this.numPages,
  });

  final int numPages;
  final ValueListenable<int> selectedPageIndex;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: List<Widget>.generate(
          this.numPages,
          (index) => LandingCarouselIndicator(
              index: index, selectedPageIndex: this.selectedPageIndex)),
    );
  }
}

class LandingCarouselIndicator extends StatelessWidget {
  const LandingCarouselIndicator({
    super.key,
    required this.index,
    required this.selectedPageIndex,
  });

  final int index;
  final ValueListenable<int> selectedPageIndex;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.s100),
      child: ValueListenableBuilder(
        valueListenable: this.selectedPageIndex,
        builder: (context, selectedPageIndex, child) {
          final isActive = selectedPageIndex == this.index;

          return AnimatedContainer(
            duration: const Duration(milliseconds: 250),
            height: 6.0,
            width: isActive ? 20 : 6,
            decoration: BoxDecoration(
              color: isActive ? LxColors.clearB600 : LxColors.clearB200,
              borderRadius: const BorderRadius.all(Radius.circular(12)),
            ),
          );
        },
      ),
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
  });

  final Config config;
  final VoidCallback onSignupPressed;
  final VoidCallback onRecoverPressed;
  final int numPages;
  final ValueListenable<int> selectedPageIndex;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        // Indicator dots to show which page we're on.
        LandingCarouselIndicators(
          numPages: this.numPages,
          selectedPageIndex: this.selectedPageIndex,
        ),
        const SizedBox(height: Space.s500),

        // Signup ->
        FilledButton(
          onPressed: this.onSignupPressed,
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.foreground,
            disabledBackgroundColor: LxColors.clearB300,
            foregroundColor: LxColors.background,
            disabledForegroundColor: LxColors.clearW200,
            fixedSize: const Size(300.0, Space.s750),
          ),
          child: const CreateWalletText(),
        ),
        const SizedBox(height: Space.s400),

        // Recover Wallet
        LxOutlinedButton(
          onTap: this.onRecoverPressed,
          style: ButtonStyle(
            side: MaterialStateProperty.all(
                const BorderSide(color: LxColors.clearB600, width: 2.0)),
            fixedSize: MaterialStateProperty.all(const Size(300.0, Space.s750)),
          ),
          label: const Text(
            "I have a Lexe wallet",
            style: TextStyle(
              fontSize: Fonts.size300,
              color: LxColors.clearB800,
            ),
          ),
        ),
        const SizedBox(height: Space.s400),
      ],
    );
  }
}

class CreateWalletText extends StatelessWidget {
  const CreateWalletText({super.key});

  @override
  Widget build(BuildContext context) {
    return Stack(
      alignment: Alignment.center,
      children: [
        Text(
          "Create new wallet",
          style: Fonts.fontUI.copyWith(
            color: LxColors.background,
            fontSize: Fonts.size300,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
        Container(
          alignment: Alignment.centerRight,
          padding: const EdgeInsets.all(8.0),
          child: const Icon(Icons.chevron_right_rounded),
        )
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
            carouselScrollController: carouselScrollController,
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

  @override
  Widget build(BuildContext context) {
    final scrollController = this.widget.carouselScrollController;
    double prevOffset = scrollController.offset;

    return AnimatedBuilder(
      animation: this.animationController,
      builder: (BuildContext _, Widget? child) {
        // Add some small EMA dampening to scroll offset.
        const a = 0.25;
        final nextOffset = a * scrollController.offset + (1.0 - a) * prevOffset;
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
