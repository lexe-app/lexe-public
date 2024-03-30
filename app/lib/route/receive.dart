import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/widgets.dart';

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

class ReceivePaymentPage2 extends StatefulWidget {
  const ReceivePaymentPage2({super.key});

  @override
  State<ReceivePaymentPage2> createState() => ReceivePaymentPageState2();
}

class ReceivePaymentPageState2 extends State<ReceivePaymentPage2> {
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
          // const SubheadingText(text: "Unified Bitcoin and Lightning QR code"),
          const SizedBox(height: Space.s700),

          // QR
          Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            clipBehavior: Clip.antiAlias,
            padding: const EdgeInsets.fromLTRB(
                Space.s600, Space.s400, Space.s600, 0),
            // Space.s600, Space.s200, Space.s600, 0),
            // constraints: const BoxConstraints(minWidth: 300, maxWidth: 300),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  "the rice house 🍕",
                  style: TextStyle(
                    color: LxColors.fgSecondary,
                    fontSize: Fonts.size300,
                    fontVariations: [Fonts.weightMedium],
                  ),
                ),
                const SizedBox(height: Space.s300),
                // const Text(
                //   "the rice house 🍕",
                //   style: TextStyle(
                //     color: LxColors.fgSecondary,
                //     fontSize: Fonts.size300,
                //     fontVariations: [Fonts.weightMedium],
                //   ),
                // ),
                // const SizedBox(height: Space.s300),
                LayoutBuilder(
                  builder: (context, constraints) => QrImage(
                    value:
                        "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
                    // value:
                    //     "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
                    // value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
                    dimension: constraints.maxWidth.toInt(),
                    color: LxColors.foreground,
                  ),
                ),
                const SizedBox(height: Space.s500),
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
                      height: 1.2,
                    ),
                  ),
                ),
                const Text(
                  "≈ \$3.69",
                  style: TextStyle(
                    color: LxColors.fgTertiary,
                    fontSize: Fonts.size500,
                    letterSpacing: -0.5,
                    height: 1.2,
                  ),
                ),
                const SizedBox(height: Space.s500),
              ],
            ),
          ),
          // const Padding(
          //   padding: EdgeInsets.all(Space.s100),
          //   child: Chip(
          //     label: Text(
          //       "Lightning Invoice",
          //       style: TextStyle(
          //         color: LxColors.fgSecondary,
          //         fontSize: Fonts.size200,
          //         fontVariations: [Fonts.weightMedium],
          //       ),
          //       textAlign: TextAlign.center,
          //     ),
          //     color: MaterialStatePropertyAll(LxColors.grey1000),
          //     elevation: 5.0,
          //     shadowColor: LxColors.clearB100,
          //   ),
          // ),

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

class ReceivePaymentPage3 extends StatefulWidget {
  const ReceivePaymentPage3({super.key});

  @override
  State<ReceivePaymentPage3> createState() => ReceivePaymentPageState3();
}

class ReceivePaymentPageState3 extends State<ReceivePaymentPage3> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        // title: const Text(
        //   "Receive payment",
        //   style: TextStyle(
        //     color: LxColors.foreground,
        //     fontVariations: [Fonts.weightMedium],
        //     fontSize: Fonts.size600,
        //     letterSpacing: -0.5,
        //     height: 1.0,
        //   ),
        // ),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Receive payment"),
          // const SubheadingText(text: "Unified Bitcoin and Lightning QR code"),
          const SizedBox(height: Space.s500),

          // const Padding(
          //   padding: EdgeInsets.only(top: Space.s700, bottom: Space.s400),
          //   child: Text(
          //     "Lightning Invoice",
          //     style: TextStyle(
          //       color: LxColors.fgSecondary,
          //       fontVariations: [Fonts.weightMedium],
          //       fontSize: Fonts.size400,
          //       // letterSpacing: -0.5,
          //     ),
          //     textAlign: TextAlign.center,
          //   ),
          // ),

          // QR
          Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            clipBehavior: Clip.antiAlias,
            padding: const EdgeInsets.fromLTRB(
                Space.s600, Space.s500, Space.s600, 0),
            // Space.s600, Space.s200, Space.s600, 0),
            // constraints: const BoxConstraints(minWidth: 300, maxWidth: 300),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    const Expanded(
                      child: Text(
                        "Lightning invoice",
                        style: TextStyle(
                          color: LxColors.fgSecondary,
                          fontSize: Fonts.size500,
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
                    value:
                        "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
                    // value:
                    //     "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
                    // value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
                    dimension: constraints.maxWidth.toInt(),
                    color: LxColors.foreground,
                  ),
                ),
                const SizedBox(height: Space.s400),
                const Text(
                  "the rice house 🍕",
                  // "really really long description holy shit just stfu you need to stop please omg i can't anymore",
                  style: TextStyle(
                    color: LxColors.fgSecondary,
                    fontSize: Fonts.size200,
                    height: 1.5,
                    // letterSpacing: -0.25,
                    // fontVariations: [Fonts.weightMedium],
                  ),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
                const SizedBox(height: Space.s400),
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
                      height: 1.2,
                    ),
                  ),
                ),
                const Text(
                  "≈ \$3.69",
                  style: TextStyle(
                    color: LxColors.fgTertiary,
                    fontSize: Fonts.size500,
                    letterSpacing: -0.5,
                    height: 1.2,
                  ),
                ),
                const SizedBox(height: Space.s500),
              ],
            ),
          ),
          const SizedBox(height: Space.s200),

          // Container(
          //   margin: const EdgeInsets.symmetric(horizontal: Space.s700),
          //   padding: const EdgeInsets.symmetric(
          //       horizontal: Space.s400, vertical: Space.s100),
          //   child: ,
          // ),

          const SizedBox(height: Space.s200),

          Row(
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
              LandingCarouselIndicators(
                  selectedPageIndex: ValueNotifier(0), numPages: 2),
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

          const SizedBox(height: Space.s200),

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

class ReceivePaymentPage4 extends StatefulWidget {
  const ReceivePaymentPage4({super.key});

  @override
  State<ReceivePaymentPage4> createState() => ReceivePaymentPageState4();
}

class ReceivePaymentPageState4 extends State<ReceivePaymentPage4> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        // title: const Text(
        //   "Receive payment",
        //   style: TextStyle(
        //     color: LxColors.foreground,
        //     fontVariations: [Fonts.weightMedium],
        //     fontSize: Fonts.size600,
        //     letterSpacing: -0.5,
        //     height: 1.0,
        //   ),
        // ),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Receive payment"),
          // const SubheadingText(text: "Unified Bitcoin and Lightning QR code"),
          const SizedBox(height: Space.s500),

          // const Padding(
          //   padding: EdgeInsets.only(top: Space.s700, bottom: Space.s400),
          //   child: Text(
          //     "Lightning Invoice",
          //     style: TextStyle(
          //       color: LxColors.fgSecondary,
          //       fontVariations: [Fonts.weightMedium],
          //       fontSize: Fonts.size400,
          //       // letterSpacing: -0.5,
          //     ),
          //     textAlign: TextAlign.center,
          //   ),
          // ),

          // QR
          Container(
            decoration: BoxDecoration(
              color: LxColors.grey1000,
              borderRadius: BorderRadius.circular(LxRadius.r300),
            ),
            clipBehavior: Clip.antiAlias,
            padding: const EdgeInsets.fromLTRB(
                Space.s600, Space.s500, Space.s600, 0),
            // Space.s600, Space.s200, Space.s600, 0),
            // constraints: const BoxConstraints(minWidth: 300, maxWidth: 300),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    const Expanded(
                      child: Text(
                        "Lightning invoice",
                        style: TextStyle(
                          color: LxColors.fgSecondary,
                          fontSize: Fonts.size500,
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
                    value:
                        "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
                    // value:
                    //     "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
                    // value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
                    dimension: constraints.maxWidth.toInt(),
                    color: LxColors.foreground,
                  ),
                ),
                const SizedBox(height: Space.s400),

                const Row(
                  mainAxisAlignment: MainAxisAlignment.start,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(
                      child: Text(
                        "the rice house 🍕",
                        // "really really long description holy shit just stfu you need to stop please omg i can't anymore",
                        style: TextStyle(
                          color: LxColors.fgSecondary,
                          fontSize: Fonts.size200,
                          height: 1.5,
                          // letterSpacing: -0.25,
                          // fontVariations: [Fonts.weightMedium],
                        ),
                        maxLines: 3,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    SizedBox(width: Space.s300),
                    Expanded(
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.start,
                        crossAxisAlignment: CrossAxisAlignment.end,
                        children: [
                          FittedBox(
                            fit: BoxFit.scaleDown,
                            alignment: Alignment.topRight,
                            child: Text.rich(
                              TextSpan(
                                children: [
                                  TextSpan(text: "55,300 "),
                                  TextSpan(
                                      text: "sats",
                                      style:
                                          TextStyle(color: LxColors.grey550)),
                                ],
                                style: TextStyle(
                                  fontSize: Fonts.size600,
                                  letterSpacing: -0.5,
                                  fontVariations: [Fonts.weightMedium],
                                  height: 1.35,
                                ),
                              ),
                            ),
                          ),
                          FittedBox(
                            fit: BoxFit.scaleDown,
                            alignment: Alignment.topRight,
                            child: Text(
                              "≈ \$3.69",
                              style: TextStyle(
                                color: LxColors.fgTertiary,
                                fontSize: Fonts.size400,
                                letterSpacing: -0.5,
                                height: 1.35,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),

                const SizedBox(height: Space.s500),
              ],
            ),
          ),
          const SizedBox(height: Space.s200),

          // Container(
          //   margin: const EdgeInsets.symmetric(horizontal: Space.s700),
          //   padding: const EdgeInsets.symmetric(
          //       horizontal: Space.s400, vertical: Space.s100),
          //   child: ,
          // ),

          const SizedBox(height: Space.s200),

          Row(
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
              LandingCarouselIndicators(
                  selectedPageIndex: ValueNotifier(0), numPages: 2),
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

          const SizedBox(height: Space.s200),

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

class ReceivePaymentPage5 extends StatefulWidget {
  const ReceivePaymentPage5({super.key});

  @override
  State<ReceivePaymentPage5> createState() => ReceivePaymentPageState5();
}

class ReceivePaymentPageState5 extends State<ReceivePaymentPage5> {
  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(),
        // title: const Text(
        //   "Receive payment",
        //   style: TextStyle(
        //     color: LxColors.foreground,
        //     fontVariations: [Fonts.weightMedium],
        //     fontSize: Fonts.size600,
        //     letterSpacing: -0.5,
        //     height: 1.0,
        //   ),
        // ),
      ),
      body: ScrollableSinglePageBody(
        padding: EdgeInsets.zero,
        body: [
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: Space.s600),
            child: HeadingText(text: "Receive payment"),
          ),
          // const SubheadingText(text: "Unified Bitcoin and Lightning QR code"),
          const SizedBox(height: Space.s500),

          // const Padding(
          //   padding: EdgeInsets.only(top: Space.s700, bottom: Space.s400),
          //   child: Text(
          //     "Lightning Invoice",
          //     style: TextStyle(
          //       color: LxColors.fgSecondary,
          //       fontVariations: [Fonts.weightMedium],
          //       fontSize: Fonts.size400,
          //       // letterSpacing: -0.5,
          //     ),
          //     textAlign: TextAlign.center,
          //   ),
          // ),

          // QR
          // SizedBox(height: 400.0, child: LnInvoiceCard()),
          SizedBox(
            height: 400.0,
            child: PageView.builder(
              controller:
                  PageController(initialPage: 0, viewportFraction: 0.88),
              padEnds: true,
              itemBuilder: (context, idx) => (idx == 0)
                  ? const LnInvoiceCard()
                  : (idx == 1)
                      ? const BtcAddressCard()
                      : null,
            ),
          ),

          const SizedBox(height: Space.s200),

          // Container(
          //   margin: const EdgeInsets.symmetric(horizontal: Space.s700),
          //   padding: const EdgeInsets.symmetric(
          //       horizontal: Space.s400, vertical: Space.s100),
          //   child: ,
          // ),

          const SizedBox(height: Space.s200),

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
                LandingCarouselIndicators(
                    selectedPageIndex: ValueNotifier(0), numPages: 2),
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
          )
        ],
      ),
    );
  }
}

class LnInvoiceCard extends StatelessWidget {
  const LnInvoiceCard({super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.s200),
      child: Container(
        decoration: BoxDecoration(
          color: LxColors.grey1000,
          borderRadius: BorderRadius.circular(LxRadius.r300),
        ),
        clipBehavior: Clip.antiAlias,
        padding:
            const EdgeInsets.fromLTRB(Space.s600, Space.s500, Space.s600, 0),
        // Space.s600, Space.s200, Space.s600, 0),
        // constraints: const BoxConstraints(minWidth: 300, maxWidth: 300),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Expanded(
                  child: Text(
                    "Lightning invoice",
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
                value:
                    "lnbcrt2234660n1pjg7xnqxq8pjg7stspp5sq0le60mua87e3lvd7njw9khmesk0nzkqa34qc4jg7tm2num5jlqsp58p4rswtywdnx5wtn8pjxv6nnvsukv6mdve4xzernd9nx5mmpv35s9qrsgqdqhg35hyetrwssxgetsdaekjaqcqpcnp4q0tmlmj0gdeksm6el92s4v3gtw2nt3fjpp7czafjpfd9tgmv052jshcgr3e64wp4uum2c336uprxrhl34ryvgnl56y2usgmvpkt0xajyn4qfvguh7fgm6d07n00hxcrktmkz9qnprr3gxlzy2f4q9r68scwsp5d6f6r",
                // value:
                //     "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U?amount=0.00001&label=sbddesign%3A%20For%20lunch%20Tuesday&message=For%20lunch%20Tuesday&lightning=LNBC10U1P3PJ257PP5YZTKWJCZ5FTL5LAXKAV23ZMZEKAW37ZK6KMV80PK4XAEV5QHTZ7QDPDWD3XGER9WD5KWM36YPRX7U3QD36KUCMGYP282ETNV3SHJCQZPGXQYZ5VQSP5USYC4LK9CHSFP53KVCNVQ456GANH60D89REYKDNGSMTJ6YW3NHVQ9QYYSSQJCEWM5CJWZ4A6RFJX77C490YCED6PEMK0UPKXHY89CMM7SCT66K8GNEANWYKZGDRWRFJE69H9U5U0W57RRCSYSAS7GADWMZXC8C6T0SPJAZUP6",
                // value: "bitcoin:BC1QYLH3U67J673H6Y6ALV70M0PL2YZ53TZHVXGG7U",
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
      ),
    );
  }
}

class BtcAddressCard extends StatelessWidget {
  const BtcAddressCard({super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.s200),
      child: Container(
        decoration: BoxDecoration(
          color: LxColors.grey1000,
          borderRadius: BorderRadius.circular(LxRadius.r300),
        ),
        clipBehavior: Clip.antiAlias,
        padding:
            const EdgeInsets.fromLTRB(Space.s600, Space.s500, Space.s600, 0),
        // Space.s600, Space.s200, Space.s600, 0),
        // constraints: const BoxConstraints(minWidth: 300, maxWidth: 300),
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
      ),
    );
  }
}

class LandingCarouselIndicators extends StatelessWidget {
  const LandingCarouselIndicators({
    super.key,
    required this.selectedPageIndex,
    required this.numPages,
  });

  final int numPages;
  final ValueListenable<int> selectedPageIndex;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: List<Widget>.generate(
          this.numPages,
          (index) => LandingCarouselIndicator(
              index: index, selectedPageIndex: this.selectedPageIndex)),
    );
  }
}

class LandingCarouselIndicator extends StatelessWidget {
  const LandingCarouselIndicator({
    super.key,
    required this.index,
    required this.selectedPageIndex,
  });

  final int index;
  final ValueListenable<int> selectedPageIndex;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: Space.s100),
      child: ValueListenableBuilder(
        valueListenable: this.selectedPageIndex,
        builder: (context, selectedPageIndex, child) {
          final isActive = selectedPageIndex == this.index;

          return AnimatedContainer(
            duration: const Duration(milliseconds: 250),
            height: 6.0,
            width: isActive ? 20 : 6,
            decoration: BoxDecoration(
              color: isActive ? LxColors.clearB600 : LxColors.clearB200,
              borderRadius: const BorderRadius.all(Radius.circular(12)),
            ),
          );
        },
      ),
    );
  }
}
