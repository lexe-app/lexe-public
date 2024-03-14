/// Reusable flutter UI components
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show MaxLengthEnforcement;
import 'package:rxdart_ext/rxdart_ext.dart';

import 'bindings_generated.dart' show MAX_PAYMENT_NOTE_BYTES;
import 'input_formatter.dart' show MaxUtf8BytesInputFormatter;
import 'style.dart' show Fonts, LxColors, LxRadius, Space;

typedef VoidContextCallback = void Function(BuildContext);

const InputDecoration baseInputDecoration = InputDecoration(
  hintStyle: TextStyle(color: LxColors.grey750),
  filled: true,
  fillColor: LxColors.clearB0,
  // hoverColor: LxColors.clearB50,
  // Remove left and right padding so we have more room for
  // amount text.
  contentPadding: EdgeInsets.symmetric(vertical: Space.s300),
  // errorBorder: InputBorder.none,
  focusedBorder: InputBorder.none,
  // focusedErrorBorder: InputBorder.none,
  disabledBorder: InputBorder.none,
  enabledBorder: InputBorder.none,
);

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

/// Start a new multistep UI flow.
///
/// This widget enables the Back button vs Close button logic, where the back
/// button takes you back one page in the flow and the close button closes exits
/// the flow entirely.
///
/// It works by creating a new child [Navigator] to contain the pages within the
/// flow. The back button pops pages from this child [Navigator], while the
/// close button pops the whole stack from the parent [Navigator].
class MultistepFlow<T> extends StatelessWidget {
  const MultistepFlow({super.key, required this.builder});

  final WidgetBuilder builder;

  @override
  Widget build(BuildContext context) {
    final parentNavigator = Navigator.of(context);

    return Navigator(
      onGenerateRoute: (RouteSettings settings) {
        return _PopToParentRoute<T>(
          parentNavigator: parentNavigator,
          settings: settings,
          builder: builder,
        );
      },
    );
  }
}

/// A tiny wrapper around [MaterialPageRoute] that just propagates the results
/// of the current [Navigator.pop] to a [parentNavigator].
class _PopToParentRoute<T> extends MaterialPageRoute<T> {
  _PopToParentRoute({
    required super.builder,
    super.settings,
    required this.parentNavigator,
  });

  final NavigatorState parentNavigator;

  @override
  bool didPop(T? result) {
    final superDidPop = super.didPop(result);
    parentNavigator.pop(result);
    return superDidPop;
  }

  /// maybePop => always pop
  @override
  RoutePopDisposition get popDisposition => RoutePopDisposition.pop;
}

/// It animates into a shortened button with a loading indicator inside when
/// we're e.g. sending a payment request and awaiting the response.
class AnimatedFillButton extends StatefulWidget {
  const AnimatedFillButton({
    super.key,
    required this.onTap,
    required this.loading,
    required this.label,
    required this.icon,
    this.style,
  });

  final VoidCallback? onTap;
  final bool loading;
  final Widget label;
  final Widget icon;
  final ButtonStyle? style;

  bool get enabled => this.onTap != null;

  @override
  State<AnimatedFillButton> createState() => _AnimatedFillButtonState();
}

class _AnimatedFillButtonState extends State<AnimatedFillButton> {
  @override
  Widget build(BuildContext context) {
    final loading = this.widget.loading;

    // When we're loading, we:
    // (1) shorten and disable the button width
    // (2) replace the button label with a loading indicator
    // (3) hide the button icon

    return AnimatedContainer(
      duration: const Duration(milliseconds: 200),
      curve: Curves.decelerate,
      // We need to set a maximum width, since we can't interpolate between an
      // unbounded width and a finite width.
      width: (!loading) ? 450.0 : Space.s900,
      child: LxFilledButton(
        // Disable the button while loading.
        onTap: (!loading) ? this.widget.onTap : null,
        label: AnimatedSwitcher(
          duration: const Duration(milliseconds: 150),
          child: (!loading)
              ? this.widget.label
              : const Center(
                  child: SizedBox.square(
                    dimension: Fonts.size300,
                    child: CircularProgressIndicator(
                      strokeWidth: 2.0,
                      color: LxColors.clearB200,
                    ),
                  ),
                ),
        ),
        icon: AnimatedOpacity(
          opacity: (!loading) ? 1.0 : 0.0,
          duration: const Duration(milliseconds: 150),
          child: this.widget.icon,
        ),
        style: this.widget.style,
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
    final double height;
    if (!this.forText) {
      height = this.height;
    } else {
      height = MediaQuery.of(context).textScaler.scale(this.height);
    }

    return SizedBox(
      width: this.width,
      height: height,
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
      // This alignment positions the button correctly on both sides of the app
      // bar.
      alignment: Alignment.centerRight,
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
      // This alignment positions the button correctly on both sides of the app
      // bar.
      alignment: Alignment.centerRight,
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

/// Heading/title text that sits directly beneath the AppBar.
class HeadingText extends StatelessWidget {
  const HeadingText({
    super.key,
    required this.text,
  });

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: Space.s500, bottom: Space.s200),
      child: Text(
        this.text,
        style: const TextStyle(
          fontSize: Fonts.size600,
          fontVariations: [Fonts.weightMedium],
          letterSpacing: -0.5,
          height: 1.0,
        ),
      ),
    );
  }
}

class SubheadingText extends StatelessWidget {
  const SubheadingText({super.key, required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Text(
      this.text,
      style: Fonts.fontUI.copyWith(
        color: LxColors.grey600,
        fontSize: Fonts.size300,
      ),
    );
  }
}

/// Text entry field for a user to set a payment's note.
class PaymentNoteInput extends StatelessWidget {
  const PaymentNoteInput({
    super.key,
    required this.fieldKey,
    required this.onSubmit,
    this.initialNote,
    this.isEnabled = true,
  });

  final GlobalKey<FormFieldState<String>> fieldKey;
  final VoidCallback onSubmit;
  final String? initialNote;
  final bool isEnabled;

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      key: this.fieldKey,

      // Disable the input field while the send request is pending.
      enabled: this.isEnabled,

      initialValue: this.initialNote,

      autofocus: false,
      keyboardType: TextInputType.text,
      textInputAction: TextInputAction.send,
      onEditingComplete: this.onSubmit,
      maxLines: null,
      maxLength: 200,
      maxLengthEnforcement: MaxLengthEnforcement.enforced,

      // Silently limit input to 512 bytes. This could be a little
      // confusing if the user inputs a ton of emojis or CJK characters
      // I guess.
      inputFormatters: const [
        MaxUtf8BytesInputFormatter(maxBytes: MAX_PAYMENT_NOTE_BYTES),
      ],

      decoration: const InputDecoration(
        hintStyle: TextStyle(color: LxColors.grey550),
        hintText: "What's this payment for? (optional)",
        counterStyle: TextStyle(color: LxColors.grey550),
        border: OutlineInputBorder(),
        enabledBorder: OutlineInputBorder(
            borderSide: BorderSide(color: LxColors.fgTertiary)),
        focusedBorder: OutlineInputBorder(
            borderSide: BorderSide(color: LxColors.foreground)),
      ),

      // Only show "XX/YY" character limit counter when text area is focused.
      buildCounter: (context,
              {required int currentLength,
              required int? maxLength,
              required bool isFocused}) =>
          (isFocused && maxLength != null)
              ? Text("$currentLength/$maxLength",
                  style: const TextStyle(
                      fontSize: Fonts.size100,
                      color: LxColors.grey550,
                      height: 1.0))
              : const SizedBox(height: Fonts.size100),

      style: Fonts.fontBody.copyWith(
        fontSize: Fonts.size200,
        height: 1.5,
        color: LxColors.fgSecondary,
        letterSpacing: -0.15,
      ),
    );
  }
}

typedef StateStreamWidgetBuilder<T> = Widget Function(
  BuildContext context,
  T data,
);

/// A small helper [Widget] that builds a new widget every time a [StateStream]
/// gets an update.
///
/// This widget can be more convenient than a standard [StreamBuilder] because
/// [StateStream]s always have an initial value and never error.
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

/// A dashed line that spans the width of its container.
class DashPainter extends CustomPainter {
  const DashPainter({
    required this.color,
    this.dashWidth = 4.0,
    this.dashSpace = 4.0,
    this.dashThickness = 1.5,
  });

  final Color color;
  final double dashWidth;
  final double dashSpace;
  final double dashThickness;

  @override
  void paint(Canvas canvas, Size size) {
    final dashAndSpace = this.dashWidth + this.dashSpace;

    final numDashes = ((size.width / dashAndSpace)).ceil();

    final path = Path()..moveTo(0.0, 0.0);

    for (var idx = 0; idx < numDashes; idx += 1) {
      path.relativeLineTo(this.dashWidth, 0.0);
      path.relativeMoveTo(this.dashSpace, 0.0);
    }

    final paint = Paint()
      ..color = this.color
      ..style = PaintingStyle.stroke
      ..strokeWidth = this.dashThickness;
    canvas.drawPath(path, paint);
  }

  @override
  bool shouldRepaint(covariant DashPainter oldDelegate) {
    return this.color != oldDelegate.color ||
        this.dashWidth != oldDelegate.dashWidth ||
        this.dashSpace != oldDelegate.dashSpace ||
        this.dashThickness != oldDelegate.dashThickness;
  }
}
