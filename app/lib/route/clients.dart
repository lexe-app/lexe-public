import 'dart:async';
import 'dart:math' show max, min;

import 'package:app_rs_dart/ffi/api.dart'
    show CreateClientRequest, CreateClientResponse, RevokeClientRequest;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show
        BitcoinOrFiat,
        BitcoinOrFiat_Bitcoin,
        BitcoinOrFiat_Fiat,
        Budget,
        BudgetInfo,
        BudgetReset,
        BudgetResetInterval,
        Network,
        RevocableClient,
        Scope;
import 'package:flutter/foundation.dart' show setEquals;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show FilteringTextInputFormatter;
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        AccessItem,
        AccessKind,
        AccessRow,
        AnimatedFillButton,
        EditableInfoRow,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        InfoCard,
        InfoRow,
        LxBackButton,
        LxCloseButton,
        LxFilledButton,
        LxOutlinedButton,
        LxRefreshButton,
        PaymentAmountInput,
        PaymentAmountInputState,
        ScrollableSinglePageBody,
        SheetDragHandle,
        SliverPullToRefresh,
        SubheadingText,
        baseInputDecoration,
        showModalAsyncFlow;
import 'package:lexeapp/currency_format.dart' as currency_format;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/date_time_ext.dart';
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/lexe_connect.dart'
    show LexeConnectFlowResult, LexeConnectScanPage;
import 'package:lexeapp/service/clients.dart' show ClientsService;
import 'package:lexeapp/string_ext.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// This page lets users add, edit, and revoke client credentials.
class ClientsPage extends StatefulWidget {
  const ClientsPage({super.key, required this.app, required this.network});

  final AppHandle app;
  final Network network;

  @override
  State<ClientsPage> createState() => _ClientsPageState();
}

class _ClientsPageState extends State<ClientsPage> {
  /// List clients on refresh.
  late final ClientsService clientsService = ClientsService(
    app: this.widget.app,
  );

  @override
  void initState() {
    super.initState();
    this.triggerRefresh();
  }

  @override
  void dispose() {
    this.clientsService.dispose();
    super.dispose();
  }

  void triggerRefresh() {
    scheduleMicrotask(this.clientsService.fetch);
  }

  Future<void> onCreatePressed() async {
    final CreateClientResponse? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) => CreateClientPage(app: this.widget.app),
          ),
        );
    if (!this.mounted || flowResult == null) return;

    // Refresh list in the background
    this.triggerRefresh();

    await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => ShowCredentialsPage(response: flowResult),
      ),
    );
  }

  Future<void> onScanPressed() async {
    final LexeConnectFlowResult? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) => LexeConnectScanPage(
              app: this.widget.app,
              network: this.widget.network,
            ),
          ),
        );
    if (!this.mounted || flowResult != LexeConnectFlowResult.approved) return;

    this.triggerRefresh();
  }

  Future<void> onRevokePressed(RevocableClient client) async {
    info("pressed revoke client (${client.pubkey})");

    final req = RevokeClientRequest(pubkey: client.pubkey);
    final fut = Result.tryFfiAsync(
      () => this.widget.app.revokeClient(req: req),
    );

    final res = await showModalAsyncFlow(
      context: this.context,
      future: fut,
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Failed to revoke client credentials"),
        content: Text(err.message),
        scrollable: true,
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text("Close"),
          ),
        ],
      ),
    );

    if (res == null || res.isOk) {
      this.triggerRefresh();
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),

        // Refresh
        actions: [
          LxRefreshButton(
            isRefreshing: this.clientsService.isFetching,
            triggerRefresh: this.triggerRefresh,
          ),
          const SizedBox(width: Space.s100),
        ],
      ),
      body: ScrollableSinglePageBody(
        bodySlivers: [
          // Pull-to-refresh
          SliverPullToRefresh(onRefresh: this.triggerRefresh),

          // Heading
          const SliverToBoxAdapter(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                HeadingText(text: "Manage client credentials"),
                SubheadingText(
                  text:
                      "Create or revoke client credentials which have control over your Lexe wallet.",
                ),
                SizedBox(height: Space.s500),
              ],
            ),
          ),

          // List body
          ValueListenableBuilder(
            valueListenable: this.clientsService.clients,
            builder: (_context, listResult, _widget) => switch (listResult) {
              // Failed to fetch clients
              Err(:final err) => SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: Space.s500),
                  child: ErrorMessageSection(
                    ErrorMessage(
                      title: "Failed to fetch client credentials",
                      message: err.message,
                    ),
                  ),
                ),
              ),
              // List of clients
              Ok(:final ok) => SliverList.builder(
                itemCount: ok.length,
                itemBuilder: (context, index) {
                  final clients = ok;
                  if (index >= clients.length) {
                    return null;
                  }

                  final client = clients[index];
                  return ClientListEntry(
                    client: client,
                    onRevokedPressed: this.onRevokePressed,
                  );
                },
              ),
            },
          ),
        ],
        // Scan and create buttons
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              SizedBox(
                width: double.infinity,
                child: LxOutlinedButton(
                  label: const Text("LexeConnect QR"),
                  icon: const Icon(LxIcons.scan),
                  onTap: this.onScanPressed,
                ),
              ),
              const SizedBox(height: Space.s300),
              LxFilledButton.strong(
                label: const Text("Create credentials"),
                icon: const Icon(LxIcons.add),
                onTap: this.onCreatePressed,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

typedef RevokeCallback = Future<void> Function(RevocableClient client);

/// A single entry in the list of clients.
class ClientListEntry extends StatelessWidget {
  const ClientListEntry({
    super.key,
    required this.client,
    required this.onRevokedPressed,
  });

  final RevocableClient client;
  final RevokeCallback onRevokedPressed;

  Future<void> _onRevokedPressed() async => this.onRevokedPressed(this.client);

  @override
  Widget build(BuildContext context) {
    final client = this.client;
    final label = client.label;
    String formatFull(int ms) => date_format.formatDateFull(
      DateTime.fromMillisecondsSinceEpoch(ms, isUtc: true),
    );
    List<String> budgetLines(BudgetInfo budget) {
      final reset = budget.reset;
      final limit = formatBudgetLimit(
        budget.limit,
        interval: reset?.interval,
        intervalCount: reset?.intervalCount ?? 1,
      );
      return [
        "budget: $limit",
        "remaining: ${formatBudgetAmount(budget.spendable)}",
        if (reset != null) "next reset: ${formatFull(reset.nextReset)}",
      ];
    }

    final expiresAtMs = client.expiresAt;

    final subtitleLines = [
      if (client.scopes.isNotEmpty)
        "scopes: ${client.scopes.map((scope) => scope.toStringId()).join(" ")}",
      if (client.permissions.isNotEmpty)
        "permissions: ${client.permissions.join(" ")}",
      if (client.budget case final budget?) ...budgetLines(budget),
      "created: ${formatFull(client.createdAt)}",
      if (expiresAtMs != null) "expires: ${formatFull(expiresAtMs)}",
      "public key: ${client.pubkey.substring(0, 12)}…",
    ];
    return ListTile(
      contentPadding: EdgeInsets.zero,
      title: Text(
        label ?? "(no label)",
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      // One `Text` per line so each line ellipsizes independently.
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final line in subtitleLines)
            Text(line, maxLines: 1, overflow: TextOverflow.ellipsis),
        ],
      ),
      trailing: IconButton(
        icon: const Icon(LxIcons.delete, weight: LxIcons.weightMedium),
        onPressed: this._onRevokedPressed,
      ),
    );
  }
}

/// The create-client flow's inputs, shared by its pages so that going back
/// and forth keeps them. Owned by [CreateClientPage], the flow's first page.
class CreateClientFlow {
  final TextEditingController labelController = TextEditingController();

  /// In ms since the UNIX epoch, or null to never expire. Defaults to one
  /// year out.
  final ValueNotifier<int?> expiresAtMs = ValueNotifier(
    DateTime.now().addMonths(12).millisecondsSinceEpoch,
  );

  final ScopePickerController scopePicker = ScopePickerController(
    ClientRole.receiver.scopes()!,
  );

  /// Whether to limit spending at all. While false, the other budget inputs
  /// are kept but unused.
  final ValueNotifier<bool> budgetLimited = ValueNotifier(true);

  /// The spending limit, once entered.
  final ValueNotifier<int?> budgetLimitMsats = ValueNotifier(null);

  /// Whether the limit resets every [budgetReset], rather than applying
  /// in total. While false, [budgetReset] is kept but unused.
  final ValueNotifier<bool> budgetResets = ValueNotifier(false);

  /// How often the limit resets, if it does.
  final ValueNotifier<BudgetReset> budgetReset = ValueNotifier(
    const BudgetReset(interval: BudgetResetInterval.monthly, intervalCount: 1),
  );

  void dispose() {
    this.labelController.dispose();
    this.expiresAtMs.dispose();
    this.scopePicker.dispose();
    this.budgetLimited.dispose();
    this.budgetLimitMsats.dispose();
    this.budgetResets.dispose();
    this.budgetReset.dispose();
  }

  /// Whether the selected scopes can spend funds.
  bool canSpend() => this.scopePicker.scopes.any((scope) => scope.canSpend());

  /// The budget to create the client with, or null for no limit. Only
  /// scopes that can spend get a budget.
  Budget? budget() {
    final limitMsats = this.budgetLimitMsats.value;
    if (!this.canSpend() || !this.budgetLimited.value || limitMsats == null) {
      return null;
    }

    final limit = BitcoinOrFiat.bitcoin(amountMsats: limitMsats);
    final reset = this.budgetResets.value ? this.budgetReset.value : null;
    return Budget(limit: limit, reset: reset);
  }
}

/// Create-client flow, page 1: collect an optional label and expiration for
/// the new client.
class CreateClientPage extends StatefulWidget {
  const CreateClientPage({super.key, required this.app});

  final AppHandle app;

  @override
  State<CreateClientPage> createState() => _CreateClientPageState();
}

class _CreateClientPageState extends State<CreateClientPage> {
  final CreateClientFlow flow = CreateClientFlow();

  @override
  void dispose() {
    this.flow.dispose();
    super.dispose();
  }

  Future<void> onNext() async {
    final CreateClientResponse? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) =>
                CreateClientScopesPage(app: this.widget.app, flow: this.flow),
          ),
        );
    if (!this.mounted || flowResult == null) return;

    // Return the response to the clients page.
    await Navigator.of(this.context).maybePop(flowResult);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Create new credentials"),
          const SubheadingText(
            text: "Add a label to remember what these credentials are for.",
          ),
          const SizedBox(height: Space.s300),

          // Label field
          TextFormField(
            controller: this.flow.labelController,
            autofocus: true,
            maxLines: 1,
            maxLength: 64,
            enableSuggestions: false,
            autocorrect: false,
            textInputAction: TextInputAction.next,
            onEditingComplete: this.onNext,
            decoration: baseInputDecoration.copyWith(
              hintText: "label",
              counterText: "",
            ),
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size700,
              fontVariations: [Fonts.weightMedium],
              letterSpacing: -0.5,
              height: 1.3,
            ),
          ),
          const SizedBox(height: Space.s400),

          // Expiration
          InfoCard(
            bodyPadding: Space.s400,
            children: [
              ExpirationRow(
                expiresAtMs: this.flow.expiresAtMs,
                bodyPadding: Space.s400,
              ),
            ],
          ),
        ],
        // Next button
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: LxFilledButton(
            label: const Text("Next"),
            icon: const Icon(LxIcons.next),
            onTap: this.onNext,
          ),
        ),
      ),
    );
  }
}

/// Create-client flow, page 2: choose the scopes to grant.
class CreateClientScopesPage extends StatefulWidget {
  const CreateClientScopesPage({
    super.key,
    required this.app,
    required this.flow,
  });

  final AppHandle app;
  final CreateClientFlow flow;

  @override
  State<CreateClientScopesPage> createState() => _CreateClientScopesPageState();
}

class _CreateClientScopesPageState extends State<CreateClientScopesPage> {
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onNext() async {
    this.errorMessage.value = null;

    if (this.widget.flow.scopePicker.scopes.isEmpty) {
      this.errorMessage.value = const ErrorMessage(
        title: "Select at least one scope",
      );
      return;
    }

    final flowResult = await CreateClientBudgetPage.enter(
      this.context,
      this.widget.app,
      this.widget.flow,
    );
    if (!this.mounted || flowResult == null) return;

    // Return the response to the previous page.
    await Navigator.of(this.context).maybePop(flowResult);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Choose credential scopes"),
          const SubheadingText(
            text:
                "Anyone holding these credentials gets the scopes you grant below.",
          ),
          const SizedBox(height: Space.s600),

          // Scope picker
          ScopePicker(controller: this.widget.flow.scopePicker),

          // Error message
          Padding(
            padding: const EdgeInsets.symmetric(vertical: Space.s400),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        // Next button
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: LxFilledButton(
            label: const Text("Next"),
            icon: const Icon(LxIcons.next),
            onTap: this.onNext,
          ),
        ),
      ),
    );
  }
}

/// Create-client flow, page 3: optionally limit what the client can spend.
class CreateClientBudgetPage extends StatefulWidget {
  const CreateClientBudgetPage({
    super.key,
    required this.app,
    required this.flow,
  });

  final AppHandle app;
  final CreateClientFlow flow;

  /// Pushes this page if the flow's scopes can spend, else the summary.
  static Future<CreateClientResponse?> enter(
    BuildContext context,
    AppHandle app,
    CreateClientFlow flow,
  ) {
    final canSpend = flow.canSpend();
    return Navigator.of(context).push(
      MaterialPageRoute(
        builder: (context) => canSpend
            ? CreateClientBudgetPage(app: app, flow: flow)
            : CreateClientSummaryPage(app: app, flow: flow),
      ),
    );
  }

  @override
  State<CreateClientBudgetPage> createState() => _CreateClientBudgetPageState();
}

class _CreateClientBudgetPageState extends State<CreateClientBudgetPage> {
  final GlobalKey<PaymentAmountInputState> amountKey = GlobalKey();
  final FocusNode amountFocus = FocusNode();

  @override
  void dispose() {
    this.amountFocus.dispose();
    super.dispose();
  }

  void onLimitedChanged(bool limited) {
    _dismissKeyboard();
    this.widget.flow.budgetLimited.value = limited;
  }

  /// Selects an interval, or "Never" for null.
  void onIntervalSelected(BudgetResetInterval? interval) {
    _dismissKeyboard();
    final flow = this.widget.flow;
    if (interval != null) {
      flow.budgetReset.value = BudgetReset(
        interval: interval,
        intervalCount: flow.budgetReset.value.intervalCount,
      );
    }
    flow.budgetResets.value = interval != null;
  }

  void onIntervalCountChanged(int intervalCount) {
    final budgetReset = this.widget.flow.budgetReset;
    budgetReset.value = BudgetReset(
      interval: budgetReset.value.interval,
      intervalCount: intervalCount,
    );
  }

  Future<void> onNext() async {
    final flow = this.widget.flow;
    if (flow.budgetLimited.value && !this.amountKey.currentState!.validate()) {
      this.amountFocus.requestFocus();
      return;
    }

    final CreateClientResponse? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) =>
                CreateClientSummaryPage(app: this.widget.app, flow: flow),
          ),
        );
    if (!this.mounted || flowResult == null) return;

    // Return the response to the previous page.
    await Navigator.of(this.context).maybePop(flowResult);
  }

  @override
  Widget build(BuildContext context) {
    final flow = this.widget.flow;
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          // Heading, and whether to limit spending at all
          Row(
            children: [
              const Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    HeadingText(text: "Set a budget"),
                    SubheadingText(text: "Fees count toward the limit."),
                  ],
                ),
              ),
              const SizedBox(width: Space.s400),
              Padding(
                padding: const EdgeInsets.only(top: Space.s300),
                child: ValueListenableBuilder(
                  valueListenable: flow.budgetLimited,
                  builder: (context, limited, _) => Semantics(
                    label: "Set a budget",
                    child: Switch(
                      value: limited,
                      onChanged: this.onLimitedChanged,
                      inactiveTrackColor: LxColors.grey1000,
                      activeTrackColor: LxColors.moneyGoUp,
                      inactiveThumbColor: LxColors.grey850,
                    ),
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: Space.s600),

          // Amount
          ValueListenableBuilder(
            valueListenable: flow.budgetLimited,
            builder: (context, limited, child) =>
                _LimitAmount(limited: limited, amount: child!),
            child: PaymentAmountInput(
              key: this.amountKey,
              allowEmpty: false,
              allowZero: false,
              compact: true,
              focusNode: this.amountFocus,
              initialMsatValue: flow.budgetLimitMsats.value,
              onMsatAmountChanged: (msats) =>
                  flow.budgetLimitMsats.value = msats,
              onEditingComplete: this.onNext,
            ),
          ),

          // Reset period
          ValueListenableBuilder(
            valueListenable: flow.budgetLimited,
            builder: (context, limited, child) =>
                _Revealable(visible: limited, child: child!),
            child: ListenableBuilder(
              listenable: Listenable.merge([
                flow.budgetResets,
                flow.budgetReset,
              ]),
              builder: (context, _) => Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Padding(
                    padding: const EdgeInsets.only(
                      top: Space.s500,
                      left: Space.s400,
                      bottom: Space.s200,
                    ),
                    child: Text(
                      "Resets",
                      style: Fonts.fontUI.copyWith(
                        fontSize: Fonts.size200,
                        color: LxColors.grey550,
                      ),
                    ),
                  ),
                  _SegmentedControl<BudgetResetInterval?>(
                    values: const [null, ...BudgetResetInterval.values],
                    selected: flow.budgetResets.value
                        ? flow.budgetReset.value.interval
                        : null,
                    label: (interval) => switch (interval) {
                      null => "Never",
                      BudgetResetInterval.daily => "Daily",
                      BudgetResetInterval.weekly => "Weekly",
                      BudgetResetInterval.monthly => "Monthly",
                      BudgetResetInterval.yearly => "Yearly",
                    },
                    onSelected: this.onIntervalSelected,
                  ),
                  _Revealable(
                    visible: flow.budgetResets.value,
                    child: Padding(
                      padding: const EdgeInsets.only(top: Space.s200),
                      child: _IntervalCountRow(
                        budgetReset: flow.budgetReset.value,
                        onChanged: this.onIntervalCountChanged,
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: Space.s600),

          // Summary
          _BudgetSummary(flow: flow),
        ],
        // Next button
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: LxFilledButton(
            label: const Text("Next"),
            icon: const Icon(LxIcons.next),
            onTap: this.onNext,
          ),
        ),
      ),
    );
  }
}

/// Create-client flow, page 4: review the new client, then create it.
class CreateClientSummaryPage extends StatefulWidget {
  const CreateClientSummaryPage({
    super.key,
    required this.app,
    required this.flow,
  });

  final AppHandle app;
  final CreateClientFlow flow;

  @override
  State<CreateClientSummaryPage> createState() =>
      _CreateClientSummaryPageState();
}

class _CreateClientSummaryPageState extends State<CreateClientSummaryPage> {
  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();
    super.dispose();
  }

  Future<void> onCreate() async {
    if (this.isPending.value) return;
    this.errorMessage.value = null;
    this.isPending.value = true;

    final flow = this.widget.flow;
    final req = CreateClientRequest(
      expiresAt: flow.expiresAtMs.value,
      label: flow.labelController.text.trim().nonEmpty(),
      scopes: flow.scopePicker.scopesList(),
      budget: flow.budget(),
    );
    final res = await Result.tryFfiAsync(
      () => this.widget.app.createClient(req: req),
    );
    if (!this.mounted) return;

    this.isPending.value = false;

    switch (res) {
      case Ok(:final ok):
        final CreateClientResponse response = ok;
        info("create-client: created: ${response.pubkey}");
        Navigator.of(this.context).pop(ok);
      case Err(:final err):
        error("create-client: error: ${err.message}");
        this.errorMessage.value = ErrorMessage(
          title: "Failed to create client credentials",
          message: err.message,
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final flow = this.widget.flow;
    final scopes = flow.scopePicker.scopesList();

    return ValueListenableBuilder(
      valueListenable: this.isPending,
      // Leaving mid-create would drop the new credentials unseen.
      builder: (context, isPending, child) =>
          PopScope(canPop: !isPending, child: child!),
      child: Scaffold(
        appBar: AppBar(
          leadingWidth: Space.appBarLeadingWidth,
          leading: const LxBackButton(isLeading: true),
        ),
        body: ScrollableSinglePageBody(
          body: [
            const HeadingText(text: "Review new credentials"),
            const SizedBox(height: Space.s200),

            AccessCard(items: [for (final scope in scopes) scope.accessItem()]),

            if (flow.canSpend()) BudgetCard(budget: flow.budget()),

            InfoCard(
              header: const Text("Details"),
              bodyPadding: Space.s400,
              children: [
                ValueListenableBuilder(
                  valueListenable: this.isPending,
                  builder: (context, isPending, _) => CredentialDetailRows(
                    expiresAtMs: flow.expiresAtMs,
                    labelController: flow.labelController,
                    enabled: !isPending,
                  ),
                ),
              ],
            ),

            // Error message
            Padding(
              padding: const EdgeInsets.only(top: Space.s400),
              child: ValueListenableBuilder(
                valueListenable: this.errorMessage,
                builder: (context, errorMessage, _) =>
                    ErrorMessageSection(errorMessage, bodyPadding: Space.s400),
              ),
            ),
          ],
          // Create button
          bottom: Padding(
            padding: const EdgeInsets.only(top: Space.s500),
            child: ValueListenableBuilder(
              valueListenable: this.isPending,
              builder: (context, isPending, _) => AnimatedFillButton(
                onTap: this.onCreate,
                loading: isPending,
                label: const Text("Create"),
                icon: const Icon(LxIcons.add),
                style: FilledButton.styleFrom(
                  backgroundColor: LxColors.foreground,
                  foregroundColor: LxColors.background,
                  iconColor: LxColors.background,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

const Duration _motionDuration = Duration(milliseconds: 280);
const Curve _motionCurve = Curves.easeOutCubic;

/// A budget reset's interval count is a nonzero `u8`.
const int _maxIntervalCount = 255;

/// The number pad has no done key, so taking another control dismisses it.
void _dismissKeyboard() => FocusManager.instance.primaryFocus?.unfocus();

/// Shows [amount], or "No limit" in its place while not [limited], rolling one
/// up and out as the other rolls in from below. [amount] stays mounted, so it
/// keeps what was typed.
class _LimitAmount extends StatefulWidget {
  const _LimitAmount({required this.limited, required this.amount});

  final bool limited;
  final Widget amount;

  @override
  State<_LimitAmount> createState() => _LimitAmountState();
}

class _LimitAmountState extends State<_LimitAmount>
    with SingleTickerProviderStateMixin {
  /// From 0 at the amount to 1 at "No limit".
  late final AnimationController roll = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 360),
    value: this.widget.limited ? 0 : 1,
  );

  late final Animation<Offset> amountSlide = this.roll.drive(
    Tween(
      begin: Offset.zero,
      end: const Offset(0, -0.7),
    ).chain(CurveTween(curve: Curves.easeInOutCubic)),
  );
  late final Animation<Offset> noLimitSlide = this.roll.drive(
    Tween(
      begin: const Offset(0, 0.7),
      end: Offset.zero,
    ).chain(CurveTween(curve: Curves.easeInOutCubic)),
  );

  /// The fades overlap for the middle fifth of the roll, so the two texts
  /// crossfade rather than one vanishing before the other appears.
  late final Animation<double> amountOpacity = this.roll.drive(
    Tween<double>(
      begin: 1,
      end: 0,
    ).chain(CurveTween(curve: const Interval(0, 0.6, curve: Curves.easeIn))),
  );
  late final Animation<double> noLimitOpacity = this.roll.drive(
    CurveTween(curve: const Interval(0.4, 1, curve: Curves.easeOut)),
  );

  @override
  void didUpdateWidget(_LimitAmount oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (this.widget.limited == oldWidget.limited) return;
    this.widget.limited ? this.roll.reverse() : this.roll.forward();
  }

  @override
  void dispose() {
    this.roll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final limited = this.widget.limited;
    return ClipRect(
      child: Stack(
        alignment: Alignment.topCenter,
        children: [
          SlideTransition(
            position: this.amountSlide,
            child: FadeTransition(
              opacity: this.amountOpacity,
              child: IgnorePointer(
                ignoring: !limited,
                child: ExcludeSemantics(
                  excluding: !limited,
                  child: ExcludeFocus(
                    excluding: !limited,
                    child: this.widget.amount,
                  ),
                ),
              ),
            ),
          ),
          Positioned.fill(
            child: IgnorePointer(
              child: ExcludeSemantics(
                excluding: limited,
                child: SlideTransition(
                  position: this.noLimitSlide,
                  child: FadeTransition(
                    opacity: this.noLimitOpacity,
                    child: Align(
                      alignment: Alignment.topCenter,
                      child: Text(
                        "No limit",
                        style: Fonts.fontUI.copyWith(
                          fontSize: Fonts.size800,
                          fontVariations: [Fonts.weightMedium],
                          letterSpacing: -0.5,
                          color: LxColors.grey650,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Shows [child] while [visible], growing and fading it in and out. It stays
/// mounted either way, but can't be reached while hidden.
class _Revealable extends StatefulWidget {
  const _Revealable({required this.visible, required this.child});

  final bool visible;
  final Widget child;

  @override
  State<_Revealable> createState() => _RevealableState();
}

class _RevealableState extends State<_Revealable>
    with SingleTickerProviderStateMixin {
  late final AnimationController controller = AnimationController(
    vsync: this,
    duration: _motionDuration,
    value: this.widget.visible ? 1 : 0,
  );
  late final Animation<double> curve = this.controller.drive(
    CurveTween(curve: _motionCurve),
  );

  @override
  void didUpdateWidget(_Revealable oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (this.widget.visible == oldWidget.visible) return;
    this.widget.visible ? this.controller.forward() : this.controller.reverse();
  }

  @override
  void dispose() {
    this.controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final hidden = !this.widget.visible;
    return SizeTransition(
      sizeFactor: this.curve,
      axisAlignment: -1,
      child: FadeTransition(
        opacity: this.curve,
        child: IgnorePointer(
          ignoring: hidden,
          child: ExcludeSemantics(
            excluding: hidden,
            child: ExcludeFocus(excluding: hidden, child: this.widget.child),
          ),
        ),
      ),
    );
  }
}

/// A pill-shaped segmented control whose white thumb slides to the selected
/// value.
class _SegmentedControl<T> extends StatelessWidget {
  const _SegmentedControl({
    required this.values,
    required this.selected,
    required this.label,
    required this.onSelected,
  });

  final List<T> values;
  final T selected;
  final String Function(T value) label;
  final ValueChanged<T> onSelected;

  /// Each segment's width: its label plus an equal share of the spare space,
  /// so the gaps between labels match. Equal widths if the labels don't fit.
  List<double> segmentWidths(BuildContext context, double width) {
    double labelWidth(T value) {
      final painter = TextPainter(
        text: TextSpan(text: this.label(value), style: _segmentLabelStyle),
        textDirection: Directionality.of(context),
        textScaler: MediaQuery.textScalerOf(context),
        maxLines: 1,
      )..layout();
      final width = painter.width;
      painter.dispose();
      return width;
    }

    final labelWidths = this.values.map(labelWidth).toList();

    final count = this.values.length;
    final spare = width - labelWidths.fold(0.0, (sum, w) => sum + w);
    if (spare < 0) return List.filled(count, width / count);
    final share = spare / count;
    return [for (final labelWidth in labelWidths) labelWidth + share];
  }

  @override
  Widget build(BuildContext context) {
    final index = this.values.indexOf(this.selected);
    return Container(
      height: 44,
      padding: const EdgeInsets.all(3),
      decoration: const ShapeDecoration(
        color: LxColors.grey850,
        shape: StadiumBorder(),
      ),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final widths = this.segmentWidths(context, constraints.maxWidth);
          final thumbLeft = widths
              .take(index)
              .fold(0.0, (sum, width) => sum + width);
          return Stack(
            children: [
              // Thumb
              AnimatedPositioned(
                left: thumbLeft,
                width: widths[index],
                top: 0,
                bottom: 0,
                duration: _motionDuration,
                curve: _motionCurve,
                child: const DecoratedBox(
                  decoration: ShapeDecoration(
                    color: LxColors.grey1000,
                    shape: StadiumBorder(),
                    shadows: [
                      BoxShadow(
                        color: LxColors.clearB50,
                        blurRadius: 4,
                        offset: Offset(0, 1),
                      ),
                    ],
                  ),
                ),
              ),

              // Segments
              Row(
                children: [
                  for (final (i, value) in this.values.indexed)
                    SizedBox(
                      width: widths[i],
                      child: Semantics(
                        button: true,
                        selected: value == this.selected,
                        inMutuallyExclusiveGroup: true,
                        child: GestureDetector(
                          behavior: HitTestBehavior.opaque,
                          onTap: () => this.onSelected(value),
                          child: Center(
                            child: AnimatedDefaultTextStyle(
                              duration: _motionDuration,
                              curve: _motionCurve,
                              style: _segmentLabelStyle.copyWith(
                                color: (value == this.selected)
                                    ? LxColors.foreground
                                    : LxColors.grey550,
                              ),
                              // Inter's tall ascender makes centered text read
                              // high, so bump it down.
                              child: Transform.translate(
                                offset: const Offset(0.0, 1.5),
                                child: FittedBox(
                                  fit: BoxFit.scaleDown,
                                  child: Text(this.label(value), maxLines: 1),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ],
          );
        },
      ),
    );
  }
}

final TextStyle _segmentLabelStyle = Fonts.fontUI.copyWith(
  fontSize: Fonts.size200,
  fontVariations: [Fonts.weightMedium],
  letterSpacing: -0.3,
);

/// "Every [N] units", with buttons to step N.
class _IntervalCountRow extends StatelessWidget {
  const _IntervalCountRow({required this.budgetReset, required this.onChanged});

  final BudgetReset budgetReset;
  final ValueChanged<int> onChanged;

  @override
  Widget build(BuildContext context) {
    final BudgetReset(:interval, :intervalCount) = this.budgetReset;
    final textStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size300,
      color: LxColors.grey450,
      letterSpacing: -0.2,
    );
    final valueStyle = textStyle.copyWith(
      color: LxColors.foreground,
      fontVariations: [Fonts.weightMedium],
    );

    void step(int intervalCount) {
      _dismissKeyboard();
      this.onChanged(intervalCount);
    }

    return Container(
      height: 48,
      padding: const EdgeInsets.only(left: Space.s400, right: 6),
      decoration: const ShapeDecoration(
        color: LxColors.grey850,
        shape: StadiumBorder(),
      ),
      child: Row(
        children: [
          Text("Every", style: textStyle),
          const SizedBox(width: Space.s200),
          _IntervalCountField(
            intervalCount: intervalCount,
            onChanged: this.onChanged,
            style: valueStyle,
          ),
          const SizedBox(width: Space.s200),
          Text(formatBudgetUnit(interval, intervalCount), style: valueStyle),
          const Spacer(),
          _StepButton(
            icon: LxIcons.remove,
            label: "Decrease",
            onPressed: (intervalCount > 1)
                ? () => step(intervalCount - 1)
                : null,
          ),
          const SizedBox(width: 6),
          _StepButton(
            icon: LxIcons.add,
            label: "Increase",
            onPressed: (intervalCount < _maxIntervalCount)
                ? () => step(intervalCount + 1)
                : null,
          ),
        ],
      ),
    );
  }
}

/// A budget reset's interval count, typed into a white pill.
class _IntervalCountField extends StatefulWidget {
  const _IntervalCountField({
    required this.intervalCount,
    required this.onChanged,
    required this.style,
  });

  final int intervalCount;
  final ValueChanged<int> onChanged;
  final TextStyle style;

  @override
  State<_IntervalCountField> createState() => _IntervalCountFieldState();
}

class _IntervalCountFieldState extends State<_IntervalCountField> {
  late final TextEditingController controller = TextEditingController(
    text: "${this.widget.intervalCount}",
  );
  final FocusNode focusNode = FocusNode();

  @override
  void initState() {
    super.initState();
    this.focusNode.addListener(this.onFocusChanged);
  }

  @override
  void didUpdateWidget(_IntervalCountField oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Show an outside count change, unless the text already parses to it.
    final count = this.widget.intervalCount;
    if (int.tryParse(this.controller.text) != count) this.setText("$count");
  }

  @override
  void dispose() {
    this.focusNode.removeListener(this.onFocusChanged);
    this.focusNode.dispose();
    this.controller.dispose();
    super.dispose();
  }

  void setText(String text) => this.controller.value = TextEditingValue(
    text: text,
    selection: TextSelection.collapsed(offset: text.length),
  );

  void onFocusChanged() {
    if (this.focusNode.hasFocus) {
      // Typing replaces the count.
      final text = this.controller.text;
      this.controller.selection = TextSelection(
        baseOffset: 0,
        extentOffset: text.length,
      );
    } else {
      // Revert an entry that wasn't applied, e.g. "" or "0".
      this.setText("${this.widget.intervalCount}");
    }
    this.setState(() {});
  }

  void onTextChanged(String text) {
    final count = int.tryParse(text);
    if (count == null || count < 1) return;
    if (count > _maxIntervalCount) this.setText("$_maxIntervalCount");
    this.widget.onChanged(min(count, _maxIntervalCount));
  }

  @override
  Widget build(BuildContext context) => AnimatedContainer(
    duration: _motionDuration,
    curve: _motionCurve,
    height: 36,
    constraints: const BoxConstraints(minWidth: 40),
    padding: const EdgeInsets.symmetric(horizontal: Space.s200),
    decoration: ShapeDecoration(
      color: LxColors.grey1000,
      shape: StadiumBorder(
        side: BorderSide(
          color: this.focusNode.hasFocus
              ? LxColors.moneyGoUp
              : LxColors.clearW0,
          width: 1.5,
        ),
      ),
    ),
    child: Center(
      widthFactor: 1,
      // The field reserves 3px after its text for the cursor (2px wide, 1px
      // gap); match it before the text so the digits center in the pill.
      child: Padding(
        padding: const EdgeInsets.only(left: 3.0),
        child: IntrinsicWidth(
          child: TextField(
            controller: this.controller,
            focusNode: this.focusNode,
            // No selection handles over the select-all on focus.
            enableInteractiveSelection: false,
            keyboardType: TextInputType.number,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            maxLength: 3,
            textAlign: TextAlign.center,
            style: this.widget.style,
            onChanged: this.onTextChanged,
            onTapOutside: (_) => this.focusNode.unfocus(),
            decoration: const InputDecoration.collapsed(
              hintText: "1",
              hintStyle: TextStyle(color: LxColors.grey750),
            ).copyWith(counterText: ""),
          ),
        ),
      ),
    ),
  );
}

/// A round white button that steps the interval count.
class _StepButton extends StatelessWidget {
  const _StepButton({
    required this.icon,
    required this.label,
    required this.onPressed,
  });

  final IconData icon;
  final String label;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: 36,
    child: IconButton(
      padding: EdgeInsets.zero,
      onPressed: this.onPressed,
      icon: Icon(this.icon, size: Fonts.size500, semanticLabel: this.label),
      style: IconButton.styleFrom(
        backgroundColor: LxColors.grey1000,
        foregroundColor: LxColors.grey350,
        disabledBackgroundColor: LxColors.grey1000,
        disabledForegroundColor: LxColors.grey750,
      ),
    ),
  );
}

/// The flow's budget as a sentence, e.g. "This credential can spend at most
/// ₿20,000 every 2 weeks."
class _BudgetSummary extends StatelessWidget {
  const _BudgetSummary({required this.flow});

  final CreateClientFlow flow;

  @override
  Widget build(BuildContext context) {
    final style = Fonts.fontUI.copyWith(
      fontSize: Fonts.size200,
      height: 1.45,
      color: LxColors.grey550,
      letterSpacing: -0.1,
    );
    final valueStyle = style.copyWith(
      color: LxColors.fgSecondary,
      fontVariations: [Fonts.weightMedium],
    );

    return ListenableBuilder(
      listenable: Listenable.merge([
        this.flow.budgetLimited,
        this.flow.budgetLimitMsats,
        this.flow.budgetResets,
        this.flow.budgetReset,
      ]),
      builder: (context, _) {
        final List<InlineSpan> spans;
        if (this.flow.budgetLimited.value) {
          final msats = this.flow.budgetLimitMsats.value ?? 0;
          final budgetReset = this.flow.budgetReset.value;
          // Non-breaking spaces keep each value on one line.
          final period = formatBudgetPeriod(
            interval: this.flow.budgetResets.value
                ? budgetReset.interval
                : null,
            intervalCount: budgetReset.intervalCount,
          ).replaceAll(" ", "\u00a0");
          spans = [
            const TextSpan(text: "This credential can spend at\u00a0most "),
            TextSpan(
              text: currency_format.formatMsatAmount(msats),
              style: (msats > 0)
                  ? valueStyle
                  : valueStyle.copyWith(color: LxColors.grey700),
            ),
            const TextSpan(text: " "),
            TextSpan(text: period, style: valueStyle),
            const TextSpan(text: "."),
          ];
        } else {
          spans = [
            const TextSpan(text: "This credential can spend "),
            TextSpan(text: "without limit", style: valueStyle),
            const TextSpan(text: "."),
          ];
        }
        return _BalancedText(TextSpan(style: style, children: spans));
      },
    );
  }
}

/// Centered rich text wrapped so its lines are as even as possible, in no
/// more lines than it needs.
class _BalancedText extends StatelessWidget {
  const _BalancedText(this.span);

  final InlineSpan span;

  /// The narrowest width at which the text still wraps into as few lines as at
  /// [maxWidth]. Each step lays it out just narrower than its longest line,
  /// which moves a word down, until that would take another line.
  double balancedWidth(BuildContext context, double maxWidth) {
    final painter = TextPainter(
      // Measure what `Text.rich` draws: the span over the default text style.
      text: TextSpan(
        style: DefaultTextStyle.of(context).style,
        children: [this.span],
      ),
      textAlign: TextAlign.center,
      textDirection: Directionality.of(context),
      textScaler: MediaQuery.textScalerOf(context),
    );
    double longestLine() => painter.computeLineMetrics().fold(
      0.0,
      (longest, line) => max(longest, line.width),
    );

    painter.layout(maxWidth: maxWidth);
    final lines = painter.computeLineMetrics().length;
    var width = longestLine();
    while (true) {
      painter.layout(maxWidth: width - 0.5);
      if (painter.computeLineMetrics().length > lines) break;
      width = longestLine();
    }
    painter.dispose();

    // Round up so the drawn text can't wrap differently from the measured.
    return width.ceilToDouble();
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) => Center(
      heightFactor: 1,
      child: SizedBox(
        width: this.balancedWidth(context, constraints.maxWidth),
        child: Text.rich(this.span, textAlign: TextAlign.center),
      ),
    ),
  );
}

/// A role dropdown and scope checkbox list.
class ScopePicker extends StatelessWidget {
  const ScopePicker({super.key, required this.controller});

  final ScopePickerController controller;

  void onReadOnlyScopesTapped(BuildContext context) {
    ScaffoldMessenger.of(context)
      ..clearSnackBars()
      ..showSnackBar(
        const SnackBar(
          duration: Duration(milliseconds: 2000),
          content: Text('To customize scopes, select "Custom" above.'),
        ),
      );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: this.controller,
      builder: (_context, _widget) {
        final role = this.controller.role;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            DropdownButtonFormField<ClientRole>(
              initialValue: role,
              decoration: const InputDecoration(labelText: "Role"),
              items: [
                for (final role in ClientRole.values)
                  DropdownMenuItem(value: role, child: Text(role.title())),
              ],
              onChanged: this.controller.selectRole,
            ),
            const SizedBox(height: Space.s400),
            if (role != ClientRole.custom)
              GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => this.onReadOnlyScopesTapped(context),
                child: ScopeCheckboxList(selected: this.controller.scopes),
              )
            else
              ScopeCheckboxList(
                selected: this.controller.scopes,
                onScopeToggled: this.controller.toggleScope,
              ),
          ],
        );
      },
    );
  }
}

/// Owns the selection state for a [ScopePicker].
class ScopePickerController extends ChangeNotifier {
  ScopePickerController(Set<Scope> initialScopes)
    : _role = ClientRole.values.firstWhere(
        (role) => setEquals(role.scopes(), initialScopes),
        orElse: () => ClientRole.custom,
      ),
      _scopes = Set.unmodifiable(initialScopes);

  ClientRole _role;
  Set<Scope> _scopes;

  ClientRole get role => this._role;

  Set<Scope> get scopes => this._scopes;

  /// Selected scopes in canonical declaration order.
  List<Scope> scopesList() => Scope.values.where(this.scopes.contains).toList();

  void selectRole(ClientRole? role) {
    if (role == null || role == this._role) return;

    this._role = role;
    final roleScopes = role.scopes();
    if (roleScopes != null) {
      this._scopes = Set.unmodifiable(roleScopes);
    }
    this.notifyListeners();
  }

  void toggleScope(Scope scope, bool selected) {
    assert(this._role == ClientRole.custom);

    final scopes = {...this._scopes};
    if (selected) {
      scopes.add(scope);
      // A selected parent supersedes its implied children.
      scopes.removeAll(scope.children());
    } else {
      scopes.remove(scope);
    }
    this._scopes = Set.unmodifiable(scopes);
    this.notifyListeners();
  }
}

typedef ScopeToggleCallback = void Function(Scope scope, bool selected);

/// A checkbox list of [Scope]s. Scopes implied by a selected parent are checked
/// and locked according to [Scope.children].
///
/// If [onScopeToggled] is null, the list is read-only and omits unselected
/// scopes.
class ScopeCheckboxList extends StatelessWidget {
  const ScopeCheckboxList({
    super.key,
    required this.selected,
    this.onScopeToggled,
  });

  final Set<Scope> selected;
  final ScopeToggleCallback? onScopeToggled;

  @override
  Widget build(BuildContext context) {
    final onScopeToggled = this.onScopeToggled;
    final editable = onScopeToggled != null;
    final locked = <Scope>{
      for (final scope in this.selected) ...scope.children(),
    };
    return Column(
      children: [
        for (final scope in Scope.values)
          if (editable ||
              this.selected.contains(scope) ||
              locked.contains(scope))
            CheckboxListTile(
              value: this.selected.contains(scope) || locked.contains(scope),
              // Keep read-only labels at full color; null `onChanged` greys out
              // only the checkboxes.
              enabled: !editable || !locked.contains(scope),
              onChanged: onScopeToggled == null
                  ? null
                  : (checked) => onScopeToggled(scope, checked!),
              title: Text(scope.title()),
              subtitle: Text(scope.description()),
              controlAffinity: ListTileControlAffinity.trailing,
              contentPadding: EdgeInsets.zero,
              visualDensity: const VisualDensity(
                vertical: VisualDensity.minimumDensity,
              ),
            ),
      ],
    );
  }
}

/// A named [Scope] bundle, or [ClientRole.custom] for individual selection.
enum ClientRole { reader, receiver, spender, admin, custom }

/// Display strings and requested scopes for each [ClientRole].
extension ClientRoleExt on ClientRole {
  String title() => switch (this) {
    ClientRole.reader => "Reader",
    ClientRole.receiver => "Receiver",
    ClientRole.spender => "Spender",
    ClientRole.admin => "Admin",
    ClientRole.custom => "Custom",
  };

  /// The scopes this role requests, or null for [ClientRole.custom].
  Set<Scope>? scopes() => switch (this) {
    ClientRole.reader => {Scope.read},
    ClientRole.receiver => {Scope.receive, ...Scope.receive.recommended()},
    ClientRole.spender => {Scope.spend, ...Scope.spend.recommended()},
    ClientRole.admin => {Scope.full},
    ClientRole.custom => null,
  };
}

/// App-side helpers for [Scope].
extension ScopeExt on Scope {
  IconData icon() => switch (this) {
    Scope.readInfo || Scope.readPayments || Scope.read => LxIcons.view,
    Scope.receive => LxIcons.receive,
    Scope.manageChannels => LxIcons.openCloseChannel,
    Scope.spend => LxIcons.send,
    Scope.full => LxIcons.key,
  };

  String title() => switch (this) {
    Scope.readInfo => "Read info",
    Scope.readPayments => "Read payments",
    Scope.read => "Read",
    Scope.receive => "Receive",
    Scope.manageChannels => "Manage channels",
    Scope.spend => "Spend",
    Scope.full => "Full access",
  };

  String description() => switch (this) {
    Scope.readInfo => "View node info, balance, and channels.",
    Scope.readPayments => "View full payment history.",
    Scope.read => "View all wallet data. Cannot spend funds.",
    Scope.receive =>
      "Create invoices, offers, and addresses to receive to, resync the "
          "node, and cancel payments.",
    Scope.manageChannels => "Open and close Lightning channels.",
    Scope.spend =>
      "Pay invoices, offers, and on-chain addresses; update payment notes.",
    Scope.full => "Full control of your wallet. Use carefully!",
  };

  /// This scope's row in an [AccessCard].
  AccessItem accessItem() => (
    icon: this.icon(),
    title: Text(this.title()),
    description: Text(this.description()),
    kind: AccessKind.scope,
  );
}

class ShowCredentialsPage extends StatefulWidget {
  const ShowCredentialsPage({super.key, required this.response});

  final CreateClientResponse response;

  @override
  State<ShowCredentialsPage> createState() => _ShowCredentialsPageState();
}

class _ShowCredentialsPageState extends State<ShowCredentialsPage> {
  Future<void> onCopyPressed() async {
    final credentials = this.widget.response.credentials;
    await LxClipboard.copyTextWithFeedback(this.context, credentials);
  }

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
        body: [
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: cardPad),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                HeadingText(text: "Save your client credentials"),
                SubheadingText(
                  text:
                      "Please save your client credentials in a safe place. You will not be able to see them again.\n\nKeep them secure, as anyone with these credentials gets the access you granted.",
                ),
                SizedBox(height: Space.s400),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.only(top: Space.s500, bottom: Space.s300),
            child: LxFilledButton(
              label: const Text("Copy"),
              onTap: this.onCopyPressed,
            ),
          ),
          InfoCard(
            children: [
              InfoRow(
                label: "Client credentials",
                value: this.widget.response.credentials,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// The "Access" card: what a credential grants, one [AccessRow] per item.
class AccessCard extends StatefulWidget {
  const AccessCard({super.key, required this.items});

  final List<AccessItem> items;

  @override
  State<AccessCard> createState() => _AccessCardState();
}

class _AccessCardState extends State<AccessCard> {
  /// Indices of the collapsed [AccessCard.items].
  final ValueNotifier<Set<int>> collapsedItems = ValueNotifier(const {});

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback(
      (_) => this.collapseItemsIfOverflowing(),
    );
  }

  @override
  void dispose() {
    this.collapsedItems.dispose();
    super.dispose();
  }

  /// Collapses the scope rows if the fully expanded page has to scroll. The
  /// API permissions row stays expanded, so they can't be skimmed past.
  void collapseItemsIfOverflowing() {
    if (!this.mounted) return;
    final position = Scrollable.maybeOf(this.context)?.position;
    if (position == null || position.maxScrollExtent <= 0) return;
    this.collapsedItems.value = {
      for (final (index, item) in this.widget.items.indexed)
        if (item.kind == AccessKind.scope) index,
    };
  }

  void toggleItem(int index) {
    final collapsed = {...this.collapsedItems.value};
    if (!collapsed.remove(index)) collapsed.add(index);
    this.collapsedItems.value = collapsed;
  }

  @override
  Widget build(BuildContext context) => InfoCard(
    header: const Text("Access"),
    bodyPadding: Space.s400,
    children: [
      ValueListenableBuilder(
        valueListenable: this.collapsedItems,
        builder: (context, collapsed, _) => Column(
          children: [
            for (final (index, item) in this.widget.items.indexed)
              AccessRow(
                item: item,
                // Error rows always show why.
                expanded:
                    item.kind == AccessKind.error || !collapsed.contains(index),
                onTap: (item.kind == AccessKind.error)
                    ? null
                    : () => this.toggleItem(index),
              ),
          ],
        ),
      ),
    ],
  );
}

/// The "Budget" card: what a credential can spend.
class BudgetCard extends StatelessWidget {
  const BudgetCard({super.key, required this.budget});

  /// Null means no limit.
  final Budget? budget;

  @override
  Widget build(BuildContext context) {
    final budget = this.budget;
    return InfoCard(
      header: const Text("Budget"),
      bodyPadding: Space.s400,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(
            horizontal: Space.s400,
            vertical: Space.s300 / 2,
          ),
          // Amount left and period right; the period wraps below a long
          // amount.
          child: SizedBox(
            width: double.infinity,
            child: Wrap(
              alignment: WrapAlignment.spaceBetween,
              crossAxisAlignment: WrapCrossAlignment.center,
              spacing: Space.s400,
              runSpacing: Space.s100,
              children: [
                Text(
                  (budget != null)
                      ? formatBudgetAmount(budget.limit)
                      : "No limit",
                  style: Fonts.fontUI.copyWith(
                    fontSize: Fonts.size300,
                    fontVariations: [Fonts.weightMedium],
                    color: LxColors.foreground,
                    height: 1.2,
                  ),
                ),
                if (budget != null)
                  Text(
                    formatBudgetPeriod(
                      interval: budget.reset?.interval,
                      intervalCount: budget.reset?.intervalCount ?? 1,
                    ),
                    style: InfoRow.labelStyle,
                  ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

/// A credential's editable expiration and label, as rows of its "Details"
/// [InfoCard].
class CredentialDetailRows extends StatefulWidget {
  const CredentialDetailRows({
    super.key,
    required this.expiresAtMs,
    this.requestedExpiresAtMs,
    required this.labelController,
    required this.enabled,
  });

  /// See [ExpirationRow.expiresAtMs].
  final ValueNotifier<int?> expiresAtMs;

  /// See [ExpirationRow.requestedMs].
  final int? requestedExpiresAtMs;

  final TextEditingController labelController;

  final bool enabled;

  @override
  State<CredentialDetailRows> createState() => _CredentialDetailRowsState();
}

class _CredentialDetailRowsState extends State<CredentialDetailRows> {
  final FocusNode labelFocusNode = FocusNode();

  @override
  void dispose() {
    this.labelFocusNode.dispose();
    super.dispose();
  }

  /// Focuses the label with the cursor at the end, rather than wherever an
  /// earlier edit through the same controller left it.
  void onLabelTap() {
    final label = this.widget.labelController;
    label.selection = TextSelection.collapsed(offset: label.text.length);
    this.labelFocusNode.requestFocus();
  }

  @override
  Widget build(BuildContext context) => Column(
    children: [
      ExpirationRow(
        expiresAtMs: this.widget.expiresAtMs,
        requestedMs: this.widget.requestedExpiresAtMs,
        enabled: this.widget.enabled,
        bodyPadding: Space.s400,
      ),
      EditableInfoRow(
        label: "Label",
        bodyPadding: Space.s400,
        onTap: this.widget.enabled ? this.onLabelTap : null,
        child: TextField(
          controller: this.widget.labelController,
          focusNode: this.labelFocusNode,
          enabled: this.widget.enabled,
          maxLines: 1,
          maxLength: 64,
          enableSuggestions: false,
          autocorrect: false,
          style: InfoRow.valueStyle,
          decoration: const InputDecoration.collapsed(
            hintText: "Optional",
            hintStyle: TextStyle(color: LxColors.grey650),
          ).copyWith(counterText: ""),
        ),
      ),
    ],
  );
}

/// An [EditableInfoRow] that picks a credential's expiration.
class ExpirationRow extends StatelessWidget {
  const ExpirationRow({
    super.key,
    required this.expiresAtMs,
    this.requestedMs,
    this.enabled = true,
    this.bodyPadding = Space.s300,
  });

  /// In ms since the UNIX epoch, or null to never expire.
  final ValueNotifier<int?> expiresAtMs;

  /// An expiration requested by a third party, if any.
  final int? requestedMs;

  final bool enabled;

  /// See [EditableInfoRow.bodyPadding].
  final double bodyPadding;

  Future<void> onTap(BuildContext context) async {
    final choice = await ExpirationChoiceSheet.show(
      context: context,
      requestedMs: this.requestedMs,
      currentMs: this.expiresAtMs.value,
    );
    if (!context.mounted || choice == null) return;
    this.expiresAtMs.value = choice.expiresAtMs;
  }

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: this.expiresAtMs,
    builder: (context, expiresAtMs, _) => EditableInfoRow(
      label: "Expires",
      onTap: this.enabled ? () => this.onTap(context) : null,
      bodyPadding: this.bodyPadding,
      child: Text(formatExpiration(expiresAtMs)),
    ),
  );
}

/// Picks a credential's expiration: the requested time, a preset, a custom
/// date, or never.
class ExpirationChoiceSheet extends StatelessWidget {
  const ExpirationChoiceSheet({
    super.key,
    required this.requestedMs,
    required this.currentMs,
  });

  /// An expiration requested by a third party, if any.
  final int? requestedMs;

  /// The currently selected expiration, or null for never.
  final int? currentMs;

  /// Show the sheet. Returns null if dismissed; otherwise the chosen
  /// `expiresAtMs`, itself null for never.
  static Future<({int? expiresAtMs})?> show({
    required BuildContext context,
    required int? requestedMs,
    required int? currentMs,
  }) => showModalBottomSheet(
    backgroundColor: LxColors.background,
    enableDrag: true,
    isScrollControlled: true,
    isDismissible: true,
    context: context,
    builder: (context) =>
        ExpirationChoiceSheet(requestedMs: requestedMs, currentMs: currentMs),
  );

  @override
  Widget build(BuildContext context) {
    final now = DateTime.now();
    final requestedMs = this.requestedMs;
    final options = <(String, int)>[
      if (requestedMs != null) ("Requested", requestedMs),
      ("1 week", now.add(const Duration(days: 7)).millisecondsSinceEpoch),
      ("1 month", now.addMonths(1).millisecondsSinceEpoch),
      ("1 year", now.addMonths(12).millisecondsSinceEpoch),
    ];

    // Presets are relative to now, so match the selection by its display.
    // Anything else is a custom date.
    final currentMs = this.currentMs;
    final currentDisplay = formatExpiration(currentMs);
    final selected = options.indexWhere(
      (option) => formatExpiration(option.$2) == currentDisplay,
    );
    final int? customDateMs = (selected == -1 && currentMs != null)
        ? currentMs
        : null;

    Future<void> onCustomDate() async {
      final firstDate = DateUtils.dateOnly(now);
      final lastDate = firstDate.addMonths(12 * 10);
      final initialDate = DateUtils.dateOnly(
        (currentMs != null)
            ? DateTime.fromMillisecondsSinceEpoch(currentMs)
            : now.addMonths(12),
      );
      final picked = await showDatePicker(
        context: context,
        // A requested expiration may fall outside the pickable range, which
        // `showDatePicker` asserts against.
        initialDate:
            (initialDate.isBefore(firstDate) || initialDate.isAfter(lastDate))
            ? null
            : initialDate,
        firstDate: firstDate,
        lastDate: lastDate,
      );
      if (!context.mounted || picked == null) return;
      // Expire at the end of the picked day.
      final endOfDay = DateTime(
        picked.year,
        picked.month,
        picked.day + 1,
      ).subtract(const Duration(seconds: 1));
      Navigator.of(context).pop((expiresAtMs: endOfDay.millisecondsSinceEpoch));
    }

    return Padding(
      padding: const EdgeInsets.only(
        left: Space.s400,
        right: Space.s400,
        bottom: Space.s600,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Center(child: SheetDragHandle()),
          const SizedBox(height: Space.s400),
          InfoCard(
            header: const Text("Expires"),
            bodyPadding: Space.s400,
            children: [
              for (final (index, (title, expiresAtMs)) in options.indexed)
                ExpirationOptionRow(
                  title: title,
                  date: formatExpiration(expiresAtMs),
                  selected: index == selected,
                  onTap: () =>
                      Navigator.of(context).pop((expiresAtMs: expiresAtMs)),
                ),
              ExpirationOptionRow(
                title: "Custom date",
                date: (customDateMs != null)
                    ? formatExpiration(customDateMs)
                    : null,
                selected: customDateMs != null,
                onTap: onCustomDate,
              ),
              ExpirationOptionRow(
                title: "Never",
                date: null,
                selected: currentMs == null,
                onTap: () => Navigator.of(context).pop((expiresAtMs: null)),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// One choice in an [ExpirationChoiceSheet].
class ExpirationOptionRow extends StatelessWidget {
  const ExpirationOptionRow({
    super.key,
    required this.title,
    required this.date,
    required this.selected,
    required this.onTap,
  });

  final String title;
  final String? date;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final date = this.date;
    return InkWell(
      onTap: this.onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(
          horizontal: Space.s400,
          vertical: Space.s300,
        ),
        child: Row(
          children: [
            Expanded(
              child: Text(
                this.title,
                style: const TextStyle(
                  fontSize: Fonts.size300,
                  color: LxColors.foreground,
                ),
              ),
            ),
            if (date != null)
              Text(
                date,
                style: const TextStyle(
                  fontSize: Fonts.size200,
                  color: LxColors.grey550,
                ),
              ),
            const SizedBox(width: Space.s300),
            SizedBox.square(
              dimension: Fonts.size400,
              child: this.selected
                  ? const Icon(
                      LxIcons.confirm,
                      size: Fonts.size400,
                      color: LxColors.moneyGoUp,
                    )
                  : null,
            ),
          ],
        ),
      ),
    );
  }
}

/// Format a credential expiration, in ms since the UNIX epoch, or null for
/// never.
String formatExpiration(int? expiresAtMs) => (expiresAtMs != null)
    ? date_format.formatDateDay(
        DateTime.fromMillisecondsSinceEpoch(expiresAtMs),
      )
    : "Never";

/// Format a budget amount, e.g. "₿100,000" or "$20.00 USD". Fiat carries its
/// code: intl prints the same symbol for many currencies, e.g. "$" for USD,
/// CAD, AUD and MXN.
String formatBudgetAmount(BitcoinOrFiat amount) => switch (amount) {
  BitcoinOrFiat_Bitcoin(:final amountMsats) => currency_format.formatMsatAmount(
    amountMsats,
  ),
  BitcoinOrFiat_Fiat(:final amount, :final currency) =>
    "${currency_format.formatFiat(double.parse(amount), currency)} $currency",
};

/// Format a budget's limit and period, e.g. "₿20,000 every month",
/// "$20.00 USD every 2 weeks", or, for a null [interval], "₿20,000 in total".
String formatBudgetLimit(
  BitcoinOrFiat limit, {
  required BudgetResetInterval? interval,
  required int intervalCount,
}) {
  final amount = formatBudgetAmount(limit);
  final period = formatBudgetPeriod(
    interval: interval,
    intervalCount: intervalCount,
  );
  return "$amount $period";
}

/// Format a budget's period, e.g. "every month", "every 2 weeks", or, for a
/// null [interval], "in total".
String formatBudgetPeriod({
  required BudgetResetInterval? interval,
  required int intervalCount,
}) {
  if (interval == null) return "in total";
  final unit = formatBudgetUnit(interval, intervalCount);
  return (intervalCount == 1) ? "every $unit" : "every $intervalCount $unit";
}

/// Format a reset interval's unit, pluralized for [count], e.g. "month" or
/// "months".
String formatBudgetUnit(BudgetResetInterval interval, int count) {
  final unit = switch (interval) {
    BudgetResetInterval.daily => "day",
    BudgetResetInterval.weekly => "week",
    BudgetResetInterval.monthly => "month",
    BudgetResetInterval.yearly => "year",
  };
  return (count == 1) ? unit : "${unit}s";
}
