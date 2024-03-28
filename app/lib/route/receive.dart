import 'package:flutter/material.dart';

import 'package:lexeapp/components.dart'
    show
        HeadingText,
        LxBackButton,
        LxFilledButton,
        LxOutlinedButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/route/show_qr.dart' show QrImage;
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxRadius, Space;

class ReceivePaymentPage extends StatefulWidget {
  const ReceivePaymentPage({super.key});

  @override
  State<ReceivePaymentPage> createState() => ReceivePaymentPageState();
}

class ReceivePaymentPageState extends State<ReceivePaymentPage> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Receive payment"),
          const SubheadingText(text: "Unified Bitcoin and Lightning QR code"),
          const SizedBox(height: Space.s800),

          // QR
          Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            clipBehavior: Clip.antiAlias,
            padding: const EdgeInsets.fromLTRB(
                Space.s600, Space.s400, Space.s600, 0),
            // constraints: const BoxConstraints(minWidth: 300, maxWidth: 300),
            child: Column(
              children: [
                const Text(
                  "the rice house 🍕",
                  textAlign: TextAlign.start,
                  style: TextStyle(
                    color: LxColors.fgSecondary,
                    fontSize: Fonts.size300,
                  ),
                ),
                const SizedBox(height: Space.s300),
                LayoutBuilder(
                  builder: (context, constraints) => QrImage(
                    value:
                        "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
                    // value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
                    dimension: constraints.maxWidth.toInt(),
                    color: LxColors.foreground,
                  ),
                ),
                const SizedBox(height: Space.s200),
                const Text.rich(
                  TextSpan(
                    children: [
                      TextSpan(text: "5,300 "),
                      TextSpan(
                          text: "sats",
                          style: TextStyle(color: LxColors.grey550)),
                    ],
                    style: TextStyle(
                      fontSize: Fonts.size700,
                      letterSpacing: -0.5,
                      fontVariations: [Fonts.weightMedium],
                    ),
                  ),
                ),
                const Text(
                  "≈ \$3.69",
                  style: TextStyle(
                    color: LxColors.fgTertiary,
                    fontSize: Fonts.size500,
                    letterSpacing: -0.5,
                  ),
                ),
                const SizedBox(height: Space.s500),
              ],
            ),
          ),
          const SizedBox(height: Space.s500),

          Row(
            children: [
              LxFilledButton(
                icon: const Icon(Icons.settings_rounded),
                onTap: () {},
              ),
              const SizedBox(width: Space.s200),
              LxFilledButton(
                icon: const Icon(Icons.share_rounded),
                onTap: () {},
              ),
              const SizedBox(width: Space.s200),
              Expanded(
                child: LxFilledButton(
                  label: const Text("Set Amount"),
                  icon: const Icon(Icons.add_rounded),
                  onTap: () {},
                ),
              ),
            ],
          )
        ],
      ),
    );
  }
}
