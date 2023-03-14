import 'dart:async' show unawaited;
import 'dart:math' show max;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:lexeapp/cfg.dart';

import '../bindings.dart' show api;
import '../bindings_generated_api.dart' show AppHandle;
import '../style.dart' show Fonts, LxColors;
import 'backup_wallet.dart' show BackupWalletPage;

class LandingPage extends StatelessWidget {
  const LandingPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
        backgroundColor: LxColors.foreground,
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
                      child: const LandingButtons(),
                    ),
                  ]),
                ),
              );
            },
          ),
        ]));
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
        color: LxColors.background,
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
            color: LxColors.clearW500,
            fontSize: Fonts.size100,
            fontVariations: [Fonts.weightExtraLight],
          ),
        ),
        const SizedBox(width: 4.0),
        Text(
          "LEXE™",
          style: Fonts.fontHubot.copyWith(
            color: LxColors.clearW600,
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
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: const [
        Icon(Icons.circle, size: 12.0, color: LxColors.clearW600),
        SizedBox(width: 12.0),
        Icon(Icons.circle, size: 12.0, color: LxColors.clearW200),
        SizedBox(width: 12.0),
        Icon(Icons.circle, size: 12.0, color: LxColors.clearW200),
      ],
    );
  }
}

class LandingButtons extends StatelessWidget {
  const LandingButtons({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const LandingCarouselIndicators(),
        const SizedBox(height: 24.0),
        const CreateWalletButton(),
        const SizedBox(height: 16.0),
        OutlinedButton(
          onPressed: () => debugPrint("pressed recover wallet button"),
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: LxColors.clearW600, width: 2.0),
            padding: const EdgeInsets.symmetric(horizontal: 32.0),
            fixedSize: const Size(300.0, 56.0),
            shape: const StadiumBorder(),
          ),
          child: const Text("I have a Lexe wallet",
              style: TextStyle(
                fontFamily: "Inter V",
                fontSize: Fonts.size300,
                color: LxColors.clearW600,
                height: 1.0,
                decoration: TextDecoration.none,
              )),
        ),
      ],
    );
  }
}

class CreateWalletButton extends StatefulWidget {
  const CreateWalletButton({super.key});

  @override
  State<CreateWalletButton> createState() => _CreateWalletButtonState();
}

class _CreateWalletButtonState extends State<CreateWalletButton> {
  bool _disableButton = false;

  Future<void> _onPressed() async {
    debugPrint("pressed create wallet button");

    // disable button
    setState(() => this._disableButton = true);
    final AppHandle app;
    try {
      app = await AppHandle.signup(bridge: api, config: config);
    } catch (err) {
      setState(() => this._disableButton = false);
      // ScaffoldMessenger.of(context)
      //     .showSnackBar(SnackBar(content: Text("$err")));
      rethrow;
    }

    // TODO(phlip9): disable restore button while request is processing? o/w
    // user could navigate away while account is getting created...

    debugPrint("done signing up");

    if (context.mounted) {
      unawaited(Navigator.of(context).pushReplacement(MaterialPageRoute(
        maintainState: false,
        builder: (BuildContext _) => BackupWalletPage(app: app),
      )));
    }
  }

  @override
  Widget build(BuildContext context) {
    return FilledButton(
      onPressed: this._disableButton ? null : this._onPressed,
      style: FilledButton.styleFrom(
        backgroundColor: LxColors.background,
        disabledBackgroundColor: LxColors.clearW200,
        foregroundColor: LxColors.foreground,
        disabledForegroundColor: LxColors.clearB300,
        fixedSize: const Size(300.0, 56.0),
      ),
      child: (!this._disableButton)
          ? const CreateWalletText()
          : const SizedBox.square(
              dimension: 24.0,
              child: CircularProgressIndicator(
                strokeWidth: 3.0,
                color: LxColors.clearW200,
              )),
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
            fontSize: Fonts.size300,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
        Container(
          alignment: Alignment.centerRight,
          padding: const EdgeInsets.all(8.0),
          child: const Icon(Icons.chevron_right),
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
            debugPrintStack(
                stackTrace: snapshot.stackTrace,
                label: "Error loading shader: ${snapshot.error}");
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
