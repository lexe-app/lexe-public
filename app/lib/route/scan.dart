// Page for scanning QR codes / barcodes

import 'package:flutter/material.dart';
// import 'package:lexeapp/logger.dart' show info;
// import 'package:mobile_scanner/mobile_scanner.dart' show MobileScanner;

import '../../style.dart' show LxColors;

class ScanPage extends StatelessWidget {
  const ScanPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      extendBodyBehindAppBar: true,
      appBar: AppBar(
        automaticallyImplyLeading: false,

        // transparent bg header
        backgroundColor: LxColors.clearB0,
        scrolledUnderElevation: 0.0,
        surfaceTintColor: LxColors.clearB0,

        // X - quit scanning
        leading: const CloseButton(),
      ),
      body: const Placeholder(),
      // body: MobileScanner(
      //   onDetect: (capture) {
      //     info("new scanner capture: ${capture.barcodes.length} barcodes:");
      //     for (final barcode in capture.barcodes) {
      //       info("  barcode:");
      //       info("    type: ${barcode.type}");
      //       info("    value: ${barcode.rawValue}");
      //     }
      //   },
      // ),
    );
  }
}
