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

  @override
  Widget build(BuildContext context) {
    // A handle to refresh the wallet page contents
    final refresh = StreamController<Null>();
    final Sink<Null> refreshTx = refresh.sink;
    final Stream<Null> refreshRx = refresh.stream;

    // A raw stream of `NodeInfo`s that gets updated after a refresh is
    // triggered.
    final nodeInfoStream = refreshRx
        .asyncMap((_) => this.app.nodeInfo())
        // TODO(phlip9): add connectivity notifier chip to app bar and update it
        //               here on error
        .doOnError((err, stackTrace) =>
            debugPrint("Error loading node info: $err\n$stackTrace"));

    // A stream of our current wallet balance, starting with `null` (to display
    // a placeholder before it's loaded). This stream ignores errors and ignores
    // duplicate balance values to avoid unnecessary re-layouts.
    final balanceStream = nodeInfoStream
        .map<int?>((nodeInfo) => nodeInfo.localBalanceMsat)
        .toStateStream(null);

    // Trigger an initial refresh event
    refreshTx.add(null);

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
            stream: balanceStream,
            builder: (context, maybeBalance) =>
                BalanceWidget(maybeBalance: maybeBalance),
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

class BalanceWidget extends StatelessWidget {
  const BalanceWidget({super.key, required this.maybeBalance});

  final int? maybeBalance;

  @override
  Widget build(BuildContext context) {
    debugPrint("BalanceWidget(maybeBalance: $maybeBalance)");

    const satsBalanceSize = Fonts.size300;
    final satsBalanceOrPlaceholder = (this.maybeBalance != null)
        ? Text(
            formatSats(this.maybeBalance!),
            style: Fonts.fontUI.copyWith(
              fontSize: satsBalanceSize,
              color: LxColors.grey700,
              fontVariations: [Fonts.weightMedium],
            ),
          )
        : const FilledPlaceholder(
            color: LxColors.grey825,
            width: Space.s900,
            height: satsBalanceSize,
          );

    final fiatBalanceOrPlaceholder = (this.maybeBalance != null)
        ? PrimaryBalanceText(
            fiatBalance: this.maybeBalance! * 0.0000360359,
            fiatName: "USD",
          )
        : const FilledPlaceholder(
            color: LxColors.grey825,
            width: Space.s1000,
            height: Fonts.size800,
          );

    return Column(
      children: [
        fiatBalanceOrPlaceholder,
        const SizedBox(height: Space.s400),
        satsBalanceOrPlaceholder,
        const SizedBox(height: Space.s400),
      ],
    );
  }
}

/// A simple colored box that we can show while some real content is loading.
///
/// The `width` and `height` are optional. If left `null`, that dimension will
/// be determined by the parent `Widget`'s constraints.
class FilledPlaceholder extends StatelessWidget {
  const FilledPlaceholder({
    super.key,
    this.color = LxColors.grey825,
    this.width = double.infinity,
    this.height = double.infinity,
    this.borderRadius = Radius.r200,
    this.child,
  });

  final Color color;
  final double width;
  final double height;
  final double borderRadius;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
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
