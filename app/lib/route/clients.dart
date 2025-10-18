import 'dart:async';

import 'package:app_rs_dart/ffi/api.dart'
    show CreateClientRequest, CreateClientResponse, UpdateClientRequest;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show RevocableClient, Scope;
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
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
        SliverPullToRefresh,
        SubheadingText,
        showModalAsyncFlow;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/service/clients.dart' show ClientsService;
import 'package:lexeapp/style.dart' show LxColors, LxIcons, Space;

/// This page lets users add, edit, and revoke SDK client credentials.
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
    final CreateClientResponse? response = await Navigator.of(this.context)
        .push(
          MaterialPageRoute(
            builder: (context) => CreateClientPage(app: this.widget.app),
          ),
        );
    if (!this.mounted || response == null) return;

    // Refresh list in the background
    this.triggerRefresh();

    await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => ShowCredentialsPage(response: response),
      ),
    );
  }

  Future<void> onRevokePressed(RevocableClient client) async {
    info("pressed revoke client (${client.pubkey})");

    final req = UpdateClientRequest(pubkey: client.pubkey, isRevoked: true);
    final fut = Result.tryFfiAsync(
      () => this.widget.app.updateClient(req: req),
    );

    final res = await showModalAsyncFlow(
      context: this.context,
      future: fut,
      errorBuilder: (context, err) => AlertDialog(
        title: const Text("Failed to revoke client"),
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
                HeadingText(text: "Manage SDK clients"),
                SubheadingText(
                  text:
                      "Add, edit, and revoke clients that can control your Lexe node with the Lexe SDK",
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
                      title: "Failed to fetch clients",
                      message: err.message,
                    ),
                  ),
                ),
              ),
              // List of clients
              Ok(:final ok) => SliverFixedExtentList.builder(
                itemExtent: Space.s850,
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
            label: const Text("Create new client"),
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
    final createdAtUtc = DateTime.fromMillisecondsSinceEpoch(
      client.createdAt,
      isUtc: true,
    );
    final createdAt = date_format.formatDateFull(createdAtUtc);

    final subtitle = "created: $createdAt\npublic key: ${client.pubkey}";
    return ListTile(
      contentPadding: EdgeInsets.zero,
      title: (label != null)
          ? Text(label, maxLines: 1, overflow: TextOverflow.ellipsis)
          : null,
      subtitle: Text(subtitle, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: IconButton(
        icon: const Icon(LxIcons.delete, weight: LxIcons.weightMedium),
        onPressed: this._onRevokedPressed,
      ),
    );
  }
}

class CreateClientPage extends StatefulWidget {
  const CreateClientPage({super.key, required this.app});

  final AppHandle app;

  @override
  State<CreateClientPage> createState() => _CreateClientPageState();
}

class _CreateClientPageState extends State<CreateClientPage> {
  final GlobalKey<FormFieldState<String>> labelFieldKey = GlobalKey();

  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    this.errorMessage.value = null;

    final labelField = this.labelFieldKey.currentState!;
    if (!labelField.validate()) return;
    final label = labelField.value;

    this.isPending.value = true;

    // TODO(phlip9): allow configuring scope once there are more useful scopes
    final req = CreateClientRequest(label: label, scope: Scope.all);
    final res = await Result.tryFfiAsync(
      () => this.widget.app.createClient(req: req),
    );
    if (!this.mounted) return;

    this.isPending.value = false;

    switch (res) {
      case Ok(:final ok):
        final CreateClientResponse response = ok;
        info("create-client: created: ${response.client.pubkey}");
        Navigator.of(this.context).pop(ok);
      case Err(:final err):
        error("create-client: error: ${err.message}");
        this.errorMessage.value = ErrorMessage(
          title: "Failed to create client",
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
          const HeadingText(text: "Create new client"),
          const SubheadingText(
            text:
                "This client is tied to your Lexe node and can be used to send and receive payments with the Lexe SDK.",
          ),
          const SizedBox(height: Space.s600),

          // Label field
          CupertinoFormSection.insetGrouped(
            margin: EdgeInsets.zero,
            children: [
              CupertinoTextFormFieldRow(
                key: this.labelFieldKey,
                prefix: const Text(
                  "Label (optional) ",
                  style: TextStyle(color: LxColors.grey550),
                ),
                placeholder: "e.g. \"LightningAddress service\"",
                placeholderStyle: const TextStyle(color: LxColors.grey750),
                textInputAction: TextInputAction.done,
                autofocus: true,
                maxLines: 1,
                maxLength: 64,
                enableSuggestions: false,
                autocorrect: false,
                onEditingComplete: this.onSubmit,
              ),
            ],
          ),

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
                      "Please save your client credentials in a safe place. You will not be able to see them again.\n\nKeep them secure, as anyone with these credentials has access to your node and your funds.",
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
