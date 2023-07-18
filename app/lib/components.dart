/// Reusable flutter UI components

import 'dart:async' show StreamController;

import 'package:flutter/material.dart';
import 'package:rxdart_ext/rxdart_ext.dart';

import '../../style.dart' show Fonts, LxColors, LxRadius, Space;

typedef VoidContextCallback = void Function(BuildContext);

/// A more robust body for a [Scaffold]. Use this widget when you expect the
/// body area to almost always be in view, but can gracefully handle smaller
/// viewports (like when the onscreen keyboard pops up).
///
/// * An optional `bottom` widget is available that will expand to fill as much
///   of the remaining viewport as possible. Use this if you want to e.g. anchor
///   some buttons to the bottom of the screen, but still have them scroll with
///   the body when the viewport is small.
///
///   This behavior contrasts with e.g. a TabBar, which instead stays fixed to
///   the bottom, is always visible on top, and forces body content to scroll
///   underneath.
///
/// NOTE(phlip9): There seem to be multiple ways to accomplish this and I'm not
/// really sure which is "best".
class ScrollableSinglePageBody extends StatelessWidget {
  const ScrollableSinglePageBody({
    super.key,
    required this.body,
    this.padding = const EdgeInsets.symmetric(horizontal: Space.s600),
    this.bottom,
    this.bottomAlignment = Alignment.bottomCenter,
    this.bottomPadding = const EdgeInsets.only(bottom: Space.s600),
  });

  final List<Widget> body;
  final EdgeInsets padding;
  final Widget? bottom;
  final Alignment bottomAlignment;
  final EdgeInsets bottomPadding;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: this.padding,
      child: CustomScrollView(
        primary: true,
        slivers: [
          // The primary body widgets.
          SliverList.list(children: this.body),

          // The bottom widgets; these expand to fill the available space.
          if (this.bottom != null)
            SliverFillRemaining(
              hasScrollBody: false,
              child: Align(
                alignment: this.bottomAlignment,
                child: Padding(
                  padding: this.bottomPadding,
                  child: this.bottom,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A simple colored box that we can show while some real content is loading.
///
/// The `width` and `height` are optional. If left `null`, that dimension will
/// be determined by the parent `Widget`'s constraints.
///
/// If the placeholder is replacing some text, `forText` should be set to `true`.
/// This is because a `Text` widget's actual rendered height also depends on the
/// current `MediaQuery.textScaleFactor`.
class FilledPlaceholder extends StatelessWidget {
  const FilledPlaceholder({
    super.key,
    this.color = LxColors.grey850,
    this.width = double.infinity,
    this.height = double.infinity,
    this.borderRadius = LxRadius.r200,
    this.forText = false,
    this.child,
  });

  final Color color;
  final double width;
  final double height;
  final double borderRadius;
  final bool forText;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    final double heightFactor;
    if (!this.forText) {
      heightFactor = 1.0;
    } else {
      heightFactor = MediaQuery.of(context).textScaleFactor;
    }

    return SizedBox(
      width: this.width,
      height: this.height * heightFactor,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: this.color,
          borderRadius: BorderRadius.circular(this.borderRadius),
        ),
        child: this.child,
      ),
    );
  }
}

enum LxCloseButtonKind {
  closeFromTop,
  closeFromRoot,
  closeDrawer,
}

/// × - Close button, usually placed on the [AppBar].
///
/// Example usage:
///
/// * Close an independent leaf page
/// * Abort a partially completed, multi-step form
/// * Exit a modal popup
/// * Close an app drawer
class LxCloseButton extends StatelessWidget {
  const LxCloseButton({
    super.key,
    this.kind = LxCloseButtonKind.closeFromTop,
  });

  final LxCloseButtonKind kind;

  void onTap(BuildContext context) {
    switch (this.kind) {
      case LxCloseButtonKind.closeFromTop:
        Navigator.of(context, rootNavigator: false).pop();
      case LxCloseButtonKind.closeFromRoot:
        Navigator.of(context, rootNavigator: true).pop();
      case LxCloseButtonKind.closeDrawer:
        Scaffold.of(context).closeDrawer();
    }
  }

  @override
  Widget build(BuildContext context) {
    return IconButton(
      icon: const Icon(Icons.close_rounded),
      onPressed: () => this.onTap(context),
    );
  }
}

/// ← - Back button, usually placed on the [AppBar] to go back a page in a
/// sub-flow.
///
/// Example usage:
///
/// * Go back to the previous page In a multi-step form.
class LxBackButton extends StatelessWidget {
  const LxBackButton({super.key});

  @override
  Widget build(BuildContext context) {
    return IconButton(
      icon: const Icon(Icons.arrow_back_rounded),
      onPressed: () => Navigator.of(context).pop(),
    );
  }
}

/// A filled button with an icon. Used as the primary action button.
///
/// It's like the standard `FilledButton.icon`, but the text is properly
/// centered in the button.
class LxFilledButton extends StatelessWidget {
  const LxFilledButton({
    super.key,
    required this.onTap,
    this.label,
    this.icon,
    this.style,
    this.textStyle,
  });

  final Widget? label;
  final Widget? icon;
  final VoidCallback? onTap;

  final ButtonStyle? style;
  final TextStyle? textStyle;

  @override
  Widget build(BuildContext context) {
    final ButtonStyle defaultStyle = FilledButton.styleFrom(
      backgroundColor: LxColors.grey1000,
      disabledBackgroundColor: LxColors.grey850,
      foregroundColor: LxColors.foreground,
      disabledForegroundColor: LxColors.grey725,
      maximumSize: const Size.fromHeight(Space.s700),
    );

    final ButtonStyle buttonStyle =
        (this.style != null) ? this.style!.merge(defaultStyle) : defaultStyle;

    const TextStyle defaultTextStyle = TextStyle(
      fontSize: Fonts.size300,
      fontVariations: [Fonts.weightMedium],
    );

    final TextStyle textStyle = (this.textStyle != null)
        ? this.textStyle!.merge(defaultTextStyle)
        : defaultTextStyle;

    return FilledButton(
      onPressed: this.onTap,
      style: buttonStyle,
      child: Stack(
        alignment: Alignment.center,
        children: [
          if (this.label != null)
            DefaultTextStyle.merge(style: textStyle, child: this.label!),
          if (this.icon != null)
            Align(
              alignment: Alignment.centerRight,
              child: IconTheme.merge(
                data: IconThemeData(size: textStyle.fontSize),
                child: this.icon!,
              ),
            )
        ],
      ),
    );
  }
}

typedef StateStreamWidgetBuilder<T> = Widget Function(
  BuildContext context,
  T data,
);

/// A small helper `Widget` that builds a new widget everytime a `StateStream`
/// gets an update.
///
/// This is slightly nicer than the standard `StreamBuilder` because
/// `StateStream`s always have an initial value and never error.
class StateStreamBuilder<T> extends StreamBuilder<T> {
  StateStreamBuilder({
    super.key,
    required StateStream<T> stream,
    required StateStreamWidgetBuilder builder,
  }) : super(
          stream: stream,
          initialData: stream.value,
          builder: (BuildContext context, AsyncSnapshot<T> snapshot) =>
              builder(context, snapshot.data),
        );
}

extension StreamControllerExt<T> on StreamController<T> {
  /// Calls `add(event)` as long as the `StreamController` is not already
  /// closed.
  void addIfNotClosed(T event) {
    if (!this.isClosed) {
      this.add(event);
    }
  }
}

/// A zigzag line that spans the width of its container.
///
/// zigzag -> \/\/\/\/\/\/\/
///
/// * [zigWidth] is the width of a single \/
/// * [strokeWidth] is the thickness of the line
class ZigZag extends StatelessWidget {
  const ZigZag({
    super.key,
    required this.color,
    required this.zigWidth,
    required this.strokeWidth,
  });

  final Color color;
  final double zigWidth;
  final double strokeWidth;

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      painter: ZigZagPainter(
        color: this.color,
        zigWidth: this.zigWidth,
        strokeWidth: this.strokeWidth,
      ),
      child: const Center(),
    );
  }
}

class ZigZagPainter extends CustomPainter {
  const ZigZagPainter({
    required this.color,
    required this.zigWidth,
    required this.strokeWidth,
  }) : assert(zigWidth > 0.0 && strokeWidth > 0.0);

  final Color color;
  final double zigWidth;
  final double strokeWidth;

  @override
  void paint(Canvas canvas, Size size) {
    // |                                   |
    // |   `\     /`\         /`\     /`   |  |
    // |     \   /   \  ...  /   \   /     | (3) step
    // |      \./                 \./      |  |
    // |                                   |
    // |(1)|--(2)--|    ...    |--(2)--|(1)|
    // |   |(3)|(3)|           |(3)|(3)|   |
    // |----------------(4)----------------|
    //
    // (1) margin = 0.5 * totalMargin = 0.5 * floor(size.width - zigWidth)
    // (2) zigWidth
    // (3) step = 0.5 * zigWidth
    // (4) size.width

    // The most number of whole zigs we can fit within the span.
    final numZigs = (size.width / this.zigWidth).truncate();
    final step = 0.5 * this.zigWidth;

    // The extra margin we'll have on each side of the zigzag.
    final totalMargin = size.width - (numZigs * this.zigWidth);
    final margin = 0.5 * totalMargin;

    // start coordinates
    final startX = margin;
    final startY = 0.5 * (size.height - step);

    canvas.save();
    canvas.translate(startX, startY);

    // extra little prefix bit to fill the full width
    final path = Path()..moveTo(-margin, margin);

    path.lineTo(0.0, 0.0);

    for (var idx = 0; idx < numZigs; idx += 1) {
      final x1 = (2 * idx + 1) * step;
      final y1 = step;

      final x2 = (2 * idx + 2) * step;
      const y2 = 0.0;

      path.lineTo(x1, y1);
      path.lineTo(x2, y2);
    }

    // extra little suffix bit to fill the full width
    path.relativeLineTo(margin, margin);

    final paint = Paint()
      ..color = this.color
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = this.strokeWidth;
    canvas.drawPath(path, paint);
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant ZigZagPainter oldDelegate) {
    return this.color != oldDelegate.color ||
        this.zigWidth != oldDelegate.zigWidth ||
        this.strokeWidth != oldDelegate.strokeWidth;
  }
}
