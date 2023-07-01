// Page for showing a QR code

import 'dart:async' show unawaited;
import 'dart:typed_data' show Uint32List, Uint8List;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart' show Encode, EncodeParams, zx;
import 'package:lexeapp/logger.dart' show error;

import '../../style.dart' show LxColors;

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

  Future<void> encodeQrImage() async {
    final value = this.widget.value;
    final dimension = this.widget.dimension;

    final Encode encodeResult = zx.encodeBarcode(
      contents: value,
      params: EncodeParams(
        width: dimension,
        height: dimension,
      ),
    );

    if (encodeResult.isValid) {
      // image data is in RGBA format (i.e., each byte: 0xAABBGGRR)
      final Uint32List dataU32 = encodeResult.data!;

      // * Annoyingly, the normal black values in the base image are actually
      //   transparent black (0x00000000) while the white values are opaque
      //   white (0xffffffff).
      // * We can work around this with the `srcOut` blend mode when displaying
      //   the image.

      final Uint8List dataU8 = Uint8List.view(dataU32.buffer);

      ui.decodeImageFromPixels(
        dataU8,
        dimension,
        dimension,
        ui.PixelFormat.rgba8888,
        (imageResult) {
          this.setState(() {
            this.qrImage = imageResult;
          });
        },
      );
    } else {
      error(
          "Failed to encode QR image: ${encodeResult.error}, value: $value, dim: $dimension");
    }
  }

  @override
  void initState() {
    super.initState();
    unawaited(encodeQrImage());
  }

  @override
  void dispose() {
    super.dispose();
    if (this.qrImage != null) {
      this.qrImage!.dispose();
    }
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
        automaticallyImplyLeading: false,
        leading: const CloseButton(),
      ),
      body: Center(
        child: QrImage(
          value: this.value,
          dimension: 300,
          color: LxColors.foreground,
        ),
      ),
    );
  }
}
