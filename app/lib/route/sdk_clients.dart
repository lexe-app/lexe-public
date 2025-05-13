import 'dart:async';

import 'package:app_rs_dart/ffi/api.dart'
    show CreateClientRequest, CreateClientResponse;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show RevocableClient, Scope;
import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
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
        SubheadingText;
import 'package:lexeapp/date_format.dart' as date_format;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/service/clients.dart' show ClientsService;
import 'package:lexeapp/style.dart' show LxIcons, Space;

/// This page lets users add, edit, and revoke SDK client credentials.
class SdkClientsPage extends StatefulWidget {
  const SdkClientsPage({
    super.key,
    required this.app,
  });

  final AppHandle app;

  @override
  State<SdkClientsPage> createState() => _SdkClientsPageState();
}

class _SdkClientsPageState extends State<SdkClientsPage> {
  /// List clients on refresh.
  late final ClientsService clientsService =
      ClientsService(app: this.widget.app);

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
    final CreateClientResponse? response =
        await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => CreateClientPage(app: this.widget.app),
    ));
    if (!this.mounted || response == null) return;

    // Refresh list in the background
    this.triggerRefresh();

    await Navigator.of(this.context).push(MaterialPageRoute(
      builder: (context) => ShowCredentialsPage(response: response),
    ));
  }

  Future<void> onRevokePressed(RevocableClient client) async {
    info("Revoke client pressed (${client.pubkey})");
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
                HeadingText(text: "Manage SDK Clients"),
                SubheadingText(
                  text:
                      "Add, edit, and revoke client credentials that can connect to your Lexe node",
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
                    child: ErrorMessageSection(ErrorMessage(
                      title: "Failed to fetch clients",
                      message: err.message,
                    )),
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
  const ClientListEntry(
      {super.key, required this.client, required this.onRevokedPressed});

  final RevocableClient client;
  final RevokeCallback onRevokedPressed;

  @override
  Widget build(BuildContext context) {
    final client = this.client;
    final title = client.label ?? "<unlabeled>";
    final createdAtUtc =
        DateTime.fromMillisecondsSinceEpoch(client.createdAt, isUtc: true);
    final createdAt = date_format.formatDateFull(createdAtUtc);

    final subtitle = "created: $createdAt\npublic key: ${client.pubkey}";
    return ListTile(
      isThreeLine: true,
      contentPadding: EdgeInsets.zero,
      title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(subtitle, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: IconButton(
        icon: const Icon(LxIcons.delete),
        onPressed: () => this.onRevokedPressed(client),
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
        Navigator.of(context).pop(ok);
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
          const HeadingText(text: "Create new client credentials"),
          const SubheadingText(
            text:
                "These credentials are tied to your Lexe node and can be used to send and receive payments with the Lexe SDK.",
          ),
          const SizedBox(height: Space.s600),

          // Label field
          CupertinoFormSection.insetGrouped(
            margin: EdgeInsets.zero,
            children: [
              CupertinoTextFormFieldRow(
                key: this.labelFieldKey,
                prefix: const Text("Label"),
                placeholder: "e.g. \"LightningAddress service\"",
                textInputAction: TextInputAction.done,
                autofocus: true,
                maxLines: 1,
                maxLength: 64,
                enableSuggestions: false,
                autocorrect: false,
                onEditingComplete: this.onSubmit,
                validator: (value) {
                  if (value == null || value.isEmpty) {
                    return "Label cannot be empty";
                  }
                  return null;
                },
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
                HeadingText(text: "Save your credentials"),
                SubheadingText(
                  text:
                      "Please save your credentials in a safe place. You will not be able to see them again.\n\nKeep them secure, as anyone with these credentials has access to your node and your funds.",
                ),
                SizedBox(height: Space.s400),
              ],
            ),
          ),
          InfoCard(
            children: [
              InfoRow(
                label: "public key",
                value: this.widget.response.client.pubkey,
              ),
              InfoRow(
                label: "credentials",
                value: this.widget.response.credentials,
              ),
            ],
          )
        ],
      ),
    );
  }
}
