/// Pages for showing a QR code image.
///
/// + [QrImage] just shows a static image from a QR code string.
/// + [InteractiveQrImage] wraps a [QrImage] and adds useful interactive things,
///   like tap to open fullscreen, or long press to copy the code string.
library;

import 'dart:async' show Completer, unawaited;
import 'dart:developer' as dev;
import 'dart:isolate';
import 'dart:math';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_zxing/flutter_zxing.dart' as zx;

import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show LxCloseButton, ScrollableSinglePageBody;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart' show LxColors, Space;

/// An encoded QR image ready to be displayed.
///
/// See [QrImageEncoder.encode].
class EncodedQrImage {
  const EncodedQrImage({
    required this.value,
    required this.dimension,
    required this.image,
    required this.scrimSize,
  });

  /// The string encoded in the QR image.
  final String value;

  /// The width and height of the QR image, in pixels.
  final int dimension;

  /// The encoded QR image, ready to be rendered. Must be `.dispose()`'d.
  final ui.Image image;

  /// The number of empty pixels in `qrImage` until the QR actually starts.
  /// We'll expand the final rendered image so the image actually fits snugly
  /// in `dimension` pixels.
  final int scrimSize;

  /// Holders must call [dispose] in order to free the inner [ui.Image].
  void dispose() {
    this.image.dispose();
  }

  // TODO(phlip9): use this when we can finally copy images to the clipboard
  // /// Transcode the current QR [ui.Image] to png-encoded bytes.
  // Future<Result<ByteData, Exception>> toPngBytes() async =>
  //     (await Result.tryAsync<ByteData?, Exception>(
  //       () => this.image.toByteData(format: ui.ImageByteFormat.png),
  //     ))
  //         .map((data) => data!);
}

/// Encode `value` as a QR image and then display it in `dimension` pixels
/// width and height.
class QrImage extends StatefulWidget {
  const QrImage({
    super.key,
    required this.value,
    required this.dimension,
    this.withScrim = false,
  });

  final String value;
  final int dimension;

  /// If true, include the built-in white margin around the image, called
  /// "scrim". Can make it hard to visually align the image with nearby
  /// elements.
  final bool withScrim;

  @override
  State<QrImage> createState() => _QrImageState();
}

class _QrImageState extends State<QrImage> {
  EncodedQrImage? qrImage;

  @override
  void initState() {
    super.initState();
    unawaited(this.initEncodeQrImage());
  }

  Future<void> initEncodeQrImage() async {
    final value = this.widget.value;
    final dimension = this.widget.dimension;

    switch (await QrImageEncoder.encode(value, dimension)) {
      case Ok(:final ok):
        this.setQRImage(ok);
      case Err(:final err):
        error("QrImage: $err");
    }
  }

  void setQRImage(final EncodedQrImage qrImage) {
    // We could get a stale `initEncodeQrImage` result that resolves after the
    // widget params have changed. Just ignore them (but remember to free!).
    final isStaleResult = qrImage.value != this.widget.value ||
        qrImage.dimension != this.widget.dimension;

    if (!this.mounted || isStaleResult) {
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
  void didUpdateWidget(QrImage old) {
    super.didUpdateWidget(old);
    final QrImage new_ = this.widget;
    if (new_.value != old.value || new_.dimension != old.dimension) {
      // TODO(phlip9): fix race
      unawaited(this.initEncodeQrImage());
    }
  }

  @override
  Widget build(BuildContext context) {
    final qrImage = this.qrImage;
    final dimensionInt = this.widget.dimension;
    final dimension = dimensionInt.toDouble();

    if (qrImage != null) {
      // If we're removing the scrim, then scale up the image by a factor in
      // order to make the QR image fully fit inside `dimension` pixels without
      // any extra margin pixels.
      // final scale = switch (this.widget.withScrim) {
      //   false => 1.0,
      //   true => (dimensionInt - (qrImage.scrimSize * 2)).toDouble() / dimension,
      // };
      // final scale = 1.0;

      double scale = 1.0;
      if (!this.widget.withScrim) {
        final scrimSize = qrImage.scrimSize;
        final dimWithoutScrim = (dimensionInt - (scrimSize * 2)).toDouble();
        scale = dimWithoutScrim / dimension;
      }

      return RawImage(
        image: qrImage.image,
        width: dimension,
        height: dimension,
        filterQuality: FilterQuality.none,
        isAntiAlias: true,
        // These three parameters work together to "cut off" the empty scrim
        // around the QR image and make it fully fit inside `dimension` pixels.
        scale: scale,
        fit: BoxFit.none,
        alignment: Alignment.center,
        colorBlendMode: BlendMode.dst,
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
            ),
          )
        ],
      ),
    );
  }
}

/// A small helper that makes a QR image interactive. Tapping the QR image will
/// open it in a fullscreen page. Long pressing will copy the code string to the
/// clipboard.
class InteractiveQrImage extends StatefulWidget {
  const InteractiveQrImage({
    super.key,
    required this.value,
    required this.dimension,
  });

  final String value;
  final int dimension;

  @override
  State<InteractiveQrImage> createState() => _InteractiveQrImageState();
}

class _InteractiveQrImageState extends State<InteractiveQrImage> {
  final GlobalKey<_QrImageState> qrImageKey = GlobalKey();

  /// Open the QR image in a new fullscreen modal dialog.
  void openQrPage() {
    unawaited(showDialog(
      context: this.context,
      builder: (_) => FullscreenQrDialog(value: this.widget.value),
    ));
  }

  @override
  Widget build(BuildContext context) {
    // Need to draw Material+Ink splasher on top of image, so splash animation
    // doesn't get occluded by opaque image.
    return SizedBox.square(
      dimension: this.widget.dimension.toDouble(),
      child: Stack(
        children: [
          // Draw the QR below the splasher
          QrImage(
            key: this.qrImageKey,
            value: this.widget.value,
            dimension: this.widget.dimension,
          ),
          // Interactive splasher material
          Material(
            type: MaterialType.transparency,
            child: InkWell(
              onTap: this.openQrPage,
              onLongPress: () =>
                  LxClipboard.copyTextWithFeedback(context, this.widget.value),
              enableFeedback: true,
              splashColor: LxColors.clearW300,
            ),
          ),
        ],
      ),
    );
  }
}

/// A fullscreen modal QR image. Generally used with [showDialog].
class FullscreenQrDialog extends StatelessWidget {
  const FullscreenQrDialog({
    super.key,
    required this.value,
  });

  final String value;

  @override
  Widget build(BuildContext context) {
    return Dialog(
      insetPadding: EdgeInsets.zero,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.zero),
      child: LayoutBuilder(
        builder: (context, constraints) => QrImage(
          value: this.value,
          // The largest square QR image we can show, within reasonable constraints.
          dimension: clampDouble(constraints.biggest.shortestSide, 200.0, 500.0)
              .toInt(),
          withScrim: true,
        ),
      ),
    );
  }
}

abstract final class QrImageEncoder {
  /// Encode [value] as a QR code image with a width and height of [dimension]
  /// pixels. Runs most of the encoding in a separate [Isolate] to avoid
  /// blocking the main UI thread.
  static Future<Result<EncodedQrImage, Exception>> encode(
    final String value,
    final int dimension,
  ) async {
    // In debug/profile mode, add timeline events.
    dev.TimelineTask? dbgTask;
    if (!cfg.release) {
      dbgTask = dev.TimelineTask()..instant("QR encode");
      dbgTask.start("ui -> isolate");
    }

    try {
      // Run `zx.encodeBarcode` in a separate isolate to avoid blocking the UI
      // isolate as much as possible.
      // Sadly, we can't also `ui.decodeImageFromPixels` in the isolate, since
      // that seems to crash for some reason.
      final (data, scrimSize) = await Isolate.run(
        debugName: "QR encode (isolate)",
        () async {
          dbgTask?.finish();

          // Use `flutter_zxing` to do the code -> image encoding. We'll need to
          // fix it up after though.
          dbgTask?.start("zx.encodeBarcode");
          final zx.Encode encodeResult = zx.zx.encodeBarcode(
            contents: value,
            params: zx.EncodeParams(
              width: dimension,
              height: dimension,
              // NOTE: even though margin is set to zero, we still get non-zero empty
              // margin in the encoded image that we need to deal with.
              margin: 0,
              format: zx.Format.qrCode,
              eccLevel: zx.EccLevel.medium,
            ),
          );
          dbgTask?.finish();

          if (!encodeResult.isValid) {
            throw Exception(
                "Failed to encode QR image: ${encodeResult.error}, dim: $dimension, value: '$value'");
          }

          // The image data is in RGBA format. With a standard black-and-white QR
          // code in mind, the colors here are "black" == 0x00000000 and
          // "white" == 0xffffffff.
          final data = encodeResult.data!.buffer;

          // Compute the number of empty pixels until the QR image content actually
          // starts.
          final scrimSize = qrScrimSize(data.asUint8List(), dimension);

          // Recolor the QR image so that colors are opaque and the black pixels
          // are [LxColors.foreground] instead.
          dbgTask?.start("recolorQrImage");
          recolorQrImage(data.asUint32List());
          dbgTask?.finish();

          dbgTask?.start("isolate -> ui");
          return (data, scrimSize);
        },
      );
      dbgTask?.finish();

      // Flutter needs a `ui.Image` decoded from our raw QR image bytes.
      dbgTask?.start("ui.decodeImageFromPixels");
      final completer = Completer<ui.Image>();
      ui.decodeImageFromPixels(
        data.asUint8List(),
        dimension,
        dimension,
        ui.PixelFormat.rgba8888,
        allowUpscaling: false,
        completer.complete,
      );
      final image = await completer.future;
      dbgTask?.finish();

      return Ok(EncodedQrImage(
        value: value,
        dimension: dimension,
        image: image,
        scrimSize: scrimSize,
      ));
    } on Exception catch (err) {
      dbgTask?.finish();
      return Err(err);
    }
  }
}

/// Modifies in-place the encoded QR image bytes from `zx.encodeBarcode` so that
/// the image has opaque pixel values in RGBA format, and the "black" QR pixels
/// are [LxColors.foreground].
///
/// It's really just this / transformation for each pixel:
///
/// (little endian)
///                 AABBGGRR
/// 0x00000000 => 0xff23211c
/// 0xffffffff => 0xffffffff
///
/// (big endian)
///                 RRGGBBAA
/// 0x00000000 => 0x1c2123ff
/// 0xffffffff => 0xffffffff
void recolorQrImage(final Uint32List data) {
  final int foreground =
      (Endian.host == Endian.little) ? 0xff23211c : 0x1c2123ff;
  for (int idx = 0; idx < data.length; idx += 1) {
    data[idx] |= foreground;
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
