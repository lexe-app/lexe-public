/// Reusable flutter UI components
library;

import 'dart:async' show Timer, unawaited;
import 'dart:io' show Platform;
import 'dart:math' show max;

import 'package:flutter/cupertino.dart'
    show CupertinoScrollBehavior, CupertinoSliverRefreshControl;
import 'package:flutter/foundation.dart' show ValueListenable, clampDouble;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show MaxLengthEnforcement;
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/input_formatter.dart'
    show IntInputFormatter, MaxUtf8BytesInputFormatter;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/string_ext.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxBreakpoints, LxColors, LxIcons, LxRadius, Space;
import 'package:lexeapp/types.dart' show BalanceKind, BalanceState, FiatAmount;
import 'package:lexeapp/url.dart' as url;
import 'package:rxdart_ext/rxdart_ext.dart';

// TODO(phlip9): frb no longer exposing consts?
// ignore: constant_identifier_names
const int MAX_PAYMENT_NOTE_BYTES = 512;

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
/// * If you need Slivers for the body widgets, then use `bodySlivers` instead
///   of `body`. `bodySlivers` only accepts Sliver widgets and `body` only
///   accepts Box widgets.
///
/// NOTE(phlip9): There seem to be multiple ways to accomplish this and I'm not
/// really sure which is "best".
class ScrollableSinglePageBody extends StatelessWidget {
  const ScrollableSinglePageBody({
    super.key,
    this.body,
    this.bodySlivers,
    this.useFullWidth = false,
    this.padding = const EdgeInsets.symmetric(horizontal: Space.s600),
    this.bottom,
    this.bottomAlignment = Alignment.bottomCenter,
    this.bottomPadding = const EdgeInsets.only(bottom: Space.s600),
  }) : // can't both be non-null
       assert(body == null || bodySlivers == null);

  /// If true, this page will always use the full screen width. Otherwise, by
  /// default, the page will be centered with a max-width on larger screens.
  final bool useFullWidth;

  final List<Widget>? body;
  final List<Widget>? bodySlivers;

  final EdgeInsets padding;
  final Widget? bottom;
  final Alignment bottomAlignment;
  final EdgeInsets bottomPadding;

  @override
  Widget build(BuildContext context) {
    const maxWidth = LxBreakpoints.mobile;

    // Calculate left-right margin so page is centered with at most maxWidth
    final EdgeInsets innerPadding;
    if (!useFullWidth) {
      final width = MediaQuery.sizeOf(context).width;

      innerPadding = (width <= maxWidth)
          ? EdgeInsets.zero
          : EdgeInsets.symmetric(horizontal: 0.5 * (width - maxWidth));
    } else {
      innerPadding = EdgeInsets.zero;
    }

    final body = this.body;
    final bodySlivers = this.bodySlivers;
    final bottom = this.bottom;

    final sliversPrePadding = <Widget>[
      // The primary body widgets (if sliver widgets).
      if (bodySlivers != null) ...bodySlivers,
      // The primary body widgets (if box widgets).
      if (body != null && body.length >= 2) SliverList.list(children: body),
      if (body != null && body.length == 1)
        SliverToBoxAdapter(child: body.first),
      // The bottom widgets; these expand to fill the available space.
      if (bottom != null)
        SliverFillRemaining(
          hasScrollBody: false,
          child: Align(
            alignment: this.bottomAlignment,
            child: Padding(padding: this.bottomPadding, child: bottom),
          ),
        ),
    ];

    final List<Widget> slivers = (!this.useFullWidth)
        ? sliversPrePadding
              .map(
                (sliver) => SliverPadding(
                  padding: innerPadding,
                  sliver: SliverConstrainedCrossAxis(
                    maxExtent: maxWidth,
                    sliver: sliver,
                  ),
                ),
              )
              .toList()
        : sliversPrePadding;

    return Padding(
      padding: this.padding,
      child: CustomScrollView(
        primary: true,
        slivers: slivers,
        scrollBehavior: const CupertinoScrollBehavior(),
      ),
    );
  }
}

/// Add this sliver as the first child of a [ScrollableSinglePageBody] or
/// [CustomScrollView] to enable pull-to-refresh in that scrollable.
///
/// This pull-to-refresh doesn't provide any visual feedback, just a haptic
/// buzz when the refresh is armed.
class SliverPullToRefresh extends CupertinoSliverRefreshControl {
  SliverPullToRefresh({super.key, required VoidCallback? onRefresh})
    : super(
        builder: null,
        refreshIndicatorExtent: 0.0,
        onRefresh: (onRefresh != null) ? (() async => onRefresh()) : null,
      );
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
                    dimension: Fonts.size400,
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
/// Don't use this widget to replace [Text]. Use the specialized
/// [FilledTextPlaceholder], which handles font sizing and baseline alignment
/// properly.
class FilledPlaceholder extends StatelessWidget {
  const FilledPlaceholder({
    super.key,
    this.color = LxColors.grey850,
    this.width = double.infinity,
    this.height = double.infinity,
    this.borderRadius = LxRadius.r200,
    this.child,
  });

  final Color color;
  final double width;
  final double height;
  final double borderRadius;
  final Widget? child;

  @override
  Widget build(BuildContext context) => SizedBox(
    width: this.width,
    height: this.height,
    child: DecoratedBox(
      decoration: BoxDecoration(
        color: this.color,
        borderRadius: BorderRadius.circular(this.borderRadius),
      ),
      child: this.child,
    ),
  );
}

/// A simple colored box that we can show while we wait for some text content to
/// load.
///
/// Like [FilledPlaceholder] but specialized for replacing [Text].
///
/// It contains an inner `Text(" ", style: this.style)`, so it has perfect
/// height sizing and generates an accurate text baseline for e.g.
/// [CrossAxisAlignment.baseline] to align against.
///
// TODO(phlip9): there's probably a more efficient way to get _just_ the sizing
// and baseline generation from `Text`, but this works for now.
class FilledTextPlaceholder extends StatelessWidget {
  const FilledTextPlaceholder({
    super.key,
    this.color = LxColors.grey850,
    this.width = double.infinity,
    this.borderRadius = LxRadius.r200,
    this.style,
  });

  final Color color;
  final double width;
  final double borderRadius;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) => SizedBox(
    width: this.width,
    child: DecoratedBox(
      decoration: BoxDecoration(
        color: this.color,
        borderRadius: BorderRadius.circular(this.borderRadius),
      ),
      child: Text(
        " ",
        style: this.style,
        maxLines: 1,
        overflow: TextOverflow.clip,
      ),
    ),
  );
}

/// A simple colored box that we can show while we wait for some text content to
/// load.
///
/// Like [FilledTextPlaceholder] but works insids a [Text.rich] or [TextSpan].
class FilledTextPlaceholderSpan extends WidgetSpan {
  FilledTextPlaceholderSpan({
    super.style,
    Color color = LxColors.grey850,
    double width = double.infinity,
    double borderRadius = LxRadius.r200,
  }) : super(
         baseline: TextBaseline.alphabetic,
         alignment: PlaceholderAlignment.baseline,
         child: FilledTextPlaceholder(
           style: style,
           color: color,
           width: width,
           borderRadius: borderRadius,
         ),
       );
}

enum LxCloseButtonKind { closeFromTop, closeFromRoot, closeDrawer }

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
    this.isLeading = false,
  });

  final LxCloseButtonKind kind;
  final bool isLeading;

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
    final button = IconButton(
      icon: const Icon(LxIcons.close),
      onPressed: () => this.onTap(context),
    );

    if (this.isLeading) {
      return Padding(
        padding: const EdgeInsets.only(left: Space.leadingTweakLeftPadding),
        child: button,
      );
    } else {
      return button;
    }
  }
}

/// ← - Back button, usually placed on the [AppBar] to go back a page in a
/// sub-flow.
///
/// Example usage:
///
/// * Go back to the previous page In a multi-step form.
class LxBackButton extends StatelessWidget {
  const LxBackButton({super.key, this.isLeading = false});

  final bool isLeading;

  @override
  Widget build(BuildContext context) {
    final button = IconButton(
      icon: const Icon(LxIcons.back),
      onPressed: () => Navigator.of(context).pop(),
    );

    if (this.isLeading) {
      return Padding(
        padding: const EdgeInsets.only(left: Space.leadingTweakLeftPadding),
        child: button,
      );
    } else {
      return button;
    }
  }
}

/// ⟳ - Animated refresh button, usually placed on the [AppBar] to refresh the
/// page contents.
///
/// Takes a [ValueNotifier<bool>] which notifies this button of the current
/// refresh state (idle or refreshing).
//
// TODO(phlip9): I'd prefer the refresh button itself spin while we're loading,
// as it'd make a cleaner animation IMO. However, it will take too long atm.
class LxRefreshButton extends StatelessWidget {
  const LxRefreshButton({
    super.key,
    required this.isRefreshing,
    required this.triggerRefresh,
  });

  final ValueListenable<bool> isRefreshing;
  final VoidCallback triggerRefresh;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: this.isRefreshing,
    builder: (_context, isRefreshing, _child) => IconButton(
      // disable while we're refreshing.
      onPressed: (isRefreshing) ? null : this.triggerRefresh,

      // animate icon to a spinner while we're refreshing.
      icon: AnimatedSwitcher(
        duration: const Duration(milliseconds: 150),
        child: (!isRefreshing)
            ? const Icon(LxIcons.refresh)
            : const SizedBox.square(
                dimension: Fonts.size500,
                child: CircularProgressIndicator(
                  strokeWidth: 3.0,
                  color: LxColors.fgTertiary,
                ),
              ),
      ),
    ),
  );
}

/// An outlined button with an icon. Used as a secondary action button.
///
/// It's like the standard `OutlinedButton.icon`, but the text is properly
/// centered in the button and the icon is right aligned.
class LxFilledButton extends StatelessWidget {
  /// Standard white-bg, black-fg, filled button.
  const LxFilledButton({
    super.key,
    required this.onTap,
    this.label,
    this.icon,
    this.style,
  });

  /// Primary emphasis button. moneyGoUp-bg, white-fg, filled button.
  LxFilledButton.tonal({
    super.key,
    required this.onTap,
    this.label,
    this.icon,
    ButtonStyle? style,
  }) : this.style = ButtonStyle(
         foregroundColor: WidgetStateProperty.resolveWith(
           (states) => (!states.contains(WidgetState.disabled))
               ? LxColors.grey1000
               : null,
         ),
         backgroundColor: WidgetStateProperty.resolveWith(
           (states) => (!states.contains(WidgetState.disabled))
               ? LxColors.moneyGoUp
               : null,
         ),
         iconColor: const WidgetStatePropertyAll(LxColors.grey1000),
       ).merge(style);

  /// High emphasis button. black-bg, white-fg, filled button.
  LxFilledButton.strong({
    super.key,
    required this.onTap,
    this.label,
    this.icon,
    ButtonStyle? style,
  }) : this.style = ButtonStyle(
         foregroundColor: WidgetStateProperty.resolveWith(
           (states) => (!states.contains(WidgetState.disabled))
               ? LxColors.background
               : null,
         ),
         backgroundColor: WidgetStateProperty.resolveWith(
           (states) => (!states.contains(WidgetState.disabled))
               ? LxColors.foreground
               : null,
         ),
         iconColor: const WidgetStatePropertyAll(LxColors.grey1000),
         overlayColor: const WidgetStatePropertyAll(LxColors.clearW200),
       ).merge(style);

  final Widget? label;
  final Widget? icon;
  final VoidCallback? onTap;
  final ButtonStyle? style;

  @override
  Widget build(BuildContext context) {
    return FilledButton(
      onPressed: this.onTap,
      style: this.style,
      child: ButtonChild(label: this.label, icon: this.icon),
    );
  }
}

/// An outlined button with an icon. Used as a secondary action button.
///
/// It's like the standard `OutlinedButton.icon`, but the text is properly
/// centered in the button and the icon is right aligned.
class LxOutlinedButton extends StatelessWidget {
  const LxOutlinedButton({
    super.key,
    required this.onTap,
    this.label,
    this.icon,
    this.style,
  });

  final Widget? label;
  final Widget? icon;
  final VoidCallback? onTap;
  final ButtonStyle? style;

  @override
  Widget build(BuildContext context) {
    return OutlinedButton(
      onPressed: this.onTap,
      style: this.style,
      child: ButtonChild(label: this.label, icon: this.icon),
    );
  }
}

class ButtonChild extends StatelessWidget {
  const ButtonChild({super.key, this.label, this.icon})
    : assert(label != null || icon != null);

  final Widget? label;
  final Widget? icon;

  @override
  Widget build(BuildContext context) {
    final label = this.label;
    final icon = this.icon;

    if (label == null) {
      return icon!;
    }

    if (icon == null) {
      return label;
    }

    return Stack(
      alignment: Alignment.center,
      children: [
        label,
        Align(alignment: Alignment.centerRight, child: icon),
      ],
    );
  }
}

/// Heading/title text that sits directly beneath the AppBar.
class HeadingText extends StatelessWidget {
  const HeadingText({super.key, required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: Space.s400, bottom: Space.s200),
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
        height: 1.2,
      ),
    );
  }
}

/// A single row showing the user's lightning / channel balance or on-chain
/// balance. Ex: two are beneath the unified balance on the main wallet screen.
class SubBalanceRow extends ItemizedAmountRow {
  factory SubBalanceRow({
    Key? key,
    required BalanceKind kind,
    required BalanceState balance,
  }) {
    return SubBalanceRow._(
      key: key,
      fiatAmount: balance.byKindFiat(kind),
      satsAmount: balance.byKindSats(kind),
      title: switch (kind) {
        BalanceKind.onchain => "On-chain",
        BalanceKind.lightning => "Lightning",
      },
      subtitle: "BTC",
      icon: ListIcon.byBalanceKind(kind),
    );
  }

  const SubBalanceRow._({
    required super.key,
    required super.fiatAmount,
    required super.satsAmount,
    required super.title,
    required super.subtitle,
    required super.icon,
  });
}

class ItemizedAmountRow extends StatelessWidget {
  const ItemizedAmountRow({
    super.key,
    required this.fiatAmount,
    required this.satsAmount,
    required this.title,
    required this.subtitle,
    required this.icon,
  });

  final FiatAmount? fiatAmount;
  final int? satsAmount;

  final String title;
  final String subtitle;
  final Widget icon;

  @override
  Widget build(BuildContext context) {
    final fiatAmount = this.fiatAmount;
    final satsAmount = this.satsAmount;

    const satsSize = Fonts.size200;
    final satsStyle = Fonts.fontUI.copyWith(
      color: LxColors.grey700,
      fontSize: satsSize,
      fontVariations: [Fonts.weightMedium],
      fontFeatures: [Fonts.featTabularNumbers],
      letterSpacing: -0.25,
    );
    final satsOrPlaceholder = (satsAmount != null)
        ? Text(currency_format.formatSatsAmount(satsAmount), style: satsStyle)
        : FilledTextPlaceholder(width: Space.s800, style: satsStyle);

    const fiatSize = Fonts.size300;
    final fiatStyle = Fonts.fontUI.copyWith(
      color: LxColors.foreground,
      fontSize: fiatSize,
      fontVariations: [Fonts.weightMedium],
      fontFeatures: [Fonts.featTabularNumbers],
      letterSpacing: -0.25,
    );
    final fiatOrPlaceholder = (fiatAmount != null)
        ? SplitAmountText(
            amount: fiatAmount.amount,
            fiatName: fiatAmount.fiat,
            style: Fonts.fontUI.copyWith(
              color: LxColors.foreground,
              fontSize: fiatSize,
              fontVariations: [Fonts.weightMedium],
              fontFeatures: [Fonts.featTabularNumbers],
              letterSpacing: -0.25,
            ),
          )
        : FilledTextPlaceholder(width: Space.s900, style: fiatStyle);

    return ListTile(
      // list tile styling
      contentPadding: const EdgeInsets.symmetric(
        horizontal: Space.s0,
        vertical: Space.s0,
      ),
      horizontalTitleGap: Space.s200,
      minTileHeight: Space.s700,

      visualDensity: VisualDensity.standard,
      dense: true,

      // actual content
      leading: this.icon,

      // NOTE: we use a Row() in `title` and `subtitle` instead of `trailing` so
      // that the text baselines align properly.
      title: Padding(
        padding: const EdgeInsets.only(bottom: Space.s100),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.start,
          crossAxisAlignment: CrossAxisAlignment.baseline,
          textBaseline: TextBaseline.alphabetic,
          children: [
            Expanded(
              child: Text(
                this.title,
                style: Fonts.fontUI.copyWith(
                  fontSize: fiatSize,
                  color: LxColors.foreground,
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.only(left: Space.s200),
              child: fiatOrPlaceholder,
            ),
          ],
        ),
      ),

      subtitle: Row(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.baseline,
        textBaseline: TextBaseline.alphabetic,
        children: [
          Expanded(
            child: Text(
              this.subtitle,
              style: Fonts.fontUI.copyWith(
                fontSize: satsSize,
                color: LxColors.fgTertiary,
              ),
            ),
          ),
          Padding(
            padding: const EdgeInsets.only(left: Space.s200),
            child: satsOrPlaceholder,
          ),
        ],
      ),
    );
  }
}

/// A small Text-like widget that formats and displays a fiat amount so that
/// the fractional part is de-emphasized.
///
/// Ex: amount = 1234.56, fiatName = "USD"
///
/// display:     $1,234.56      (+): emphasized
///              ++++++---      (-): de-emphasized
class SplitAmountText extends StatelessWidget {
  const SplitAmountText({
    super.key,
    required this.amount,
    required this.fiatName,
    this.style,
    this.styleFract,
    this.textAlign,
    this.locale,
  });

  final double amount;
  final String fiatName;

  /// The base font style.
  final TextStyle? style;

  /// Styling for the deemphasized fractional part, applied on top of [style].
  final TextStyle? styleFract;

  final TextAlign? textAlign;

  /// Used for debugging
  final String? locale;

  @override
  Widget build(BuildContext context) {
    // ex: 1234.56 -> "$1,234.56" (locale dependent) -> ("$1,234", ".56")
    final (amountWhole, amountFract) = currency_format.formatFiatParts(
      this.amount,
      this.fiatName,
      locale: this.locale,
    );

    final TextStyle styleFract =
        this.styleFract ?? const TextStyle(color: LxColors.fgTertiary);

    return Text.rich(
      TextSpan(
        children: <TextSpan>[
          TextSpan(text: amountWhole),
          TextSpan(text: amountFract, style: styleFract),
        ],
        style: this.style,
      ),
      maxLines: 1,
      textAlign: this.textAlign,
    );
  }
}

class PaymentAmountInput extends StatelessWidget {
  const PaymentAmountInput({
    super.key,
    required this.fieldKey,
    required this.intInputFormatter,
    required this.allowEmpty,
    required this.allowZero,
    this.validate,
    this.onEditingComplete,
    this.initialValue,
  });

  final GlobalKey<FormFieldState<String>> fieldKey;

  final IntInputFormatter intInputFormatter;

  /// If true, `.validate()` will allow an empty field value (`null`).
  final bool allowEmpty;

  /// If true, `.validate()` will allow a zero field value (`0`).
  final bool allowZero;

  /// Additional validation to perform on the value. We already validate that
  /// the value is a non-zero unsigned integer. Return `Err(null)` to prevent
  /// submission without displaying an error bar.
  final Result<(), String> Function(int amount)? validate;

  final VoidCallback? onEditingComplete;

  final int? initialValue;

  Result<(), String> validateAmountStr(String? maybeAmountStr) {
    if (maybeAmountStr == null || maybeAmountStr.isEmpty) {
      if (this.allowEmpty) {
        return const Ok(());
      } else {
        return const Err("");
      }
    }

    final int amount;
    switch (this.intInputFormatter.tryParse(maybeAmountStr)) {
      case Ok(:final ok):
        amount = ok;
      case Err():
        return const Err("Amount must be a number.");
    }

    if (!this.allowZero && amount == 0) {
      return const Err("");
    }

    if (amount < 0) {
      return const Err("");
    }

    final validate = this.validate;
    return (validate != null) ? validate(amount) : const Ok(());
  }

  @override
  Widget build(BuildContext context) {
    final int? initialValue = this.initialValue;

    // Check locale-specific positioning by formatting a dummy amount
    // Call formatSatsAmount on a dummy value using currently active locale
    final formattedTest = currency_format.formatSatsAmount(
      69,
      bitcoinSymbol: true,
    );

    // Determine where the ₿ symbol appears based on locale formatting.
    // If the formatted test doesn't clearly show the symbol at start or end
    // (e.g., due to unexpected leading/trailing characters), we default to
    // showing it as a prefix to ensure the symbol is always visible.
    final showSuffix = formattedTest.endsWith("₿");
    final showPrefix = !showSuffix;

    // Common text style for amount input and bitcoin symbol
    final amountTextStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size800,
      fontVariations: [Fonts.weightMedium],
      letterSpacing: -0.5,
    );

    // "₿ <amount>" or "<amount> ₿" depending on locale
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      mainAxisSize: MainAxisSize.min,
      children: [
        // Left bitcoin symbol (only if locale uses prefix)
        if (showPrefix)
          Text("₿ ", style: amountTextStyle.copyWith(color: LxColors.grey700)),
        // The text field with intrinsic width
        IntrinsicWidth(
          child: TextFormField(
            key: this.fieldKey,
            autofocus: true,
            keyboardType: const TextInputType.numberWithOptions(
              signed: false,
              decimal: false,
            ),
            initialValue: (initialValue != null)
                ? this.intInputFormatter.formatInt(initialValue)
                : "0",
            textDirection: TextDirection.ltr,
            textInputAction: TextInputAction.next,
            textAlign: TextAlign.left,
            onEditingComplete: this.onEditingComplete,
            validator: (str) => this.validateAmountStr(str).err,
            decoration: baseInputDecoration.copyWith(
              hintText: "0",
              // Remove default padding to make it more compact
              contentPadding: EdgeInsets.zero,
              // Ensure there's no collapse of the field when empty
              constraints: const BoxConstraints(minWidth: Space.s700),
            ),
            inputFormatters: [this.intInputFormatter],
            style: amountTextStyle,
          ),
        ),
        // Right bitcoin symbol (only if locale uses suffix)
        if (showSuffix)
          Text(" ₿", style: amountTextStyle.copyWith(color: LxColors.grey700)),
      ],
    );
  }
}

/// Text entry field for a user to set a payment's note or description.
class PaymentNoteInput extends StatelessWidget {
  const PaymentNoteInput({
    super.key,
    required this.fieldKey,
    required this.onSubmit,
    this.initialNote,
    this.hintText = "Optional note (visible to you only)",
    this.isEnabled = true,
  });

  final GlobalKey<FormFieldState<String>> fieldKey;
  final VoidCallback onSubmit;
  final String? initialNote;
  final String hintText;
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

      decoration: InputDecoration(
        hintStyle: const TextStyle(color: LxColors.grey550),
        hintText: this.hintText,
        counterStyle: const TextStyle(color: LxColors.grey550),
        border: const OutlineInputBorder(),
        enabledBorder: const OutlineInputBorder(
          borderSide: BorderSide(color: LxColors.fgTertiary),
        ),
        focusedBorder: const OutlineInputBorder(
          borderSide: BorderSide(color: LxColors.foreground),
        ),
      ),

      // Only show "XX/YY" character limit counter when text area is focused.
      buildCounter:
          (
            context, {
            required int currentLength,
            required int? maxLength,
            required bool isFocused,
          }) => (isFocused && maxLength != null)
          ? Text(
              "$currentLength/$maxLength",
              style: const TextStyle(
                fontSize: Fonts.size100,
                color: LxColors.grey550,
                height: 1.0,
              ),
            )
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

typedef StateStreamWidgetBuilder<T> =
    Widget Function(BuildContext context, T data);

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

typedef ValueStreamWidgetBuilder<T> =
    Widget Function(BuildContext context, T? data);

/// A small helper [Widget] that builds a new widget every time a [ValueStream]
/// gets an update.
///
/// The main difference with [StateStreamBuilder] is that the stream value might
/// be [null] if there was an error.
class ValueStreamBuilder<T> extends StreamBuilder<T> {
  ValueStreamBuilder({
    super.key,
    required ValueStream<T> stream,
    required ValueStreamWidgetBuilder<T> builder,
  }) : super(
         stream: stream,
         initialData: stream.value,
         builder: (BuildContext context, AsyncSnapshot<T> snapshot) =>
             builder(context, snapshot.data),
       );
}

/// The receipt-style separator on various confirm pages.
class ReceiptSeparator extends SizedBox {
  const ReceiptSeparator({super.key})
    : super(
        height: Space.s600,
        child: const ZigZag(
          color: LxColors.grey750,
          zigWidth: 14.0,
          strokeWidth: 1.0,
        ),
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

/// A channel balance bar graphic. Effectively a [ProgressIndicator], but not
/// animated and avoids display artifacts at the extremes (near zero value and
/// small bar width).
class ChannelBalanceBar extends StatelessWidget {
  const ChannelBalanceBar({
    super.key,
    required this.color,
    required this.backgroundColor,
    required this.value,
    this.height = Space.s300,
  });

  const ChannelBalanceBar.usable({
    super.key,
    required this.value,
    this.height = Space.s300,
  }) : color = LxColors.moneyGoUp,
       backgroundColor = LxColors.moneyGoUpSecondary;

  const ChannelBalanceBar.pending({
    super.key,
    required this.value,
    this.height = Space.s300,
  }) : color = LxColors.grey800,
       backgroundColor = LxColors.grey850;

  final Color color;
  final Color backgroundColor;
  final double value;
  final double height;

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      painter: ChannelBalanceBarPainter(
        color: this.color,
        backgroundColor: this.backgroundColor,
        value: this.value,
      ),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          // exactly `height`
          minHeight: this.height,
          maxHeight: this.height,
          // also ensure the bar isn't so small that it clips
          minWidth: this.height,
        ),
      ),
    );
  }
}

///               size
/// |---------------------------------|
///      size * value
/// |---------------------|
///  _________________________________
/// (_/_/_/_/_/_/_/_/_/_/_)___________) | height
class ChannelBalanceBarPainter extends CustomPainter {
  const ChannelBalanceBarPainter({
    super.repaint,
    required this.color,
    required this.backgroundColor,
    required this.value,
  });

  final Color color;
  final Color backgroundColor;
  final double value;

  @override
  void paint(Canvas canvas, Size size) {
    // just clamp the value in [0, 1] so it always displays properly
    final value = clampDouble(this.value, 0.0, 1.0);
    // the bar should be centered in the box
    final r = 0.5 * size.height;

    // The rounded stroke caps are drawn _past_ the line extent, so we need to
    // draw the line inside smaller bounds so the rounded caps don't get cut off

    // Draw the background bar across the whole box.
    final pathBg = Path()..moveTo(r, r);
    pathBg.lineTo(max(r, size.width - r), r);
    final paintBg = Paint()
      ..color = this.backgroundColor
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = size.height;
    canvas.drawPath(pathBg, paintBg);

    // Draw the foreground bar on-top, across the active section.
    // Note: this is technically wrong for colors with transparency, but we
    // don't need that atm, so we can keep it simple.
    final pathFg = Path()..moveTo(r, r);
    pathFg.lineTo(max(r, (size.width * value) - r), r);
    final paintFg = Paint()
      ..color = this.color
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = size.height;
    canvas.drawPath(pathFg, paintFg);
  }

  @override
  bool shouldRepaint(covariant ChannelBalanceBarPainter oldDelegate) {
    return this.color != oldDelegate.color ||
        this.backgroundColor != oldDelegate.backgroundColor ||
        this.value != oldDelegate.value;
  }
}

/// Carousel indicators + next/prev button combo
///
/// ```
/// <      * * --      >
/// ```
class CarouselIndicatorsAndButtons extends StatelessWidget {
  const CarouselIndicatorsAndButtons({
    super.key,
    required this.numPages,
    required this.selectedPageIndex,
    this.onTapPrev,
    this.onTapNext,
    this.arrowColor = LxColors.clearB400,
    this.arrowDisabledOpacity = 0.0,
    this.indicatorActiveColor = LxColors.clearB600,
    this.indicatorInactiveColor = LxColors.clearB200,
  });

  final int numPages;
  final ValueListenable<int> selectedPageIndex;

  final VoidCallback? onTapPrev;
  final VoidCallback? onTapNext;

  final Color arrowColor;
  final double arrowDisabledOpacity;
  final Color indicatorActiveColor;
  final Color indicatorInactiveColor;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        // < : prev page
        ValueListenableBuilder(
          valueListenable: this.selectedPageIndex,
          builder: (_context, idx, _child) {
            final isEnabled = idx > 0;
            return AnimatedOpacity(
              opacity: (isEnabled) ? 1.0 : this.arrowDisabledOpacity,
              duration: const Duration(milliseconds: 150),
              child: IconButton(
                onPressed: (isEnabled) ? this.onTapPrev : null,
                icon: const Icon(LxIcons.backSecondary),
                color: this.arrowColor,
                disabledColor: this.arrowColor,
              ),
            );
          },
        ),

        // page indicator
        CarouselIndicators(
          numPages: this.numPages,
          selectedPageIndex: this.selectedPageIndex,
          activeColor: this.indicatorActiveColor,
          inactiveColor: this.indicatorInactiveColor,
        ),

        // > : next page
        ValueListenableBuilder(
          valueListenable: this.selectedPageIndex,
          builder: (_context, idx, _child) {
            final isEnabled = idx < this.numPages - 1;
            return AnimatedOpacity(
              opacity: (isEnabled) ? 1.0 : this.arrowDisabledOpacity,
              duration: const Duration(milliseconds: 150),
              child: IconButton(
                onPressed: (isEnabled) ? this.onTapNext : null,
                icon: const Icon(LxIcons.nextSecondary),
                color: this.arrowColor,
                disabledColor: this.arrowColor,
              ),
            );
          },
        ),
      ],
    );
  }
}

/// Visual carousel indicator dots for displaying (1) the current selected page
/// index in a carousel, and (2) the number of pages in the carousel.
///
/// Ex: If there are 3 pages in a carousel, and we're currently on the middle
///     page, the indicators will look like:
///
///     * -- *
class CarouselIndicators extends StatelessWidget {
  const CarouselIndicators({
    super.key,
    required this.selectedPageIndex,
    required this.numPages,
    this.activeColor = LxColors.clearB600,
    this.inactiveColor = LxColors.clearB200,
  });

  final int numPages;
  final ValueListenable<int> selectedPageIndex;
  final Color activeColor;
  final Color inactiveColor;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: List<Widget>.generate(
        this.numPages,
        (index) => CarouselIndicator(
          index: index,
          selectedPageIndex: this.selectedPageIndex,
          activeColor: this.activeColor,
          inactiveColor: this.inactiveColor,
        ),
      ),
    );
  }
}

class CarouselIndicator extends StatelessWidget {
  const CarouselIndicator({
    super.key,
    required this.index,
    required this.selectedPageIndex,
    required this.activeColor,
    required this.inactiveColor,
  });

  final int index;
  final ValueListenable<int> selectedPageIndex;
  final Color activeColor;
  final Color inactiveColor;

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
              color: isActive ? this.activeColor : this.inactiveColor,
              borderRadius: const BorderRadius.all(Radius.circular(12)),
            ),
          );
        },
      ),
    );
  }
}

/// The little colored bar at the very top of a bottom sheet.
class SheetDragHandle extends StatelessWidget {
  const SheetDragHandle({super.key, this.color = LxColors.grey725});

  final Color color;

  @override
  Widget build(BuildContext context) => Center(
    child: Container(
      margin: const EdgeInsets.only(top: Space.s200),
      width: Space.s800,
      height: 4,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: this.color,
        borderRadius: BorderRadius.circular(2),
      ),
    ),
  );
}

typedef ErrorDialogBuilder<E> = Widget Function(BuildContext context, E err);

/// Show a [LoadingSpinnerModal] while an async [Future] is pending. When it
/// resolves, optionally construct an error dialog from the error and show it
/// as another modal.
///
/// Returns [null] if the user canceled (gesture/HW back) during the loading,
/// otherwise returns the [Future] output. If [barrierDismissible] is true,
/// also allow the user to cancel by tapping outside the spinner.
Future<Result<T, E>?> showModalAsyncFlow<T, E>({
  required BuildContext context,
  required Future<Result<T, E>> future,
  ErrorDialogBuilder<E>? errorBuilder,
  bool barrierDismissible = false,
}) async {
  final Result<T, E>? result = await showDialog(
    context: context,
    barrierDismissible: barrierDismissible,
    builder: (_context) => FutureBuilder(
      future: future,
      builder: (context, result) {
        if (result.hasData || result.hasError) {
          unawaited(Navigator.of(context).maybePop(result.data));
        }
        return const LoadingSpinnerModal();
      },
    ),
  );

  // Canceled
  if (!context.mounted) return null;

  // If there was an error, show an error dialog
  if (errorBuilder != null) {
    if (result case Err(:final err)) {
      final _ = await showDialog(
        context: context,
        builder: (context) => errorBuilder(context, err),
      );
      if (!context.mounted) return null;
    }
  }

  return result;
}

/// An error title and message. Used with [ErrorMessageSection].
final class ErrorMessage {
  const ErrorMessage({this.title, this.message})
    : assert(title != null || message != null);

  final String? title;
  final String? message;

  /// Concat the error title and message together, separated by a newline.
  @override
  String toString() {
    final title = this.title;
    final message = this.message;

    if (title != null && message == null) return title;
    if (title == null && message != null) return message;

    return "$title\n$message";
  }

  @override
  int get hashCode => this.title.hashCode ^ this.message.hashCode;
  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is ErrorMessage &&
            (identical(other.title, this.title) || other.title == this.title) &&
            (identical(other.message, this.message) ||
                other.message == this.message));
  }
}

/// A white card that fades-in error details when the [errorMessage] is set.
///
/// The card is interactive:
/// 1. If the error message is too long, we'll truncate it but allow the user
///    to tap-to-expand.
/// 2. The user can also long-press the card to copy the full error message to
///    their clipboard.
// TODO(phlip9): handle structured errors
// TODO(phlip9): slide up/down animation
class ErrorMessageSection extends StatefulWidget {
  const ErrorMessageSection(this.errorMessage, {super.key, this.other});

  /// The error message to display, or `null` if there's no error.
  final ErrorMessage? errorMessage;

  /// An optional widget to display when there's no error.
  final Widget? other;

  @override
  State<ErrorMessageSection> createState() => _ErrorMessageSectionState();
}

class _ErrorMessageSectionState extends State<ErrorMessageSection> {
  final ValueNotifier<bool> isExpanded = ValueNotifier(false);

  @override
  void dispose() {
    this.isExpanded.dispose();
    super.dispose();
  }

  @override
  void didUpdateWidget(covariant ErrorMessageSection oldWidget) {
    super.didUpdateWidget(oldWidget);

    // Reset isExpanded when the error message gets reset.
    if (this.widget.errorMessage == null ||
        this.widget.errorMessage != oldWidget.errorMessage) {
      this.isExpanded.value = false;
    }
  }

  /// Toggle whether the error section is expanded or collapsed.
  void toggleExpanded() {
    this.isExpanded.value = !this.isExpanded.value;
  }

  /// Copy the combined error title and message to the clipboard.
  void copyToClipboard() {
    final errorMessage = this.widget.errorMessage;
    if (errorMessage == null) return;

    LxClipboard.copyTextWithFeedback(this.context, errorMessage.toString());
  }

  static const int titleMaxLines = 2;
  static const TextStyle titleStyle = TextStyle(
    // color: LxColors.errorText,
    fontVariations: [Fonts.weightMedium],
    fontSize: Fonts.size200,
    height: 1.2,
    letterSpacing: -0.2,
    overflow: TextOverflow.ellipsis,
  );

  static const int messageMaxLines = 1;
  static const messageStyle = TextStyle(
    // color: LxColors.errorText,
    fontSize: Fonts.size100,
    height: 1.3,
    letterSpacing: -0.1,
    overflow: TextOverflow.ellipsis,
  );

  static bool _needsExpandable({
    required BuildContext context,
    required double maxWidth,
    required String? message,
    required String? title,
  }) {
    // infinite max width will never overflow
    if (maxWidth.isInfinite) return false;

    // check if the error message body overflows its maxLines
    if (message != null) {
      if (_doesTextOverflow(
        context: context,
        maxWidth: maxWidth,
        style: messageStyle,
        maxLines: messageMaxLines,
        text: message,
      )) {
        return true;
      }
    }

    // check if the error title overflows its maxLines
    if (title != null) {
      if (_doesTextOverflow(
        context: context,
        maxWidth: maxWidth,
        style: titleStyle,
        maxLines: titleMaxLines,
        text: title,
      )) {
        return true;
      }
    }

    return false;
  }

  static bool _doesTextOverflow({
    required BuildContext context,
    required double maxWidth,
    required TextStyle style,
    required int maxLines,
    required String text,
  }) {
    // Fast check. Doesn't handle long lines that overflow the container
    if (text.countLines() > maxLines) return true;

    // Layout text with style at this `context`
    final TextPainter textPainter = TextPainter(
      text: TextSpan(
        text: text,
        style: DefaultTextStyle.of(context).style.merge(style),
      ),
      maxLines: maxLines,
      textAlign: TextAlign.start,
      textDirection: TextDirection.ltr,
      textScaler: MediaQuery.textScalerOf(context),
    );
    textPainter.layout(maxWidth: maxWidth);
    final didExceedMaxLines = textPainter.didExceedMaxLines;

    // Clean up
    textPainter.dispose();

    return didExceedMaxLines;
  }

  @override
  Widget build(BuildContext context) {
    const double horizPad = Space.s300;
    const double vertPad = Space.s300;

    final errorMessage = this.widget.errorMessage;
    final title = errorMessage?.title;
    final message = errorMessage?.message;

    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 200),
      child: (errorMessage != null)
          // white card containing error title+message
          // 1. tap to expand
          // 2. long press to copy
          ? Card.filled(
              color: LxColors.grey1000,
              margin: EdgeInsets.zero,
              clipBehavior: Clip.hardEdge,
              child: LayoutBuilder(
                builder: (context, size) {
                  // The error message section needs to be expandable if the
                  // error title or error message body are too long and overflow
                  // the card.
                  final needsExpandable = _needsExpandable(
                    context: context,
                    // account for horizontal Padding below
                    maxWidth: (size.maxWidth - (2.0 * horizPad)).clamp(
                      0.0,
                      double.infinity,
                    ),
                    message: message,
                    title: title,
                  );

                  // Make the card interactive
                  return InkWell(
                    onTap: needsExpandable ? this.toggleExpanded : null,
                    onLongPress: this.copyToClipboard,
                    child: Padding(
                      // account for v caret icon in bottom padding
                      padding: needsExpandable
                          ? const EdgeInsets.fromLTRB(
                              horizPad,
                              vertPad,
                              horizPad,
                              Space.s100,
                            )
                          : const EdgeInsets.symmetric(
                              horizontal: horizPad,
                              vertical: vertPad,
                            ),

                      // Outer Row(Expanded(..)) forces error card to take full
                      // horizontal space
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.start,
                        children: [
                          Expanded(
                            child: Column(
                              mainAxisAlignment: MainAxisAlignment.start,
                              mainAxisSize: MainAxisSize.min,
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                // error title
                                if (title != null)
                                  Padding(
                                    padding: (message != null)
                                        ? const EdgeInsets.only(
                                            bottom: Space.s200,
                                          )
                                        : EdgeInsets.zero,
                                    child: ValueListenableBuilder(
                                      valueListenable: this.isExpanded,
                                      builder: (_context, isExpanded, _child) =>
                                          Text(
                                            title,
                                            maxLines: (needsExpandable)
                                                ? ((isExpanded)
                                                      // null doesn't work -- then it never wraps
                                                      ? 4
                                                      : titleMaxLines)
                                                : titleMaxLines,
                                            style: titleStyle,
                                          ),
                                    ),
                                  ),

                                // not expanded: first few lines of error message body
                                //     expanded: full error message body
                                if (message != null)
                                  ValueListenableBuilder(
                                    valueListenable: this.isExpanded,
                                    builder: (_context, isExpanded, _child) => Text(
                                      message,
                                      maxLines: (needsExpandable)
                                          ? ((isExpanded)
                                                // null doesn't work -- then it never wraps
                                                ? 100
                                                : messageMaxLines)
                                          : messageMaxLines,
                                      style: messageStyle,
                                    ),
                                  ),

                                // v - expand hint. only shown if actually expandable.
                                if (needsExpandable)
                                  Center(
                                    child: Padding(
                                      padding: const EdgeInsets.only(
                                        top: Space.s100,
                                      ),
                                      child: ValueListenableBuilder(
                                        valueListenable: this.isExpanded,
                                        builder:
                                            (_context, isExpanded, _child) =>
                                                Icon(
                                                  (isExpanded)
                                                      ? LxIcons.expandUpSmall
                                                      : LxIcons.expandDownSmall,
                                                  // color: LxColors.errorText,
                                                  color: LxColors.foreground,
                                                  weight: LxIcons.weightLight,
                                                ),
                                      ),
                                    ),
                                  ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                  );
                },
              ),
            )
          : this.widget.other,
    );
  }
}

class LoadingSpinnerModal extends StatelessWidget {
  const LoadingSpinnerModal({super.key});

  @override
  Widget build(BuildContext context) {
    final DialogThemeData dialogTheme = DialogTheme.of(context);

    // Ideally we would use a [Dialog] here. Too bad it mandates a 280 min
    // width, which looks ugly. So we're rolling our own...
    return Center(
      child: Material(
        type: MaterialType.card,

        // Inherit properties from the [DialogTheme]
        color: dialogTheme.backgroundColor,
        shape: dialogTheme.shape,
        elevation: dialogTheme.elevation ?? 0.0,
        shadowColor: dialogTheme.shadowColor,
        surfaceTintColor: dialogTheme.surfaceTintColor,
        textStyle: dialogTheme.contentTextStyle,

        // The actual spinner
        child: const Padding(
          padding: EdgeInsets.all(Space.s600),
          child: SizedBox.square(
            dimension: Space.s700,
            child: CircularProgressIndicator(
              strokeWidth: 5.0,
              color: LxColors.fgSecondary,
            ),
          ),
        ),
      ),
    );
  }
}

/// A filled circle with a widget inside.
class FilledCircle extends DecoratedBox {
  FilledCircle({
    super.key,
    required double size,
    required Color color,
    Widget? child,
  }) : super(
         decoration: BoxDecoration(
           color: color,
           borderRadius: BorderRadius.all(Radius.circular(0.5 * size)),
         ),
         child: SizedBox.square(dimension: size, child: child),
       );
}

/// An icon inside a filled circle.
class ListIcon extends StatelessWidget {
  const ListIcon(this.icon, {super.key, required this.background});

  const ListIcon.lightning({super.key})
    : icon = const Icon(
        LxIcons.lightning,
        size: Space.s500,
        color: LxColors.fgSecondary,
        fill: 1.0,
        weight: LxIcons.weightLight,
      ),
      background = LxColors.grey850;

  const ListIcon.bitcoin({super.key})
    : icon = const Icon(
        LxIcons.bitcoin,
        size: Space.s500,
        color: LxColors.fgSecondary,
      ),
      background = LxColors.grey850;

  factory ListIcon.byBalanceKind(BalanceKind kind) => switch (kind) {
    BalanceKind.onchain => const ListIcon.bitcoin(),
    BalanceKind.lightning => const ListIcon.lightning(),
  };

  final Widget icon;
  final Color background;

  @override
  Widget build(BuildContext context) =>
      FilledCircle(size: Space.s650, color: this.background, child: this.icon);
}

/// A [Column] of [InfoRow]s, surrounded by a white rounded card. Includes
/// optional header text above the card.
class InfoCard extends StatelessWidget {
  const InfoCard({
    super.key,
    required this.children,
    this.header,
    this.bodyPadding = Space.s300,
  });

  final String? header;
  final List<Widget> children;
  final double bodyPadding;

  @override
  Widget build(BuildContext context) {
    final section = Card(
      color: LxColors.grey1000,
      elevation: 0.0,
      margin: const EdgeInsets.all(0),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: Space.s300 / 2),
        child: Column(children: this.children),
      ),
    );

    const intraCardSpace = Space.s200;

    final header = this.header;
    if (header != null) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: intraCardSpace),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Padding(
              padding: EdgeInsets.only(
                left: this.bodyPadding,
                bottom: Space.s200,
              ),
              child: Text(
                header,
                style: const TextStyle(
                  color: LxColors.fgTertiary,
                  fontSize: Fonts.size200,
                ),
              ),
            ),
            section,
          ],
        ),
      );
    } else {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: intraCardSpace),
        child: section,
      );
    }
  }
}

/// A [Row] inside an [InfoCard].
class InfoRow extends StatelessWidget {
  const InfoRow({
    super.key,
    required this.label,
    required this.value,
    this.linkTarget,
    this.bodyPadding = Space.s300,
  });

  /// The row label
  final String label;

  /// The row display value
  final String value;

  /// If set, tapping on the row will open this link.
  final String? linkTarget;

  /// Horizontal padding between the row and the card edge.
  final double bodyPadding;

  @override
  Widget build(BuildContext context) {
    const valueStyle = TextStyle(
      color: LxColors.fgSecondary,
      fontSize: Fonts.size200,
      height: 1.2,
      fontFeatures: [Fonts.featDisambugation],
      decorationColor: LxColors.grey500,
    );
    final isMobile = Platform.isAndroid || Platform.isIOS;

    // Mobile: we'll make the text copy-on-tap
    // Desktop: we'll make the text selectable

    final linkTarget = this.linkTarget;
    final valueText = (isMobile || linkTarget != null)
        ? Text(
            this.value,
            style: (linkTarget != null)
                ? valueStyle.copyWith(decoration: TextDecoration.underline)
                : valueStyle,
          )
        : SelectableText(this.value, style: valueStyle);

    final row = Padding(
      padding: EdgeInsets.symmetric(
        horizontal: this.bodyPadding,
        vertical: Space.s300 / 2,
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Label
          ConstrainedBox(
            constraints: const BoxConstraints.tightFor(width: Space.s925),
            child: Text(
              this.label,
              style: const TextStyle(
                color: LxColors.grey550,
                fontSize: Fonts.size200,
                height: 1.2,
              ),
            ),
          ),
          const SizedBox(width: Space.s400),

          // Value
          Expanded(child: valueText),

          // Link icon (if linkTarget)
          if (linkTarget != null)
            Padding(
              padding: EdgeInsets.only(left: this.bodyPadding / 2, right: 2.0),
              child: const Icon(
                LxIcons.openLink,
                size: Fonts.size200,
                color: LxColors.fgSecondary,
              ),
            ),
        ],
      ),
    );

    /// On mobile, user taps/holds the row => copy to clipboard.
    /// HACK: we'll copy only the first line, since that's where the primary
    /// content usually is. We should really have different content types here
    /// that self-determine how they should be copied.
    void copyValue() {
      if (this.value.isEmpty || this.value == " ") return;
      final toCopy = this.value.split('\n').first;
      unawaited(LxClipboard.copyTextWithFeedback(context, toCopy));
    }

    Future<void> onTap() async {
      // Try to open link if row has one, else fallback to copy
      if (linkTarget != null) {
        final result = await url.open(linkTarget);
        if (result.ok ?? false) {
          return;
        }
      }

      copyValue();
    }

    final maybeCopyOnTapRow = (isMobile)
        ? InkWell(onTap: onTap, onLongPress: copyValue, child: row)
        : row;

    return maybeCopyOnTapRow;
  }
}

class MultiTapDetector extends StatefulWidget {
  const MultiTapDetector({
    super.key,
    required this.child,
    required this.onMultiTapDetected,
    this.tapCount = 3,
    this.timeout = const Duration(seconds: 1),
  });

  final Widget child;
  final VoidCallback onMultiTapDetected;
  final int tapCount;
  final Duration timeout;

  @override
  State<MultiTapDetector> createState() => _MultiTapDetectorState();
}

class _MultiTapDetectorState extends State<MultiTapDetector> {
  int _count = 0;
  Timer? _timer;

  void onTap() {
    this._count++;
    this._timer?.cancel();

    if (this._count == widget.tapCount) {
      this._count = 0;
      widget.onMultiTapDetected();
    } else {
      this._timer = Timer(widget.timeout, () => this._count = 0);
    }
  }

  @override
  void dispose() {
    this._timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(onTap: this.onTap, child: widget.child);
  }
}

/// A single "{idx}. {word}" line in a SeedWordCard.
class SeedWord extends StatelessWidget {
  const SeedWord({super.key, required this.index, required this.word})
    : _onRemove = null;

  const SeedWord.removable({
    super.key,
    required this.index,
    required this.word,
    required VoidCallback onRemove,
  }) : _onRemove = onRemove;

  final int index;
  final String word;
  final VoidCallback? _onRemove;

  static const _indexStyle = TextStyle(
    fontSize: Fonts.size200,
    color: LxColors.fgSecondary,
    fontFeatures: [Fonts.featTabularNumbers],
    fontVariations: [Fonts.weightLight],
  );

  static const _wordStyle = TextStyle(
    fontSize: Fonts.size200,
    fontVariations: [Fonts.weightSemiBold],
  );

  @override
  Widget build(BuildContext context) {
    // Following is an estimation of what's the max word width in our alphabet.
    // We assume that the word is a bip39 from English language. The longest word
    // is "tomorrow" with 8 chracters plus the icon. Then we assume that a chracter
    // in pixels is 0.7 times the font size.
    final calculatedMinWidth = _wordStyle.fontSize! * 9 * 0.7;
    return SizedBox(
      height: Space.s500,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.center,
        textBaseline: TextBaseline.alphabetic,
        children: [
          SizedBox(
            width: Space.s550,
            child: Text(
              "${this.index + 1}.",
              textAlign: TextAlign.right,
              style: _indexStyle,
            ),
          ),
          const SizedBox(width: Space.s300),

          ConstrainedBox(
            constraints: BoxConstraints(minWidth: calculatedMinWidth),
            child: Row(
              children: [
                Text(this.word, textAlign: TextAlign.left, style: _wordStyle),
                if (this._onRemove != null)
                  GestureDetector(
                    onTap: this._onRemove,
                    child: const Padding(
                      padding: EdgeInsets.only(left: Space.s100),
                      child: Icon(
                        LxIcons.close,
                        size: Fonts.size200,
                        color: LxColors.fgSecondary,
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// A rounded card the displays all 24 words of a seed phrase in two columns.
class SeedWordsCard extends StatelessWidget {
  const SeedWordsCard({super.key, required this.seedWords})
    : assert(seedWords.length == 24),
      _onRemove = null;

  const SeedWordsCard.removable({
    super.key,
    required this.seedWords,
    required onRemove,
  }) : _onRemove = onRemove;

  final List<String> seedWords;
  final VoidCallback? _onRemove;

  Widget _seedWord(int index) {
    final word = index < this.seedWords.length ? this.seedWords[index] : "";
    final isLast = index == this.seedWords.length - 1;
    if (isLast && _onRemove != null) {
      return SeedWord.removable(
        index: index,
        word: word,
        onRemove: this._onRemove,
      );
    }
    return SeedWord(index: index, word: word);
  }

  @override
  Widget build(BuildContext context) {
    const double spaceWordGroup = Space.s200;

    return Container(
      padding: const EdgeInsets.fromLTRB(
        // slightly less left-padding to visually center contents
        Space.s400,
        Space.s300,
        Space.s500,
        Space.s300,
      ),
      decoration: BoxDecoration(
        color: LxColors.grey1000,
        borderRadius: BorderRadius.circular(LxRadius.r300),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        mainAxisAlignment: MainAxisAlignment.spaceEvenly,
        crossAxisAlignment: CrossAxisAlignment.center,
        // Layout the words in two columns, with regular spacing between each
        // group of three words.
        children: [
          // words column 1-12
          Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.start,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              for (int i = 0; i < 3; i++) this._seedWord(i),
              const SizedBox(height: spaceWordGroup),
              for (int i = 3; i < 6; i++) this._seedWord(i),
              const SizedBox(height: spaceWordGroup),
              for (int i = 6; i < 9; i++) this._seedWord(i),
              const SizedBox(height: spaceWordGroup),
              for (int i = 9; i < 12; i++) this._seedWord(i),
            ],
          ),

          const SizedBox(width: Space.s500),

          // words column 13-24
          Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.start,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              for (int i = 12; i < 15; i++) this._seedWord(i),
              const SizedBox(height: spaceWordGroup),
              for (int i = 15; i < 18; i++) this._seedWord(i),
              const SizedBox(height: spaceWordGroup),
              for (int i = 18; i < 21; i++) this._seedWord(i),
              const SizedBox(height: spaceWordGroup),
              for (int i = 21; i < 24; i++) this._seedWord(i),
            ],
          ),
        ],
      ),
    );
  }
}
