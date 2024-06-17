// Page for showing a QR code

import 'dart:async' show unawaited;
import 'dart:math';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart'
    show EccLevel, Encode, EncodeParams, Format, zx;

import 'package:lexeapp/components.dart'
    show LxCloseButton, ScrollableSinglePageBody;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/style.dart' show LxColors, Space;

/// Encode `value` as a QR image and then display it in `dimension` pixels
/// width and height.
class QrImage extends StatefulWidget {
  const QrImage({
    super.key,
    required this.value,
    required this.dimension,
    this.color = LxColors.grey0,
  });

  final String value;
  final Color color;
  final int dimension;

  @override
  State<QrImage> createState() => _QrImageState();
}

class _QrImageState extends State<QrImage> {
  /// The encoded QR image, ready to be rendered. Must be `.dispose()`'d.
  ui.Image? qrImage;

  /// The number of empty pixels in `qrImage` until the QR actually starts.
  /// We'll expand the final rendered image so the image actually fits snugly
  /// in `dimension` pixels.
  int? scrimSize;

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
        // NOTE: even though margin is set to zero, we still get non-zero empty
        // margin in the encoded image that we need to deal with.
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

      // Compute the number of empty pixels until the QR image content actually
      // starts.
      final scrimSize = qrScrimSize(data, dimension);

      ui.decodeImageFromPixels(
        data,
        dimension,
        dimension,
        ui.PixelFormat.rgba8888,
        allowUpscaling: false,
        (qrImage) => this.setQRImage(qrImage, scrimSize),
      );
    } else {
      error(
          "Failed to encode QR image: ${encodeResult.error}, dim: $dimension, value: '$value'");
    }
  }

  void setQRImage(ui.Image qrImage, int scrimSize) {
    if (!this.mounted) {
      qrImage.dispose();
      return;
    }

    this.setState(() {
      this.maybeDisposeQRImage();
      this.qrImage = qrImage;
      this.scrimSize = scrimSize;
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
  void didUpdateWidget(QrImage old) {
    super.didUpdateWidget(old);
    final QrImage new_ = this.widget;
    if (new_.value != old.value ||
        new_.dimension != old.dimension ||
        new_.color != old.color) {
      unawaited(encodeQrImage());
    }
  }

  @override
  Widget build(BuildContext context) {
    final qrImage = this.qrImage;
    final dimensionInt = this.widget.dimension;
    final dimension = dimensionInt.toDouble();

    if (qrImage != null) {
      // Scale up the image by factor `scale` in order to make the QR image
      // fully fit inside `dimension` pixels without any extra margin pixels.
      final scrimSize = this.scrimSize!;
      final dimWithoutScrim = (dimensionInt - (scrimSize * 2)).toDouble();
      final scale = dimWithoutScrim / dimension;

      return RawImage(
        image: qrImage,
        width: dimension,
        height: dimension,
        color: this.widget.color,
        filterQuality: FilterQuality.none,
        isAntiAlias: true,
        // These three parameters work together to "cut off" the empty scrim
        // around the QR image and make it fully fit inside `dimension` pixels.
        scale: scale,
        fit: BoxFit.none,
        alignment: Alignment.center,
        // * The normal black values in the QR image are actually
        //   transparent black (0x00000000) while the white values are
        //   opaque white (0xffffffff).
        // * To show the black parts of the QR with our chosen `color` while
        //   leaving the white parts transparent, we can use the `srcOut`
        //   blend mode.
        colorBlendMode: BlendMode.srcOut,
      );
    } else {
      return SizedBox.square(dimension: dimension);
    }
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
        leading: const LxCloseButton(isLeading: true),
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

/// A small helper that makes a QR image interactive with taps and long presses.
///
/// Normally, we just wrap a [QrImage] in an [InkWell] and call it a day, but
/// since the image is opaque, the animated ink splash in the background doesn't
/// show properly. This helper widget makes everything work as expected.
class InteractiveQrImage extends StatelessWidget {
  const InteractiveQrImage({
    super.key,
    required this.value,
    required this.dimension,
    this.color = LxColors.grey0,
    this.onTap,
    this.onLongPress,
  });

  final String value;
  final Color color;
  final int dimension;

  final VoidCallback? onTap;
  final VoidCallback? onLongPress;

  @override
  Widget build(BuildContext context) {
    // Need to draw Material+Ink splasher on top of image, so splash animation
    // doesn't get occluded by opaque image.
    return Stack(
      children: [
        QrImage(value: this.value, dimension: this.dimension),
        Material(
          type: MaterialType.transparency,
          child: InkWell(
            onTap: this.onTap,
            onLongPress: this.onLongPress,
            enableFeedback: true,
            splashColor: LxColors.clearW300,
            child: SizedBox.square(dimension: this.dimension.toDouble()),
          ),
        ),
      ],
    );
  }
}

/// Compute the "scrim" size (a.k.a. empty margin) around the QR image in `data`
///
/// For example, in the below ASCII QR image:
///
///     "." == 0xffffffff
///     "■" == 0x00000000
///
/// ```
/// .........................................................................
/// .........................................................................
/// .........................................................................
/// ...■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■......■■■■■■■■■■■■......■■■■
/// ...■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■......■■■■■■■■■■■■......■■■■
/// ...■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■......■■■■■■■■■■■■......■■■■
/// ...■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■......■■■■■■■■■■■■......■■■■
/// ...■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■......■■■■■■■■■■■■......■■■■
/// ...■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■......■■■■■■■■■■■■......■■■■
/// ...■■■■■■..............................■■■■■■......■■■■■■■■■■■■■■■■■■....
/// ...■■■■■■..............................■■■■■■......■■■■■■■■■■■■■■■■■■....
/// ...■■■■■■..............................■■■■■■......■■■■■■■■■■■■■■■■■■....
/// ...■■■■■■..............................■■■■■■......■■■■■■■■■■■■■■■■■■.... . . .
/// ...■■■■■■..............................■■■■■■......■■■■■■■■■■■■■■■■■■....
/// ...■■■■■■..............................■■■■■■......■■■■■■■■■■■■■■■■■■....
/// ...■■■■■■......■■■■■■■■■■■■■■■■■■......■■■■■■......■■■■■■......■■■■■■....
/// ...■■■■■■......■■■■■■■■■■■■■■■■■■......■■■■■■......■■■■■■......■■■■■■....
/// ...■■■■■■......■■■■■■■■■■■■■■■■■■......■■■■■■......■■■■■■......■■■■■■....
/// ...■■■■■■......■■■■■■■■■■■■■■■■■■......■■■■■■......■■■■■■......■■■■■■....
/// ...■■■■■■......■■■■■■■■■■■■■■■■■■......■■■■■■......■■■■■■......■■■■■■....
/// ...■■■■■■......■■■■■■■■■■■■■■■■■■......■■■■■■......■■■■■■......■■■■■■....
///                        .                                .
///                        .                                .
/// ```
///
/// We can see the "scrim" size is 3 empty pixels until the actual QR image
/// starts.
int qrScrimSize(final Uint8List data, final int dimension) {
  const int bytesPerPixel = 4;
  // Sanity check our QR image has the correct dimensions.
  assert(data.length == bytesPerPixel * dimension * dimension);

  // Give up searching for the scrim end after 50 px or `dimension/4` px.
  final searchMaxPx = min(dimension >> 2, 50);

  final bytesPerRow = dimension * bytesPerPixel;

  // Walk down diagonally from the top-left corner until we find the start of
  // the QR image (not a 0xff byte).
  //
  // We look for !0xff, as the image is RGBA encoded and opacity inverted
  // (thanks flutter_zxing...), so 0xffffffff is an opaque white background
  // pixel and 0x00000000 is a transparent black foreground pixel.
  //
  // Thus the actual QR image starts wherever we find the first black pixel
  // (the first pixel byte != 0xff).
  for (var px = 0; px < searchMaxPx; px++) {
    final rowOffsetBytes = px * bytesPerRow;
    final colOffsetBytes = px * bytesPerPixel;
    final imgOffsetBytes = rowOffsetBytes + colOffsetBytes;
    if (data[imgOffsetBytes] != 0xff) {
      return px;
    }
  }

  return searchMaxPx;
}

// /// Use this to sanity check the output of flutter_zxing...
// void printQr(final Uint8List data, final int dimension) {
//   info("QrImage: dim: $dimension, bytes: ${data.length}");
//   var s = StringBuffer();
//   for (var row = 0; row < 60; row++) {
//     final rowIdx = row * (dimension * 4);
//     for (var col = 0; col < 115; col++) {
//       final colIdx = rowIdx + (col * 4);
//       final b = data[colIdx + 0] == 0x00;
//       s.write(b ? "■" : ".");
//     }
//     info(s.toString());
//     s.clear();
//   }
// }
