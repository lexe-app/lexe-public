import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show RevocableClient;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxCloseButton,
        LxFilledButton,
        LxRefreshButton,
        ScrollableSinglePageBody,
        SliverPullToRefresh,
        SubheadingText;
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
    info("Create new client pressed");
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
                  child: ErrorMessageSection(ErrorMessage(
                    title: "Failed to fetch clients",
                    message: err.message,
                  )),
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
    final createdAt = DateTime.fromMillisecondsSinceEpoch(client.createdAt);
    final subtitle = "created: $createdAt\npublic key: ${client.pubkey}";
    return ListTile(
      isThreeLine: true,
      contentPadding: EdgeInsets.zero,
      title: Text(title),
      subtitle: Text(subtitle, maxLines: 2, overflow: TextOverflow.ellipsis),
      trailing: IconButton(
        icon: const Icon(LxIcons.delete),
        onPressed: () => this.onRevokedPressed(client),
      ),
    );
  }
}
