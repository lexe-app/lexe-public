import 'dart:async';

import 'package:app_rs_dart/ffi/api.dart'
    show CreateClientRequest, CreateClientResponse, RevokeClientRequest;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show RevocableClient, Scope;
import 'package:flutter/foundation.dart' show setEquals;
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
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
        LxRefreshButton,
        ScrollableSinglePageBody,
        SheetDragHandle,
        SliverPullToRefresh,
        SubheadingText,
        baseInputDecoration,
        showModalAsyncFlow;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/date_time_ext.dart';
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/service/clients.dart' show ClientsService;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

/// This page lets users add, edit, and revoke client credentials.
class ClientsPage extends StatefulWidget {
  const ClientsPage({super.key, required this.app});

  final AppHandle app;

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
        // Create button
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: LxFilledButton.strong(
            label: const Text("Create credentials"),
            icon: const Icon(LxIcons.add),
            onTap: this.onCreatePressed,
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
    final expiresAtMs = client.expiresAt;

    final subtitleLines = [
      if (client.scopes.isNotEmpty)
        "scopes: ${client.scopes.map((scope) => scope.toStringId()).join(" ")}",
      if (client.permissions.isNotEmpty)
        "permissions: ${client.permissions.join(" ")}",
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

/// Create-client flow, page 1: collect an optional label and expiration for
/// the new client.
class CreateClientPage extends StatefulWidget {
  const CreateClientPage({super.key, required this.app});

  final AppHandle app;

  @override
  State<CreateClientPage> createState() => _CreateClientPageState();
}

class _CreateClientPageState extends State<CreateClientPage> {
  final GlobalKey<FormFieldState<String>> labelFieldKey = GlobalKey();

  /// In ms since the UNIX epoch, or null to never expire. Defaults to one
  /// year out.
  final ValueNotifier<int?> expiresAtMs = ValueNotifier(
    DateTime.now().addMonths(12).millisecondsSinceEpoch,
  );

  @override
  void dispose() {
    this.expiresAtMs.dispose();
    super.dispose();
  }

  Future<void> onNext() async {
    final labelField = this.labelFieldKey.currentState!;
    if (!labelField.validate()) return;
    final label = labelField.value;

    final CreateClientResponse? flowResult = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) => CreateClientScopesPage(
              app: this.widget.app,
              expiresAtMs: this.expiresAtMs.value,
              label: (label != null && label.isNotEmpty) ? label : null,
            ),
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
            key: this.labelFieldKey,
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
                expiresAtMs: this.expiresAtMs,
                bodyPadding: Space.s400,
              ),
            ],
          ),
        ],
        // Next button
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: LxFilledButton.strong(
            label: const Text("Next"),
            icon: const Icon(LxIcons.next),
            onTap: this.onNext,
          ),
        ),
      ),
    );
  }
}

/// Create-client flow, page 2: choose the scopes to grant, then create
/// the client.
class CreateClientScopesPage extends StatefulWidget {
  const CreateClientScopesPage({
    super.key,
    required this.app,
    required this.expiresAtMs,
    required this.label,
  });

  final AppHandle app;

  /// In ms since the UNIX epoch, or null to never expire.
  final int? expiresAtMs;

  final String? label;

  @override
  State<CreateClientScopesPage> createState() => _CreateClientScopesPageState();
}

class _CreateClientScopesPageState extends State<CreateClientScopesPage> {
  final ScopePickerController scopePicker = ScopePickerController(
    ClientRole.receiver.scopes()!,
  );

  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();
    this.scopePicker.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    if (this.isPending.value) return;
    this.errorMessage.value = null;

    final scopes = this.scopePicker.scopes;
    if (scopes.isEmpty) {
      this.errorMessage.value = const ErrorMessage(
        title: "Select at least one scope",
      );
      return;
    }

    this.isPending.value = true;

    final req = CreateClientRequest(
      expiresAt: this.widget.expiresAtMs,
      label: this.widget.label,
      scopes: this.scopePicker.scopesList(),
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
          ScopePicker(controller: this.scopePicker),

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
        // Create button
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isPending,
            builder: (context, isPending, _widget) => AnimatedFillButton(
              onTap: this.onSubmit,
              loading: isPending,
              label: const Text("Create"),
              icon: const Icon(LxIcons.add),
            ),
          ),
        ),
      ),
    );
  }
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

/// User-facing display strings for [Scope].
extension ScopeExt on Scope {
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
