/// Pages for showing a QR code image.
///
/// + [QrImage] just shows a static image from a QR code string.
/// + [InteractiveQrImage] wraps a [QrImage] and adds useful interactive things,
///   like tap to open fullscreen, or long press to copy the code string.
library;

import 'dart:async' show unawaited;
import 'dart:convert' show utf8;
import 'dart:ui' as ui;

import 'package:app_rs_dart/ffi/qr.dart' as qr;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show LxCloseButton, ScrollableSinglePageBody;
import 'package:lexeapp/style.dart' show LxColors, Space;

/// Encode `value` as a QR image and then display it in `dimension` pixels
/// width and height.
class QrImage extends StatelessWidget {
  const QrImage({super.key, required this.value, required this.dimension});

  final String value;
  final double dimension;

  @override
  Widget build(BuildContext context) {
    return Image(
      image: QrImageProvider.utf8(this.value),
      width: dimension,
      height: dimension,
      filterQuality: FilterQuality.none,
      isAntiAlias: false,
      fit: BoxFit.none,
      alignment: Alignment.center,
      // colorBlendMode: BlendMode.dst,
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
  final double dimension;

  @override
  State<InteractiveQrImage> createState() => _InteractiveQrImageState();
}

class _InteractiveQrImageState extends State<InteractiveQrImage> {
  /// Open the QR image in a new fullscreen modal dialog.
  void openQrPage() {
    unawaited(
      showDialog(
        context: this.context,
        builder: (_) => FullscreenQrDialog(value: this.widget.value),
      ),
    );
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
          QrImage(value: this.widget.value, dimension: this.widget.dimension),
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
  const FullscreenQrDialog({super.key, required this.value});

  final String value;

  @override
  Widget build(BuildContext context) {
    return Dialog(
      insetPadding: EdgeInsets.zero,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.zero),
      backgroundColor: LxColors.grey1000,
      // Need padding around fullscreen QR image for quiet zone to improve
      // scan-ability.
      child: Padding(
        padding: const EdgeInsets.all(Space.s500),
        child: LayoutBuilder(
          builder: (context, constraints) => QrImage(
            value: this.value,
            // The largest square QR image we can show, within reasonable constraints.
            dimension: clampDouble(
              constraints.biggest.shortestSide,
              200.0,
              500.0,
            ),
          ),
        ),
      ),
    );
  }
}

/// Design mode page for testing basic QR display.
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
          Center(child: QrImage(value: this.value, dimension: 300)),
        ],
      ),
    );
  }
}

/// An [ImageProvider] that generates a QR code image.
///
/// + The image is decoded to fit the target box.
/// + Using an [ImageProvider] ensures the decoded image is cached in the
///   global image cache.
class QrImageProvider extends ImageProvider<QrImageKey> {
  /// Create a new [QrImageProvider] from bytes.
  const QrImageProvider(this.value);

  /// Create a new [QrImageProvider] from a UTF-8 encoded string.
  QrImageProvider.utf8(final String value) : this.value = utf8.encode(value);

  /// The bytes that will be encoded into a QR code image.
  final Uint8List value;

  /// Generate the cache key for the QR image that uniquely identifies it in the
  /// global image cache.
  ///
  /// The `configuration` parameter may contain the target size of final decoded
  /// image. Since we decode to this size, it must also be included in the cache
  /// key.
  @override
  Future<QrImageKey> obtainKey(ImageConfiguration configuration) {
    final len = this.value.length;
    final size = configuration.size;

    // Compute the scale factor to decode the QR image to the correct target size.
    // Flutter already appears to scale the base image as if reading logical
    // pixels, so no need to account for device pixel ratio.
    final scale = (size != null)
        ? qr.encodedPixelsPerSide(dataLenBytes: len).toDouble() /
              size.shortestSide
        : 1.0;

    return SynchronousFuture<QrImageKey>(
      QrImageKey(value: this.value, scale: scale),
    );
  }

  @override
  ImageStreamCompleter loadImage(QrImageKey key, ImageDecoderCallback decode) {
    // Technically this could use `OneFrameImageStreamCompleter` but it seems
    // more complicated...
    return MultiFrameImageStreamCompleter(
      // Generate the QR image and decode it to the target size.
      codec: qr
          .encode(data: key.value)
          .then<ui.ImmutableBuffer>(ui.ImmutableBuffer.fromUint8List)
          .then<ui.Codec>(decode),
      scale: key.scale,
      debugLabel: "QrImageProvider",
    );
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is QrImageProvider &&
            (identical(other.value, this.value) || other.value == this.value));
  }

  @override
  int get hashCode => Object.hash(this.runtimeType, this.value);
}

/// Uniquely identifies a QR image in the global image cache.
@immutable
class QrImageKey {
  const QrImageKey({required this.value, required this.scale});

  /// The UTF-8 encoded string that will be encoded into a QR code image.
  final Uint8List value;

  /// The scale factor to decode the QR image to the correct target size.
  final double scale;

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        (other.runtimeType == this.runtimeType &&
            other is QrImageKey &&
            (identical(other.value, this.value) || other.value == this.value) &&
            (other.scale == this.scale));
  }

  @override
  int get hashCode => Object.hash(this.runtimeType, this.value, this.scale);
}
