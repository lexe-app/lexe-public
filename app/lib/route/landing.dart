import 'dart:async' show unawaited;
import 'dart:math' show max;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show SystemUiOverlayStyle;

import '../bindings_generated_api.dart' show AppHandle, Config;
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

class _LandingPageState extends State<LandingPage> {
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

  @override
  Widget build(BuildContext context) {
    // set the SystemUiOverlay bars to transparent so the background shader
    // shows through.
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: LxTheme.systemOverlayStyleLight.copyWith(
        statusBarColor: LxColors.clearW0,
        systemNavigationBarColor: LxColors.clearW0,
        systemNavigationBarDividerColor: LxColors.clearW0,
      ),
      child: Scaffold(
        backgroundColor: LxColors.background,
        body: Stack(children: [
          const InkuShader(child: Center()),
          LayoutBuilder(
            builder: (BuildContext context, BoxConstraints viewport) {
              final viewportHeight = viewport.maxHeight;

              const width = 300.0;
              const minHeight = 525.0;
              const verticalBreakpoint = 700.0;

              final maxHeight = max(minHeight, viewportHeight);
              final top = (viewportHeight > verticalBreakpoint) ? 196.0 : 64.0;
              final bottom =
                  (viewportHeight > verticalBreakpoint) ? 64.0 : 32.0;

              return Center(
                child: Container(
                  constraints: BoxConstraints(
                    minWidth: width,
                    maxWidth: width,
                    minHeight: minHeight,
                    maxHeight: maxHeight,
                  ),
                  child: Stack(fit: StackFit.passthrough, children: [
                    Container(
                      padding: EdgeInsets.only(top: top),
                      child: const LandingCalloutText(),
                    ),
                    Container(
                      padding: EdgeInsets.only(bottom: bottom),
                      alignment: Alignment.bottomCenter,
                      child: LandingButtons(
                        config: this.widget.config,
                        onSignupPressed: () => unawaited(this.doSignupFlow()),
                        onRecoverPressed: () => unawaited(this.doRestoreFlow()),
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
  const LandingCalloutText({super.key});

  @override
  Widget build(BuildContext context) {
    final heroText = Text(
      "LIGHTNING.\nBITCOIN.\nONE WALLET.",
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
        heroText,
        const SizedBox(height: 16.0),
        lexeText,
      ],
    );
  }
}

class LandingCarouselIndicators extends StatelessWidget {
  const LandingCarouselIndicators({super.key});

  @override
  Widget build(BuildContext context) {
    return const Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Icon(Icons.circle, size: 12.0, color: LxColors.clearB600),
        SizedBox(width: 12.0),
        Icon(Icons.circle, size: 12.0, color: LxColors.clearB200),
        SizedBox(width: 12.0),
        Icon(Icons.circle, size: 12.0, color: LxColors.clearB200),
      ],
    );
  }
}

class LandingButtons extends StatelessWidget {
  const LandingButtons(
      {super.key,
      required this.config,
      required this.onSignupPressed,
      required this.onRecoverPressed});

  final Config config;
  final VoidCallback onSignupPressed;
  final VoidCallback onRecoverPressed;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const LandingCarouselIndicators(),
        const SizedBox(height: 24.0),
        // Signup
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

        const SizedBox(height: 16.0),
        // Recover Wallet
        OutlinedButton(
          onPressed: this.onRecoverPressed,
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: LxColors.clearB600, width: 2.0),
            padding: const EdgeInsets.symmetric(horizontal: 32.0),
            fixedSize: const Size(300.0, Space.s750),
            shape: const StadiumBorder(),
          ),
          child: const Text(
            "I have a Lexe wallet",
            style: TextStyle(
              fontFamily: "Inter V",
              fontSize: Fonts.size300,
              color: LxColors.clearB800,
              height: 1.0,
              decoration: TextDecoration.none,
            ),
          ),
        ),
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
  const InkuShader({super.key, this.child});

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

          return AnimatedShader(shader: snapshot.data!, child: this.child);
        });
  }
}

class AnimatedShader extends StatefulWidget {
  const AnimatedShader({super.key, required this.shader, this.child});

  final ui.FragmentShader shader;
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
    return AnimatedBuilder(
      animation: this.animationController,
      builder: (BuildContext _, Widget? child) => CustomPaint(
        painter: ShaderPainter(widget.shader, this.animationController.value),
        // raster cache probably shouldn't cache this since it changes every frame
        isComplex: false,
        willChange: true,
        child: child,
      ),
      child: widget.child,
    );
  }
}

class ShaderPainter extends CustomPainter {
  const ShaderPainter(this.shader, this.time);

  final ui.FragmentShader shader;
  final double time;

  @override
  void paint(ui.Canvas canvas, ui.Size size) {
    // set shader uniforms
    // 0 : u_resolution.x
    shader.setFloat(0, size.width);
    // 1 : u_resolution.y
    shader.setFloat(1, size.height);
    // 2 : u_time
    shader.setFloat(2, this.time);

    final screenRect = Rect.fromLTWH(0.0, 0.0, size.width, size.height);
    final paint = Paint()..shader = this.shader;

    canvas.drawRect(screenRect, paint);
  }

  @override
  bool shouldRepaint(covariant ShaderPainter oldDelegate) =>
      this.time != oldDelegate.time || shader != oldDelegate.shader;
}
