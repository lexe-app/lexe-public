// The primary wallet page.

import 'dart:async' show StreamController;
// import 'dart:core' show Sink;

import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show NumberFormat;
import 'package:rxdart_ext/rxdart_ext.dart';

import '../../bindings_generated_api.dart' show AppHandle;
import '../../style.dart' show Fonts, LxColors, Radius, Space;

class WalletPage extends StatelessWidget {
  const WalletPage({super.key, required this.app});

  final AppHandle app;

  Stream<int?> satsBalances(Stream<Null> refreshRx) async* {
    yield null;

    await for (final _ in refreshRx) {
      final nodeInfo = await this.app.nodeInfo();
      yield nodeInfo.localBalanceMsat;
    }
  }

  Stream<double?> fiatRates(Stream<Null> refreshRx, String fiatName) async* {
    yield null;

    await for (final _ in refreshRx) {
      final fiatRate = await this.app.fiatRate(fiat: fiatName);
      yield fiatRate.rate;
    }
  }

  @override
  Widget build(BuildContext context) {
    // A handle to refresh the wallet page contents
    final refresh = StreamController<Null>.broadcast();
    // final Sink<Null> refreshTx = refresh.sink;
    final Stream<Null> refreshRx = refresh.stream.startWith(null);

    // A stream of our current wallet balance, starting with `null` (to display
    // a placeholder before it's loaded). This stream ignores errors and ignores
    // duplicate balance values to avoid unnecessary re-layouts.
    // final satsBalanceStream = Rx.concatEager([
    //   Stream.value(null),
    //   refreshRx.asyncMap((_) async {
    //     final nodeInfo = await this.app.nodeInfo();
    //     return nodeInfo.localBalanceMsat;
    //   })
    // ]);

    // TODO(phlip9): get from user preferences
    const String fiatName = "USD";
    // final fiatRateStream = refreshRx
    //     .asyncMap((_) => this.app.fiatRate(fiat: fiatName))
    //     .map((fiatRate) => fiatRate.rate);

    final balanceStateStream = Rx.combineLatest2(
      this.satsBalances(refreshRx).debug(identifier: "satsBalances"),
      this.fiatRates(refreshRx, fiatName).debug(identifier: "fiatRates"),
      (satsBalance, fiatRate) => BalanceState(
          satsBalance: satsBalance, fiatName: fiatName, fiatRate: fiatRate),
    ).toStateStream(BalanceState.placeholder);

    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        leading: Builder(
          builder: (context) => IconButton(
            iconSize: Fonts.size700,
            icon: const Icon(Icons.menu_rounded),
            onPressed: () => Scaffold.of(context).openDrawer(),
          ),
        ),
      ),
      drawer: const WalletDrawer(),
      body: ListView(
        children: [
          const SizedBox(height: Space.s1000),
          StateStreamBuilder(
            stream: balanceStateStream,
            builder: (context, balanceState) => BalanceWidget(balanceState),
          ),
          const SizedBox(height: Space.s700),
          const WalletActions(),
        ],
      ),
      // TODO(phlip9): this default pull-to-refresh is really not great...
      // body: RefreshIndicator(
      //   backgroundColor: LxColors.background,
      //   color: LxColors.foreground,
      //   onRefresh: () async {
      //     refreshTx.add(null);
      //     await Future.delayed(const Duration(seconds: 1));
      //   },
      //   child: ListView(
      //     children: const [
      //       SizedBox(height: Space.s1000),
      //       BalanceWidget(),
      //       SizedBox(height: Space.s700),
      //       WalletActions(),
      //     ],
      //   ),
      // ),
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

class WalletDrawer extends StatelessWidget {
  const WalletDrawer({super.key});

  @override
  Widget build(BuildContext context) {
    final systemBarHeight = MediaQuery.of(context).padding.top;

    return Drawer(
      child: Padding(
        padding: EdgeInsets.only(top: systemBarHeight),
        child: ListView(
          padding: EdgeInsets.zero,
          children: [
            // X - close
            DrawerListItem(
              icon: Icons.close_rounded,
              onTap: () => Scaffold.of(context).closeDrawer(),
            ),
            const SizedBox(height: Space.s600),

            // * Settings
            // * Backup
            // * Security
            // * Support
            DrawerListItem(
              title: "Settings",
              icon: Icons.settings_outlined,
              onTap: () => debugPrint("settings pressed"),
            ),
            DrawerListItem(
              title: "Backup",
              icon: Icons.drive_file_move_outline,
              onTap: () => debugPrint("backup pressed"),
            ),
            DrawerListItem(
              title: "Security",
              icon: Icons.lock_outline_rounded,
              onTap: () => debugPrint("security pressed"),
            ),
            DrawerListItem(
              title: "Support",
              icon: Icons.help_outline_rounded,
              onTap: () => debugPrint("support pressed"),
            ),
            const SizedBox(height: Space.s600),

            // < Invite Friends >
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: Space.s500),
              child: OutlinedButton(
                style: OutlinedButton.styleFrom(
                  backgroundColor: LxColors.background,
                  foregroundColor: LxColors.foreground,
                  side:
                      const BorderSide(color: LxColors.foreground, width: 2.0),
                  padding: const EdgeInsets.symmetric(vertical: Space.s500),
                ),
                onPressed: () => debugPrint("invite pressed"),
                child: Text("Invite Friends",
                    style: Fonts.fontUI.copyWith(
                      fontSize: Fonts.size400,
                      fontVariations: [Fonts.weightMedium],
                    )),
              ),
            ),
            const SizedBox(height: Space.s600),

            // app version
            Text("Lexe App · v1.2.345",
                textAlign: TextAlign.center,
                style: Fonts.fontUI.copyWith(
                  color: LxColors.grey600,
                  fontSize: Fonts.size200,
                )),
          ],
        ),
      ),
    );
  }
}

class DrawerListItem extends StatelessWidget {
  const DrawerListItem({super.key, this.title, this.icon, this.onTap});

  final String? title;
  // final String? subtitle;
  final IconData? icon;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: Space.s500),
      horizontalTitleGap: Space.s200,
      visualDensity: VisualDensity.standard,
      dense: false,
      leading: (this.icon != null)
          ? Icon(this.icon!, color: LxColors.foreground, size: Fonts.size700)
          : null,
      title: (this.title != null)
          ? Text(this.title!,
              style: Fonts.fontUI.copyWith(
                fontSize: Fonts.size400,
                fontVariations: [Fonts.weightMedium],
              ))
          : null,
      // subtitle: (this.subtitle != null)
      //     ? Text(this.subtitle!,
      //         style: Fonts.fontUI
      //             .copyWith(fontSize: Fonts.size300, color: LxColors.grey600))
      //     : null,
      onTap: this.onTap,
    );
  }
}

final NumberFormat decimalFormatter = NumberFormat.decimalPattern();

String formatSats(int balance) => "${decimalFormatter.format(balance)} SATS";

class BalanceState {
  const BalanceState({
    required this.satsBalance,
    required this.fiatName,
    required this.fiatRate,
  });

  static BalanceState placeholder =
      const BalanceState(satsBalance: null, fiatName: "USD", fiatRate: null);

  final int? satsBalance;
  final String fiatName;
  final double? fiatRate;

  double? fiatBalance() => (this.satsBalance != null && this.fiatRate != null)
      ? this.satsBalance! * this.fiatRate!
      : null;

  @override
  String toString() => "BalanceState($satsBalance, $fiatName, $fiatRate)";
}

class BalanceWidget extends StatelessWidget {
  const BalanceWidget(this.state, {super.key});

  final BalanceState state;

  @override
  Widget build(BuildContext context) {
    debugPrint("BalanceWidget($state)");

    const satsBalanceSize = Fonts.size300;
    final satsBalanceOrPlaceholder = (this.state.satsBalance != null)
        ? Text(
            formatSats(this.state.satsBalance!),
            style: Fonts.fontUI.copyWith(
              fontSize: satsBalanceSize,
              color: LxColors.grey700,
              fontVariations: [Fonts.weightMedium],
            ),
          )
        : const FilledPlaceholder(
            width: Space.s900,
            height: satsBalanceSize,
            forText: true,
          );

    final fiatBalance = this.state.fiatBalance();
    final fiatBalanceOrPlaceholder = (fiatBalance != null)
        ? PrimaryBalanceText(
            fiatBalance: fiatBalance,
            fiatName: this.state.fiatName,
          )
        : const FilledPlaceholder(
            width: Space.s1000,
            height: Fonts.size800,
            forText: true,
          );

    return Column(
      children: [
        fiatBalanceOrPlaceholder,
        const SizedBox(height: Space.s400),
        satsBalanceOrPlaceholder,
      ],
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
    this.borderRadius = Radius.r200,
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

class PrimaryBalanceText extends StatelessWidget {
  const PrimaryBalanceText({
    super.key,
    required this.fiatBalance,
    required this.fiatName,
  });

  final double fiatBalance;
  final String fiatName;

  @override
  Widget build(BuildContext context) {
    final NumberFormat currencyFormatter = NumberFormat.simpleCurrency(
      name: this.fiatName,
    );
    final fiatBalanceStr = currencyFormatter.format(this.fiatBalance);

    final decimalSeparator = currencyFormatter.symbols.DECIMAL_SEP;
    final maybeDecimalIdx = fiatBalanceStr.lastIndexOf(decimalSeparator);

    // ex: fiatBalance = 123.45679
    //     fiatBalanceSignificant = "$123"
    //     fiatBalanceFractional = ".46"
    final String fiatBalanceSignificant;
    final String? fiatBalanceFractional;

    if (maybeDecimalIdx >= 0) {
      fiatBalanceSignificant = fiatBalanceStr.substring(0, maybeDecimalIdx);
      fiatBalanceFractional = fiatBalanceStr.substring(maybeDecimalIdx);
    } else {
      fiatBalanceSignificant = fiatBalanceStr;
      fiatBalanceFractional = null;
    }

    // debugPrint(
    //   "PrimaryBalanceText(fiatBalance: $fiatBalance, "
    //   "fiatBalanceStr: $fiatBalanceStr, decimalSep: $decimalSeparator, "
    //   "signifiant: $fiatBalanceSignificant, fract: $fiatBalanceFractional)",
    // );

    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Text(
          fiatBalanceSignificant,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size800,
            fontVariations: [Fonts.weightMedium],
          ),
        ),
        if (fiatBalanceFractional != null)
          Text(
            fiatBalanceFractional,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size800,
              color: LxColors.grey650,
              fontVariations: [Fonts.weightMedium],
            ),
          ),
      ],
    );
  }
}

class WalletActions extends StatelessWidget {
  const WalletActions({super.key});

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        const WalletActionButton(
          onPressed: null,
          icon: Icons.add_rounded,
          label: "Fund",
        ),
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: () => debugPrint("recv pressed"),
          icon: Icons.arrow_downward_rounded,
          label: "Receive",
        ),
        const SizedBox(width: Space.s400),
        WalletActionButton(
          onPressed: () => debugPrint("send pressed"),
          icon: Icons.arrow_upward_rounded,
          label: "Send",
        ),
      ],
    );
  }
}

class WalletActionButton extends StatelessWidget {
  const WalletActionButton({
    super.key,
    required this.onPressed,
    required this.icon,
    required this.label,
  });

  final VoidCallback? onPressed;
  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    final bool isDisabled = (this.onPressed == null);

    return Column(
      children: [
        FilledButton(
          onPressed: this.onPressed,
          style: FilledButton.styleFrom(
            backgroundColor: LxColors.grey975,
            disabledBackgroundColor: LxColors.grey850,
            foregroundColor: LxColors.foreground,
            disabledForegroundColor: LxColors.grey725,
          ),
          child: Padding(
            padding: const EdgeInsets.all(Space.s400),
            child: Icon(this.icon, size: Fonts.size700),
          ),
        ),
        const SizedBox(height: Space.s400),
        Text(
          label,
          style: Fonts.fontUI.copyWith(
            fontSize: Fonts.size300,
            color: (!isDisabled) ? LxColors.foreground : LxColors.grey725,
            fontVariations: [Fonts.weightSemiBold],
          ),
        ),
      ],
    );
  }
}
