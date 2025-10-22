import 'dart:convert' show JsonEncoder, jsonDecode;

import 'package:app_rs_dart/ffi/api.dart' show NodeInfo;
import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show AppUserInfo;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show DateFormat;
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        InfoCard,
        InfoRow,
        InfoRowButton,
        LxCloseButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/result.dart' show Result;
import 'package:lexeapp/route/raw_data.dart' show RawDataPage;
import 'package:lexeapp/style.dart' show Space;

/// A basic page containing relevant user and user node identities, versions,
/// etc...
class NodeInfoPage extends StatefulWidget {
  const NodeInfoPage({
    super.key,
    required this.nodeInfo,
    required this.userInfo,
    required this.app,
  });

  final ValueListenable<NodeInfo?> nodeInfo;
  final AppUserInfo userInfo;
  final AppHandle app;

  @override
  State<NodeInfoPage> createState() => _NodeInfoPageState();
}

class _NodeInfoPageState extends State<NodeInfoPage> {
  Future<Result<String, Exception>> listBroadcastedTxs() async {
    return Result.tryAsync(() async {
      final result = await this.widget.app.listBroadcastedTxs();
      return this._formatTimestamps(result);
    });
  }

  /// Called when "Brodcasted Txs" button is pressed.
  void onBroadcastedTxsTap() {
    Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (context) => RawDataPage(
          title: "Broadcasted Transactions",
          subtitle: "All on-chain transactions your node has ever broadcast",
          data: this.listBroadcastedTxs(),
        ),
      ),
    );
  }

  String _formatTimestamps(String jsonString) {
    final data = jsonDecode(jsonString);

    if (data is List) {
      for (final item in data) {
        if (item is Map<String, dynamic> && item.containsKey('created_at')) {
          final createdAtMs = item['created_at'] as int;
          final dateTime = DateTime.fromMillisecondsSinceEpoch(createdAtMs);
          final formatted = DateFormat('yyyy-MM-dd HH:mm:ss').format(dateTime);
          item['created_at'] = formatted;
        }
      }
    }

    const encoder = JsonEncoder.withIndent('  ');
    return encoder.convert(data);
  }

  @override
  Widget build(BuildContext context) {
    final userInfo = this.widget.userInfo;
    const cardPad = Space.s300;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),
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
                HeadingText(text: "Node Info"),
                SubheadingText(text: "Your Lexe user and node identities."),
                SizedBox(height: Space.s500),
              ],
            ),
          ),

          // NodeInfo and userInfo.nodePk{Proof}
          InfoCard(
            header: const Text("Node"),
            children: [
              ValueListenableBuilder(
                valueListenable: this.widget.nodeInfo,
                builder: (context, nodeInfo, child) {
                  final version = nodeInfo?.version;
                  // Use " " to prevent slight vertical reflow when node info
                  // fills.
                  final vVersion = (version != null) ? "v$version" : " ";
                  return InfoRow(label: "Version", value: vVersion);
                },
              ),
              ValueListenableBuilder(
                valueListenable: this.widget.nodeInfo,
                builder: (context, nodeInfo, child) => InfoRow(
                  label: "Measurement",
                  value: nodeInfo?.measurement ?? " ",
                ),
              ),
              InfoRow(label: "Node public key", value: userInfo.nodePk),
              // Show the NodePkProof here so a user can prove possession of their
              // node key pair.
              InfoRow(
                label: "Proof-of-Possession",
                value: userInfo.nodePkProof,
              ),
            ],
          ),

          // UserPk
          InfoCard(
            header: const Text("User"),
            children: [
              InfoRow(label: "User public key", value: userInfo.userPk),
            ],
          ),

          InfoCard(
            header: const Text("Node internals"),
            children: [
              InfoRowButton(
                label: Text("View broadcasted transactions"),
                onTap: this.onBroadcastedTxsTap,
              ),
            ],
          ),
        ],
      ),
    );
  }
}
