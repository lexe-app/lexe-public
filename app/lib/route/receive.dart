import 'dart:math';

import 'package:flutter/material.dart';

import 'package:lexeapp/address_format.dart' as address_format;
import 'package:lexeapp/components.dart'
    show
        CarouselIndicators,
        LxBackButton,
        LxFilledButton,
        ScrollableSinglePageBody;
import 'package:lexeapp/logger.dart';
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
        title: const Text(
          "Receive payment",
          style: TextStyle(
            fontSize: Fonts.size500,
            fontVariations: [Fonts.weightMedium],
            letterSpacing: -0.5,
            height: 1.0,
          ),
        ),
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        body: [
          const SizedBox(height: Space.s500),

          // QR
          SizedBox(
            height: 545.0,
            child: LayoutBuilder(
              builder: (context, constraints) => PageView(
                controller: PageController(
                  initialPage: 0,
                  viewportFraction:
                      min(1.0, 365.0 / max(1.0, constraints.minWidth)),
                ),
                padEnds: true,
                children: const [
                  LnInvoiceCard(),
                  BtcAddressCard(),
                ],
              ),
            ),
          ),

          const SizedBox(height: Space.s400),

          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.s600),
            child: Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                const IconButton(
                  onPressed: null,
                  icon: Icon(Icons.chevron_left_rounded),
                  color: LxColors.fgSecondary,
                  disabledColor: LxColors.clearB0,
                  padding: EdgeInsets.zero,
                  visualDensity: VisualDensity.compact,
                ),
                CarouselIndicators(
                  selectedPageIndex: ValueNotifier(0),
                  numPages: 2,
                ),
                IconButton(
                  onPressed: () {},
                  icon: const Icon(Icons.chevron_right_rounded),
                  color: LxColors.fgSecondary,
                  disabledColor: LxColors.clearB0,
                  padding: EdgeInsets.zero,
                  visualDensity: VisualDensity.compact,
                ),
              ],
            ),
          ),

          const SizedBox(height: Space.s200),

          Padding(
            padding: const EdgeInsets.symmetric(horizontal: Space.s600),
            child: Row(
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
                    label: const Text("Amount"),
                    icon: const Icon(Icons.add_rounded),
                    onTap: () {},
                  ),
                ),
              ],
            ),
          ),

          const SizedBox(height: Space.s400),
        ],
      ),
    );
  }
}

class ReceiveCard extends StatelessWidget {
  const ReceiveCard({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisAlignment: MainAxisAlignment.start,
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: Space.s200),
          child: Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            clipBehavior: Clip.antiAlias,
            padding: const EdgeInsets.fromLTRB(
                Space.s500, Space.s500, Space.s500, Space.s500),
            constraints: const BoxConstraints(maxWidth: 350.0),
            child: this.child,
          ),
        ),
        const Expanded(child: Center()),
      ],
    );
  }
}

class LnInvoiceCard extends StatelessWidget {
  const LnInvoiceCard({super.key});

  @override
  Widget build(BuildContext context) {
    return ReceiveCard(
      child: Column(
        mainAxisAlignment: MainAxisAlignment.start,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            "Lightning offer",
            style: TextStyle(
              color: LxColors.foreground,
              fontSize: Fonts.size300,
              fontVariations: [Fonts.weightMedium],
              letterSpacing: -0.5,
              height: 1.0,
            ),
          ),
          // const SizedBox(height: Space.s100),
          Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                address_format.ellipsizeBtcAddress(
                    "lno1pqps7sjqpgtyzm3qv4uxzmtsd3jjqer9wd3hy6tsw35k7msjzfpy7nz5yqcnygrfdej82um5wf5k2uckyypwa3eyt44h6txtxquqh7lz5djge4afgfjn7k4rgrkuag0jsd5xvxg"),
                // "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontSize: Fonts.size100,
                  color: LxColors.fgTertiary,
                  height: 1.0,
                ),
              ),
              IconButton(
                onPressed: () {},
                icon: const Icon(
                  Icons.copy_rounded,
                  // size: Fonts.size300,
                ),
                color: LxColors.fgTertiary,
                visualDensity:
                    const VisualDensity(horizontal: -4.0, vertical: -4.0),
                padding: EdgeInsets.zero,
                iconSize: Fonts.size300,
                // style: IconButton.styleFrom(fixedSize: Size.square(20.0)),
              ),
            ],
          ),
          const SizedBox(height: Space.s200),
          // const Text(
          //   "invoices can only be paid once!",
          //   style: TextStyle(
          //     color: LxColors.fgTertiary,
          //     fontSize: Fonts.size200,
          //     // fontVariations: [Fonts.weightMedium],
          //     // letterSpacing: -0.5,
          //     height: 1.5,
          //   ),
          // ),
          // const SizedBox(height: Space.s300),
          // const Text(
          //   "the rice house 🍕",
          //   style: TextStyle(
          //     color: LxColors.grey550,
          //     fontSize: Fonts.size300,
          //     letterSpacing: -0.25,
          //     height: 1.5,
          //     // fontVariations: [Fonts.weightMedium],
          //   ),
          // ),
          // const SizedBox(height: Space.s400),
          LayoutBuilder(
            builder: (context, constraints) {
              return QrImage(
                value:
                    "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
                // value:
                //     "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
                // value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
                dimension: constraints.maxWidth.toInt(),
                color: LxColors.foreground,
              );
            },
          ),
          const SizedBox(height: Space.s400),

          // Amount (sats)
          const Text.rich(
            TextSpan(
              children: [
                TextSpan(text: "5,300 "),
                TextSpan(
                    text: "sats", style: TextStyle(color: LxColors.grey550)),
              ],
              style: TextStyle(
                fontSize: Fonts.size600,
                letterSpacing: -0.5,
                fontVariations: [Fonts.weightMedium],
                height: 1.0,
              ),
            ),
          ),
          const SizedBox(height: Space.s100),

          // Amount (fiat)
          const Text(
            "≈ \$3.69",
            style: TextStyle(
              color: LxColors.fgTertiary,
              fontSize: Fonts.size400,
              letterSpacing: -0.5,
              height: 1.0,
            ),
          ),
          const SizedBox(height: Space.s400),

          // Description
          const Text(
            // "the rice house 🍕",
            "really really long description holy shit just stfu you need to stop please omg i can't anymore",
            style: TextStyle(
              color: LxColors.foreground,
              fontSize: Fonts.size200,
              height: 1.5,
              letterSpacing: -0.5,
            ),
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
          ),
          // const SizedBox(height: Space.s200),
          // const SizedBox(height: Space.s500),
        ],
      ),
    );
  }
}

class BtcAddressCard extends StatelessWidget {
  const BtcAddressCard({super.key});

  @override
  Widget build(BuildContext context) {
    return ReceiveCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Expanded(
                child: Text(
                  "Bitcoin address",
                  style: TextStyle(
                    color: LxColors.foreground,
                    fontSize: Fonts.size300,
                    fontVariations: [Fonts.weightMedium],
                    letterSpacing: -0.5,
                    height: 1.0,
                  ),
                ),
              ),
              IconButton(
                onPressed: () {},
                icon: const Icon(
                  Icons.copy_rounded,
                  // size: Fonts.size300,
                ),
                color: LxColors.fgSecondary,
                visualDensity:
                    const VisualDensity(horizontal: -3.0, vertical: -3.0),
                padding: EdgeInsets.zero,
                iconSize: Fonts.size500,
                // style: IconButton.styleFrom(fixedSize: Size.square(20.0)),
              ),
            ],
          ),
          // const SizedBox(height: Space.s100),
          // Row(
          //   children: [
          //     const Expanded(
          //       child: Text(
          //         "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
          //         maxLines: 1,
          //         overflow: TextOverflow.ellipsis,
          //         style: TextStyle(
          //           fontSize: Fonts.size100,
          //           color: LxColors.fgTertiary,
          //         ),
          //       ),
          //     ),
          //     IconButton(
          //       onPressed: () {},
          //       icon: const Icon(
          //         Icons.copy_rounded,
          //         // size: Fonts.size300,
          //       ),
          //       color: LxColors.fgTertiary,
          //       visualDensity:
          //           const VisualDensity(horizontal: -4.0, vertical: -4.0),
          //       padding: EdgeInsets.zero,
          //       iconSize: Fonts.size300,
          //       // style: IconButton.styleFrom(fixedSize: Size.square(20.0)),
          //     ),
          //   ],
          // ),
          const SizedBox(height: Space.s400),
          // const Text(
          //   "invoices can only be paid once!",
          //   style: TextStyle(
          //     color: LxColors.fgTertiary,
          //     fontSize: Fonts.size200,
          //     // fontVariations: [Fonts.weightMedium],
          //     // letterSpacing: -0.5,
          //     height: 1.5,
          //   ),
          // ),
          // const SizedBox(height: Space.s300),
          // const Text(
          //   "the rice house 🍕",
          //   style: TextStyle(
          //     color: LxColors.grey550,
          //     fontSize: Fonts.size300,
          //     letterSpacing: -0.25,
          //     height: 1.5,
          //     // fontVariations: [Fonts.weightMedium],
          //   ),
          // ),
          // const SizedBox(height: Space.s400),
          LayoutBuilder(
            builder: (context, constraints) => QrImage(
              // value:
              //     "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
              // value:
              //     "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
              value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
              dimension: constraints.maxWidth.toInt(),
              color: LxColors.foreground,
            ),
          ),
          // const SizedBox(height: Space.s400),

          // Row(
          //   mainAxisAlignment: MainAxisAlignment.start,
          //   crossAxisAlignment: CrossAxisAlignment.start,
          //   children: [
          //     Expanded(
          //       child: Column(
          //         crossAxisAlignment: CrossAxisAlignment.start,
          //         children: [
          //           ActionChip.elevated(
          //             onPressed: () {},
          //             avatar: const Icon(
          //               Icons.add_rounded,
          //               color: LxColors.foreground,
          //             ),
          //             color: const MaterialStatePropertyAll(
          //                 LxColors.grey1000),
          //             label: const Text("Note"),
          //             labelStyle:
          //                 const TextStyle(color: LxColors.foreground),
          //             elevation: 0.0,
          //             shadowColor: LxColors.clearB0,
          //             side: const BorderSide(color: LxColors.foreground),
          //           ),
          //         ],
          //       ),
          //       // Chip(
          //       //   label: Text("Description"),
          //       // ),
          //     ),
          //     const SizedBox(width: Space.s300),
          //     Expanded(
          //       child: Column(
          //         crossAxisAlignment: CrossAxisAlignment.end,
          //         children: [
          //           // ActionChip.elevated(
          //           //   onPressed: () {},
          //           //   avatar: const Icon(
          //           //     Icons.add_rounded,
          //           //     color: LxColors.foreground,
          //           //   ),
          //           //   color: const MaterialStatePropertyAll(
          //           //       LxColors.grey1000),
          //           //   label: const Text("Amount"),
          //           //   labelStyle:
          //           //       const TextStyle(color: LxColors.foreground),
          //           //   elevation: 0.0,
          //           //   shadowColor: LxColors.clearB0,
          //           //   side: const BorderSide(color: LxColors.foreground),
          //           // ),
          //         ],
          //       ),
          //     ),
          //   ],
          // ),

          // const SizedBox(height: Space.s500),
          const SizedBox(height: Space.s600),
        ],
      ),
    );
  }
}
