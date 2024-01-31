// Page for showing a QR code

import 'dart:async' show unawaited;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart'
    show EccLevel, Encode, EncodeParams, Format, zx;

import '../components.dart' show LxCloseButton, ScrollableSinglePageBody;
import '../logger.dart';
import '../style.dart' show LxColors, Space;

class QrImage extends StatefulWidget {
  const QrImage({
    super.key,
    required this.value,
    required this.dimension,
    this.color = const Color(0xff000000),
  });

  final String value;
  final Color color;
  final int dimension;

  @override
  State<QrImage> createState() => _QrImageState();
}

class _QrImageState extends State<QrImage> {
  ui.Image? qrImage;

  @override
  void initState() {
    super.initState();
    unawaited(encodeQrImage());
  }

  Future<void> encodeQrImage() async {
    final value = this.widget.value;
    final dimension = this.widget.dimension;

    final Encode encodeResult = zx.encodeBarcode(
      contents: value,
      params: EncodeParams(
        width: dimension,
        height: dimension,
        margin: 0,
        format: Format.qrCode,
        eccLevel: EccLevel.medium,
      ),
    );

    if (encodeResult.isValid) {
      // The image data is in RGBA format. With a standard black-and-white QR
      // code in mind, the colors here are "black" == 0x00000000 and
      // "white" == 0xffffffff.
      final data = encodeResult.data!.buffer.asUint8List();
      ui.decodeImageFromPixels(
        data,
        dimension,
        dimension,
        ui.PixelFormat.rgba8888,
        allowUpscaling: false,
        this.setQRImage,
      );
    } else {
      error(
          "Failed to encode QR image: ${encodeResult.error}, value: $value, dim: $dimension");
    }
  }

  void setQRImage(ui.Image qrImage) {
    if (!this.mounted) {
      qrImage.dispose();
      return;
    }

    this.setState(() {
      this.maybeDisposeQRImage();
      this.qrImage = qrImage;
    });
  }

  void maybeDisposeQRImage() {
    if (this.qrImage != null) {
      this.qrImage!.dispose();
      this.qrImage = null;
    }
  }

  @override
  void dispose() {
    this.maybeDisposeQRImage();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final dimension = this.widget.dimension.toDouble();

    return (this.qrImage != null)
        ? RawImage(
            image: this.qrImage,
            width: dimension,
            height: dimension,
            color: this.widget.color,
            isAntiAlias: true,
            // * The normal black values in the QR image are actually
            //   transparent black (0x00000000) while the white values are
            //   opaque white (0xffffffff).
            // * To show the black parts of the QR with our chosen `color` while
            //   leaving the white parts transparent, we can use the `srcOut`
            //   blend mode.
            colorBlendMode: BlendMode.srcOut,
          )
        : SizedBox.square(dimension: dimension);
  }
}

class ShowQrPage extends StatelessWidget {
  const ShowQrPage({super.key, required this.value});

  final String value;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxCloseButton(),
      ),
      body: ScrollableSinglePageBody(
        body: [
          const SizedBox(height: Space.s900),
          Center(
            child: QrImage(
              value: this.value,
              dimension: 300,
              color: LxColors.foreground,
            ),
          )
        ],
      ),
    );
  }
}
