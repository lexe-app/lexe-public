/// Reusable flutter UI components
library;

import 'dart:async' show unawaited;
import 'dart:math' show max;

import 'package:flutter/foundation.dart' show ValueListenable, clampDouble;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show MaxLengthEnforcement;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/input_formatter.dart'
    show IntInputFormatter, MaxUtf8BytesInputFormatter;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxBreakpoints, LxColors, LxIcons, LxRadius, Space;
import 'package:lexeapp/types.dart' show BalanceKind, BalanceState;
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
  }) :
        // can't both be non-null
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
            child: Padding(
              padding: this.bottomPadding,
              child: bottom,
            ),
          ),
        ),
    ];

    final List<Widget> slivers = (!this.useFullWidth)
        ? sliversPrePadding
            .map((sliver) => SliverPadding(
                  padding: innerPadding,
                  sliver: SliverConstrainedCrossAxis(
                    maxExtent: maxWidth,
                    sliver: sliver,
                  ),
                ))
            .toList()
        : sliversPrePadding;

    return Padding(
      padding: this.padding,
      child: CustomScrollView(
        primary: true,
        slivers: slivers,
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
                    dimension: Fonts.size400,
                    child: CircularProgressIndicator(
                      strokeWidth: 2.0,
                      color: LxColors.clearB200,
                      strokeCap: StrokeCap.round,
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
          child: button);
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
  const LxBackButton({
    super.key,
    this.isLeading = false,
  });

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
          child: button);
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
                      strokeCap: StrokeCap.round,
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
      fiatName: balance.fiatRate?.fiat,
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
    required super.fiatName,
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
    required this.fiatName,
    required this.fiatAmount,
    required this.satsAmount,
    required this.title,
    required this.subtitle,
    required this.icon,
  });

  final String? fiatName;
  final double? fiatAmount;
  final int? satsAmount;

  final String title;
  final String subtitle;
  final Widget icon;

  @override
  Widget build(BuildContext context) {
    final fiatName = this.fiatName;
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
        ? Text(
            currency_format.formatSatsAmount(satsAmount),
            style: satsStyle,
          )
        : FilledTextPlaceholder(
            width: Space.s800,
            style: satsStyle,
          );

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
            amount: fiatAmount,
            fiatName: fiatName!,
            style: Fonts.fontUI.copyWith(
              color: LxColors.foreground,
              fontSize: fiatSize,
              fontVariations: [Fonts.weightMedium],
              fontFeatures: [Fonts.featTabularNumbers],
              letterSpacing: -0.25,
            ),
          )
        : FilledTextPlaceholder(
            width: Space.s900,
            style: fiatStyle,
          );

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
            )
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
          )
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
    final (amountWhole, amountFract) = currency_format
        .formatFiatParts(this.amount, this.fiatName, locale: this.locale);

    final TextStyle styleFract =
        this.styleFract ?? const TextStyle(color: LxColors.fgTertiary);

    return Text.rich(
      TextSpan(
        children: <TextSpan>[
          TextSpan(text: amountWhole),
          TextSpan(
            text: amountFract,
            style: styleFract,
          ),
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
    this.validate,
    this.onEditingComplete,
    this.initialValue,
  });

  final GlobalKey<FormFieldState<String>> fieldKey;

  final IntInputFormatter intInputFormatter;
  final bool allowEmpty;

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

    if (amount <= 0) {
      return const Err("");
    }

    final validate = this.validate;
    return (validate != null) ? validate(amount) : const Ok(());
  }

  @override
  Widget build(BuildContext context) {
    final int? initialValue = this.initialValue;

    // <amount> sats
    return TextFormField(
      key: this.fieldKey,
      autofocus: true,
      keyboardType:
          const TextInputType.numberWithOptions(signed: false, decimal: false),
      initialValue: (initialValue != null)
          ? this.intInputFormatter.formatInt(initialValue)
          : "0",
      textDirection: TextDirection.ltr,
      textInputAction: TextInputAction.next,
      textAlign: TextAlign.right,
      onEditingComplete: this.onEditingComplete,
      validator: (str) => this.validateAmountStr(str).err,
      decoration: baseInputDecoration.copyWith(
        hintText: "0",
        // Goal: I want the amount to be right-aligned, starting from the
        //       center of the screen.
        //
        // |    vvvvvvv            |
        // |    123,456| sats      |
        // |                       |
        //
        // There's probably a better way to do this, but this works. Just
        // expand the " sats" suffix so that it takes up half the width (minus
        // some correction).
        suffix: LayoutBuilder(
          builder: (context, constraints) => ConstrainedBox(
            constraints: BoxConstraints(
              minWidth: max(0.0, (constraints.maxWidth / 2) - Space.s450),
            ),
            child: const Text(" sats"),
          ),
        ),
      ),
      inputFormatters: [this.intInputFormatter],
      style: Fonts.fontUI.copyWith(
        fontSize: Fonts.size800,
        fontVariations: [Fonts.weightMedium],
        letterSpacing: -0.5,
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

typedef ValueStreamWidgetBuilder<T> = Widget Function(
  BuildContext context,
  T? data,
);

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
          height: Space.s650,
          child: const ZigZag(
              color: LxColors.grey750, zigWidth: 14.0, strokeWidth: 1.0),
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
  })  : color = LxColors.moneyGoUp,
        backgroundColor = LxColors.moneyGoUpSecondary;

  const ChannelBalanceBar.pending({
    super.key,
    required this.value,
    this.height = Space.s300,
  })  : color = LxColors.grey800,
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
/// otherwise returns the [Future] output.
Future<Result<T, E>?> showModalAsyncFlow<T, E>({
  required BuildContext context,
  required Future<Result<T, E>> future,
  ErrorDialogBuilder<E>? errorBuilder,
}) async {
  final Result<T, E>? result = await showDialog(
    context: context,
    // Don't want loading spinner to be dismissable.
    barrierDismissible: false,
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

final class ErrorMessage {
  const ErrorMessage({this.title, this.message})
      : assert(title != null || message != null);

  final String? title;
  final String? message;
}

/// A section that fades-in error details when the [errorMessage] is set.
class ErrorMessageSection extends StatelessWidget {
  const ErrorMessageSection(this.errorMessage, {super.key});

  final ErrorMessage? errorMessage;

  @override
  Widget build(BuildContext context) {
    final errorMessage = this.errorMessage;
    final title = errorMessage?.title;
    final message = errorMessage?.message;

    // TODO(phlip9): maybe tap to expand full error message?
    // TODO(phlip9): slide up animation?

    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 200),
      child: (errorMessage != null)
          ? ListTile(
              contentPadding: EdgeInsets.zero,
              title: (title != null)
                  ? Padding(
                      padding: const EdgeInsets.only(bottom: Space.s200),
                      child: Text(
                        title,
                        style: const TextStyle(
                          color: LxColors.errorText,
                          fontVariations: [Fonts.weightMedium],
                          height: 1.15,
                        ),
                      ),
                    )
                  : null,
              subtitle: (message != null)
                  ? Text(
                      message,
                      maxLines: 3,
                      style: const TextStyle(
                        color: LxColors.errorText,
                        overflow: TextOverflow.ellipsis,
                      ),
                    )
                  : null,
            )
          : null,
    );
  }
}

class LoadingSpinnerModal extends StatelessWidget {
  const LoadingSpinnerModal({super.key});

  @override
  Widget build(BuildContext context) {
    final DialogTheme dialogTheme = DialogTheme.of(context);

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
              strokeCap: StrokeCap.round,
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
  const ListIcon(
    this.icon, {
    super.key,
    required this.background,
  });

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
  Widget build(BuildContext context) => FilledCircle(
        size: Space.s650,
        color: this.background,
        child: this.icon,
      );
}
