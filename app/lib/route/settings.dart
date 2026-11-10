import 'dart:math' show min;

import 'package:app_rs_dart/ffi/api.dart' show FiatCurrency, UserSettings;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        InfoCard,
        LxBackButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/service/fiat_rates.dart' show FiatRateService;
import 'package:lexeapp/service/user_settings.dart' show UserSettingsService;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// This page lets users edit their settings, e.g. their preferred fiat
/// currency.
// TODO(nicole): Move the fiat currency picker to its own page once there are
// more settings.
class SettingsPage extends StatelessWidget {
  const SettingsPage({
    super.key,
    required this.fiatRateService,
    required this.userSettingsService,
  });

  final FiatRateService fiatRateService;
  final UserSettingsService userSettingsService;

  @override
  Widget build(BuildContext context) {
    const cardPad = Space.s300;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        padding: const EdgeInsets.symmetric(horizontal: Space.s600 - cardPad),
        bodySlivers: [
          // Exactly fill the screen, so the page never scrolls and the
          // currency list shrinks to fit above the keyboard.
          SliverFillRemaining(
            hasScrollBody: true,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Padding(
                  padding: EdgeInsets.symmetric(horizontal: cardPad),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      HeadingText(text: "Settings"),
                      SizedBox(height: Space.s400),
                      SubheadingText(text: "Fiat currency"),
                    ],
                  ),
                ),
                Expanded(
                  child: _FiatCurrencyPicker(
                    fiatRateService: this.fiatRateService,
                    userSettingsService: this.userSettingsService,
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

/// Lets the user search for and pick their preferred fiat currency. Needs a
/// bounded height, which long lists fill and scroll within.
class _FiatCurrencyPicker extends StatefulWidget {
  const _FiatCurrencyPicker({
    required this.fiatRateService,
    required this.userSettingsService,
  });

  final FiatRateService fiatRateService;
  final UserSettingsService userSettingsService;

  @override
  State<_FiatCurrencyPicker> createState() => _FiatCurrencyPickerState();
}

class _FiatCurrencyPickerState extends State<_FiatCurrencyPicker> {
  final ValueNotifier<String> query = ValueNotifier("");
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  /// The currencies in display order, fixed once they are known so that
  /// picking one doesn't reorder the list.
  List<FiatCurrency>? ordered;

  @override
  void dispose() {
    this.query.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onFiatCurrencySelected(String? fiatCurrency) async {
    if (fiatCurrency == null) return;
    // Ignore picks while an update is in flight
    if (this.widget.userSettingsService.isUpdating.value) return;

    final res = await this.widget.userSettingsService.update(
      preferredFiatCurrency: fiatCurrency,
    );
    if (!this.mounted) return;

    this.errorMessage.value = switch (res) {
      Ok() => null,
      Err(:final err) => ErrorMessage(
        title: "Failed to update fiat currency",
        message: err,
      ),
    };
  }

  @override
  Widget build(BuildContext context) {
    final fiatRateService = this.widget.fiatRateService;
    final userSettingsService = this.widget.userSettingsService;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Fiat currency search
        InfoCard(
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(
                horizontal: Space.s400,
                vertical: Space.s200,
              ),
              child: Row(
                children: [
                  const Icon(
                    LxIcons.search,
                    size: Fonts.size400,
                    color: LxColors.fgTertiary,
                  ),
                  const SizedBox(width: Space.s300),
                  Expanded(
                    child: TextField(
                      autofocus: true,
                      onChanged: (query) => this.query.value = query,
                      autocorrect: false,
                      enableSuggestions: false,
                      style: const TextStyle(fontSize: Fonts.size300),
                      decoration: const InputDecoration.collapsed(
                        hintText: "Search",
                        hintStyle: TextStyle(color: LxColors.fgTertiary),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),

        // Error message
        ValueListenableBuilder(
          valueListenable: this.errorMessage,
          builder: (_context, errorMessage, _widget) =>
              ErrorMessageSection(errorMessage),
        ),

        // Currencies matching the search. Long lists scroll within the card.
        // ~7.5 rows, so the cut-off row hints that it scrolls.
        Flexible(
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: Space.s200),
            // Not an `InfoCard`, whose children can't shrink to fit.
            child: Card(
              clipBehavior: Clip.hardEdge,
              color: LxColors.grey1000,
              elevation: 0.0,
              margin: EdgeInsets.zero,
              child: Padding(
                padding: const EdgeInsets.symmetric(vertical: Space.s300 / 2),
                child: ListenableBuilder(
                  listenable: Listenable.merge([
                    this.query,
                    fiatRateService.fiatRates,
                    userSettingsService.preferredFiatCurrency,
                  ]),
                  builder: (context, child) {
                    final query = this.query.value.trim().toLowerCase();
                    final selected =
                        userSettingsService.preferredFiatCurrency.value ??
                        UserSettings.defaultFiatCurrency();

                    final currencies = fiatRateService.supportedFiatCurrencies;
                    if (currencies != null) {
                      this.ordered ??= _orderCurrencies(currencies, selected);
                    }
                    final matches = (this.ordered ?? const [])
                        .where(
                          (currency) =>
                              currency.code.toLowerCase().contains(query) ||
                              currency.name.toLowerCase().contains(query),
                        )
                        .toList();
                    final status = (this.ordered == null)
                        ? "Loading currencies..."
                        : (matches.isEmpty)
                        ? "No matching currencies"
                        : null;
                    if (status != null) {
                      return Padding(
                        padding: const EdgeInsets.all(Space.s400),
                        child: Text(
                          status,
                          style: const TextStyle(color: LxColors.fgTertiary),
                        ),
                      );
                    }

                    return RadioGroup<String>(
                      groupValue: selected,
                      onChanged: this.onFiatCurrencySelected,
                      // Constrains the `ListView` below precisely to avoid
                      // `shrinkWrap: true`, which sizes the list by its
                      // content. Since scrolling changes which rows are laid
                      // out, the list and the page above it would re-lay out
                      // on every scroll.
                      child: ConstrainedBox(
                        constraints: BoxConstraints(
                          maxHeight: min(
                            450.0,
                            matches.length * _currencyRowHeight,
                          ),
                        ),
                        child: _ScrollEdgeFade(
                          color: LxColors.grey1000,
                          child: ListView.builder(
                            padding: EdgeInsets.zero,
                            itemExtent: _currencyRowHeight,
                            itemCount: matches.length,
                            // Each row paints its own ink; on the card's
                            // Material it would repaint the card per frame.
                            itemBuilder: (context, index) => Material(
                              type: MaterialType.transparency,
                              child: RadioListTile<String>(
                                value: matches[index].code,
                                title: Text(matches[index].code),
                                subtitle: Text(matches[index].name),
                                visualDensity: const VisualDensity(
                                  vertical: VisualDensity.minimumDensity,
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                    );
                  },
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

/// Fades each edge of a vertical scrollable [child] into [color] when there is
/// more content beyond it, hinting that the content scrolls.
class _ScrollEdgeFade extends StatefulWidget {
  const _ScrollEdgeFade({required this.color, required this.child});

  final Color color;
  final Widget child;

  @override
  State<_ScrollEdgeFade> createState() => _ScrollEdgeFadeState();
}

class _ScrollEdgeFadeState extends State<_ScrollEdgeFade> {
  /// Whether more content lies above and below the viewport.
  final ValueNotifier<({bool above, bool below})> overflow = ValueNotifier((
    above: false,
    below: false,
  ));

  @override
  void dispose() {
    this.overflow.dispose();
    super.dispose();
  }

  bool onNotification(Notification notification) {
    switch (notification) {
      case ScrollNotification(depth: 0, :final metrics) ||
          ScrollMetricsNotification(depth: 0, :final metrics):
        this.overflow.value = (
          above: metrics.extentBefore > 0.0,
          below: metrics.extentAfter > 0.0,
        );
        return false;
      default:
        return false;
    }
  }

  @override
  Widget build(BuildContext context) => NotificationListener(
    onNotification: this.onNotification,
    child: ValueListenableBuilder(
      valueListenable: this.overflow,
      builder: (context, overflow, child) => Stack(
        children: [
          child!,
          if (overflow.above)
            Positioned(
              top: 0.0,
              left: 0.0,
              right: 0.0,
              child: _EdgeFade(
                edge: Alignment.topCenter,
                color: this.widget.color,
              ),
            ),
          if (overflow.below)
            Positioned(
              bottom: 0.0,
              left: 0.0,
              right: 0.0,
              child: _EdgeFade(
                edge: Alignment.bottomCenter,
                color: this.widget.color,
              ),
            ),
        ],
      ),
      // Keep the child (and its scroll position) out of rebuilds.
      child: this.widget.child,
    ),
  );
}

/// A strip that fades from [color] at [edge] to clear.
// An overlay rather than a `ShaderMask`, which would composite the whole
// scrolling list offscreen every frame.
class _EdgeFade extends StatelessWidget {
  const _EdgeFade({required this.edge, required this.color});

  final Alignment edge;
  final Color color;

  @override
  Widget build(BuildContext context) => IgnorePointer(
    child: SizedBox(
      height: Space.s600,
      child: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: this.edge,
            end: -this.edge,
            colors: [this.color, this.color.withValues(alpha: 0.0)],
          ),
        ),
      ),
    ),
  );
}

/// [currencies] with [selected] first, then [_pinnedFiatCurrencies] in order,
/// then the rest by code.
List<FiatCurrency> _orderCurrencies(
  List<FiatCurrency> currencies,
  String selected,
) {
  int rank(FiatCurrency currency) {
    if (currency.code == selected) return 0;
    final pinned = _pinnedFiatCurrencies.indexOf(currency.code);
    return (pinned < 0) ? _pinnedFiatCurrencies.length + 1 : pinned + 1;
  }

  return List.of(currencies)..sort((a, b) {
    final byRank = rank(a).compareTo(rank(b));
    return (byRank != 0) ? byRank : a.code.compareTo(b.code);
  });
}

/// Currencies pinned to the top of the picker, below the selected one.
// The app is English-only, so most users are likely in the US, the eurozone,
// the UK, Canada, or Australia.
const List<String> _pinnedFiatCurrencies = ["USD", "EUR", "GBP", "CAD", "AUD"];

/// The height of a currency row: a [RadioListTile] with a title and a subtitle
/// at minimum density. The list is sized from it rather than measured.
const double _currencyRowHeight = 58.0;
