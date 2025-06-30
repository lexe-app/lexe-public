import 'package:app_rs_dart/ffi/api.dart' show NodeInfo;
import 'package:app_rs_dart/ffi/types.dart' show AppUserInfo;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        InfoCard,
        InfoRow,
        LxCloseButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/style.dart' show Space;

/// A basic page containing relevant user and user node identities, versions,
/// etc...
class NodeInfoPage extends StatefulWidget {
  const NodeInfoPage({
    super.key,
    required this.nodeInfo,
    required this.userInfo,
  });

  final ValueListenable<NodeInfo?> nodeInfo;
  final AppUserInfo userInfo;

  @override
  State<NodeInfoPage> createState() => _NodeInfoPageState();
}

class _NodeInfoPageState extends State<NodeInfoPage> {
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
            header: "Node",
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
            header: "User",
            children: [
              InfoRow(label: "User public key", value: userInfo.userPk),
            ],
          ),
        ],
      ),
    );
  }
}
