// Page for scanning QR codes / barcodes

import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart'
    show FixedScannerOverlay, ReaderWidget;

import 'package:lexeapp/components.dart' show LxCloseButton;
import 'package:lexeapp/logger.dart' show info;
import 'package:lexeapp/style.dart' show LxColors, LxRadius, LxTheme, Space;

class ScanPage extends StatelessWidget {
  const ScanPage({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      extendBodyBehindAppBar: true,
      appBar: AppBar(
        // transparent bg header
        backgroundColor: LxColors.clearB0,
        scrolledUnderElevation: 0.0,
        surfaceTintColor: LxColors.clearB0,

        // X - quit scanning
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),

        // * Make the top status bar transparent, so the whole screen includes
        //   the camera view.
        // * Make the bottom nav thing `foreground` instead of black.
        systemOverlayStyle: LxTheme.systemOverlayStyleLight.copyWith(
          statusBarColor: LxColors.clearW0,
          systemNavigationBarColor: LxColors.foreground,
          systemNavigationBarDividerColor: LxColors.foreground,
        ),
      ),
      // TODO(phlip9): just show a file picker or something for non-mobile
      //               OS like macOS, linux, windows.
      // We're waiting on the flutter `camera` pkg to support desktop OS's.
      body: ReaderWidget(
        onScan: (barcode) {
          info("barcode: '${barcode.text}'");
        },
        showFlashlight: false,
        showToggleCamera: false,
        cropPercent: 0.50,
        actionButtonsAlignment: Alignment.bottomCenter,
        actionButtonsPadding: const EdgeInsets.all(Space.s600),
        loading: const DecoratedBox(
          decoration: BoxDecoration(color: LxColors.foreground),
          child: Center(),
        ),
        scannerOverlay: const FixedScannerOverlay(
          borderColor: LxColors.grey975,
          // grey900 x clear700
          overlayColor: Color(0xb2eff3f5),
          borderRadius: LxRadius.r400,
          borderLength: 120.0,
          borderWidth: 8.0,
          cutOutSize: 240.0,
        ),
      ),
    );
  }
}
