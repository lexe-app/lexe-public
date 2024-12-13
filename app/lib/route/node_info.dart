import 'package:app_rs_dart/ffi/api.dart' show NodeInfo;
import 'package:app_rs_dart/ffi/types.dart' show AppUserInfo;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show HeadingText, LxCloseButton, ScrollableSinglePageBody, SubheadingText;
import 'package:lexeapp/route/payment_detail.dart'
    show PaymentDetailInfoCard, PaymentDetailInfoRow;
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

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Node Info"),
          const SubheadingText(text: "Your Lexe user and node identities."),
          const SizedBox(height: Space.s500),

          // NodeInfo and userInfo.nodePk{Proof}
          PaymentDetailInfoCard(
            header: "Node",
            children: [
              ValueListenableBuilder(
                valueListenable: this.widget.nodeInfo,
                builder: (context, nodeInfo, child) => PaymentDetailInfoRow(
                    label: "Version", value: nodeInfo?.version ?? ""),
              ),
              ValueListenableBuilder(
                valueListenable: this.widget.nodeInfo,
                builder: (context, nodeInfo, child) => PaymentDetailInfoRow(
                    label: "Measurement", value: nodeInfo?.measurement ?? ""),
              ),
              PaymentDetailInfoRow(
                  label: "Node public key", value: userInfo.nodePk),
              // Show the NodePkProof here so a user can prove possession of their
              // node key pair.
              PaymentDetailInfoRow(
                  label: "Proof-of-Possession", value: userInfo.nodePkProof),
            ],
          ),

          // UserPk
          PaymentDetailInfoCard(header: "User", children: [
            PaymentDetailInfoRow(
                label: "User public key", value: userInfo.userPk),
          ]),
        ],
      ),
    );
  }
}
