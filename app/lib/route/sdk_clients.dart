import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show ClientInfo;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxCloseButton,
        LxFilledButton,
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
  final ValueNotifier<FfiResult<List<ClientInfo>>?> clients =
      ValueNotifier(null);

  @override
  void initState() {
    super.initState();

    // Fetch the clients when the page is opened
    unawaited(this.listClients());
  }

  Future<void> listClients() async {
    // TODO(phlip9): remove
    this.clients.value = null;

    final res = await Result.tryFfiAsync(this.widget.app.listClients);
    if (!this.mounted) return;

    res.inspectErr((err) => error("Failed to fetch clients: $err"));
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
              Err(:final err) => SliverToBoxAdapter(
                  child: ErrorMessageSection(ErrorMessage(
                    title: "Failed to fetch clients",
                    message: err.message,
                  )),
                ),
              Ok(:final ok) => SliverToBoxAdapter(
                  child: Text("List of clients goes here (${ok.length})"),
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
