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
        // LxRefreshButton,
        ScrollableSinglePageBody,
        SliverPullToRefresh,
        SubheadingText;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
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
  final ValueNotifier<FfiResult<List<RevocableClient>>?> clients =
      ValueNotifier(null);

  @override
  void initState() {
    super.initState();

    // Fetch the clients when the page is opened
    unawaited(this.listClients());
  }

  Future<void> listClients() async {
    final res = await Result.tryFfiAsync(this.widget.app.listClients);
    if (!this.mounted) return;

    res.inspectErr((err) => error("Failed to fetch clients: $err")).map(
      (clients) {
        // sort clients by creation date (newest first)
        clients.sort((c1, c2) => c2.createdAt.compareTo(c1.createdAt));
        return clients;
      },
    );
    this.clients.value = res;
  }

  Future<void> onCreatePressed() async {
    info("Create new client pressed");
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),

        // // Refresh
        // actions: [
        //   LxRefreshButton(
        //     isRefreshing: this.isRefreshing,
        //     triggerRefresh: this.listClients,
        //   ),
        //   const SizedBox(width: Space.s100),
        // ],
      ),
      body: ScrollableSinglePageBody(
        bodySlivers: [
          // Pull-to-refresh
          SliverPullToRefresh(onRefresh: this.listClients),

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
            valueListenable: this.clients,
            builder: (_context, listResult, _widget) => switch (listResult) {
              // Still loading
              null => const SliverToBoxAdapter(
                  child: Center(
                      child: Padding(
                          padding: EdgeInsets.symmetric(vertical: Space.s400),
                          child: CircularProgressIndicator())),
                ),
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
                    return ClientListEntry(client: client);
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

/// A single entry in the list of clients.
class ClientListEntry extends StatelessWidget {
  const ClientListEntry({super.key, required this.client});

  final RevocableClient client;

  @override
  Widget build(BuildContext context) {
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
        onPressed: () {
          // TODO(phlip9): implement revoke client
          info("Revoke client ${client.pubkey}");
        },
      ),
    );
  }
}
