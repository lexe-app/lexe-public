import 'dart:ui' as ui;
import 'package:flutter/material.dart';

import 'bindings.dart' show api;

void main() {
  runApp(const LexeApp());
}

class LexeApp extends StatelessWidget {
  const LexeApp({super.key});

  @override
  Widget build(BuildContext context) {
    return const MaterialApp(
      title: 'Lexe',
      home: LandingPage(),
    );
  }
}

class LandingPage extends StatelessWidget {
  const LandingPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
        backgroundColor: const Color(0xff353535),
        body: InkuShader(
            child: Center(
          child: Container(
              padding: const EdgeInsets.only(top: 128.0, bottom: 64.0),
              constraints: const BoxConstraints.expand(width: 300.0),
              child: Column(
                mainAxisSize: MainAxisSize.max,
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.center,
                children: const [
                  Flexible(
                      flex: 2,
                      child: Align(
                          alignment: Alignment.center,
                          child: LandingCalloutText())),
                  Flexible(
                    flex: 1,
                    child: Align(
                        alignment: Alignment.center, child: LandingButtons()),
                  )
                ],
              )),
        )));
  }
}

class LandingCalloutText extends StatelessWidget {
  const LandingCalloutText({super.key});

  @override
  Widget build(BuildContext context) {
    const heroText = Text(
      "LIGHTNING.\nBITCOIN.\nONE WALLET.",
      overflow: TextOverflow.clip,
      style: TextStyle(
        color: Colors.white,
        height: 1.5,
        fontSize: 40.0,
        fontFamily: "Hubot Sans",
        fontVariations: [
          ui.FontVariation("wght", 700),
          ui.FontVariation("wdth", 90),
        ],
        decoration: TextDecoration.none,
      ),
    );

    final lexeText = Row(
      // mainAxisSize: MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.end,
      children: const [
        // SizedBox(width: 128.0),
        Text("brought to you by",
            style: TextStyle(
              fontFamily: "Mona Sans",
              color: Colors.white54,
              fontSize: 12.0,
              decoration: TextDecoration.none,
            )),
        SizedBox(width: 4.0),
        Text("LEXE™",
            style: TextStyle(
              fontFamily: "Hubot Sans",
              color: Colors.white60,
              fontSize: 12.0,
              fontVariations: [
                ui.FontVariation("wght", 500),
                ui.FontVariation("wdth", 100),
              ],
              decoration: TextDecoration.none,
            )),
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
        Icon(Icons.circle, size: 12.0, color: Colors.white60),
        SizedBox(width: 12.0),
        Icon(Icons.circle, size: 12.0, color: Colors.white24),
        SizedBox(width: 12.0),
        Icon(Icons.circle, size: 12.0, color: Colors.white24),
      ],
    );
  }
}

class LandingButtons extends StatelessWidget {
  const LandingButtons({super.key});

  @override
  Widget build(BuildContext context) {
    const buttonSize = Size(300.0, 56.0);

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const LandingCarouselIndicators(),
        const SizedBox(height: 24.0),
        FilledButton(
          onPressed: () => debugPrint("pressed create wallet button"),
          style: FilledButton.styleFrom(
            backgroundColor: Colors.white,
            foregroundColor: Colors.black,
            fixedSize: buttonSize,
          ),
          child: Stack(
            alignment: Alignment.center,
            children: [
              const Text("Create new wallet",
                  style: TextStyle(
                    fontSize: 16.0,
                    fontVariations: [ui.FontVariation("wght", 400)],
                  )),
              Container(
                alignment: Alignment.centerRight,
                padding: const EdgeInsets.all(8.0),
                child: const Icon(Icons.chevron_right),
              )
            ],
          ),
        ),
        const SizedBox(height: 16.0),
        OutlinedButton(
          onPressed: () => debugPrint("Rust FFI test: ${api.hello()}"),
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: Colors.white70, width: 2.0),
            padding: const EdgeInsets.symmetric(horizontal: 32.0),
            fixedSize: buttonSize,
            shape: const StadiumBorder(),
          ),
          child: const Text("I have a Lexe wallet",
              style: TextStyle(
                color: Colors.white70,
                fontSize: 16.0,
                fontVariations: [ui.FontVariation("wght", 400)],
              )),
        ),
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

          return AnimatedShader(shader: snapshot.data!, child: child);
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
    animationController = AnimationController(
      vsync: this,
      upperBound: 10000.0,
      duration: const Duration(seconds: 10000), // why no infinite animation??
    );
    animationController.forward(from: 0.0);
  }

  @override
  void dispose() {
    animationController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: animationController,
      builder: (BuildContext _, Widget? child) => CustomPaint(
        painter: ShaderPainter(widget.shader, animationController.value),
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
    shader.setFloat(2, time);

    final screenRect = Rect.fromLTWH(0.0, 0.0, size.width, size.height);
    final paint = Paint()..shader = shader;

    canvas.drawRect(screenRect, paint);
  }

  @override
  bool shouldRepaint(covariant ShaderPainter oldDelegate) =>
      time != oldDelegate.time || shader != oldDelegate.shader;
}
