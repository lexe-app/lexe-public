/// Reusable flutter UI components

import 'dart:async' show StreamController;

import 'package:flutter/material.dart';
import 'package:rxdart_ext/rxdart_ext.dart';

import '../../style.dart' show LxColors, LxRadius, Space;

typedef VoidContextCallback = void Function(BuildContext);

/// A more robust body for a [Scaffold]. Use this widget when you expect the
/// body area to almost always be in view, but can gracefully handle smaller
/// viewports (like when the onscreen keyboard pops up).
///
/// * An optional `bottom` widget is available that will expand to fill as much
///   of the remaining viewport as possible. Use this if you want to e.g. anchor
///   some buttons to the bottom of the screen, but still have them scroll with
///   the body when the viewport is small.
///
///   This behavior contrasts with e.g. a TabBar, which instead stays fixed to
///   the bottom, is always visible on top, and forces body content to scroll
///   underneath.
///
/// NOTE(phlip9): There seem to be multiple ways to accomplish this and I'm not
/// really sure which is "best".
class ScrollableSinglePageBody extends StatelessWidget {
  const ScrollableSinglePageBody({
    super.key,
    required this.body,
    this.padding = const EdgeInsets.symmetric(horizontal: Space.s400),
    this.bottom,
    this.bottomAlignment = Alignment.bottomCenter,
    this.bottomPadding = const EdgeInsets.only(bottom: Space.s600),
  });

  final List<Widget> body;
  final EdgeInsets padding;
  final Widget? bottom;
  final Alignment bottomAlignment;
  final EdgeInsets bottomPadding;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: this.padding,
      child: CustomScrollView(
        primary: true,
        slivers: [
          // The primary body widgets.
          SliverList.list(children: this.body),

          // The bottom widgets; these expand to fill the available space.
          if (this.bottom != null)
            SliverFillRemaining(
              hasScrollBody: false,
              child: Align(
                alignment: this.bottomAlignment,
                child: Padding(
                  padding: this.bottomPadding,
                  child: this.bottom,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A simple colored box that we can show while some real content is loading.
///
/// The `width` and `height` are optional. If left `null`, that dimension will
/// be determined by the parent `Widget`'s constraints.
///
/// If the placeholder is replacing some text, `forText` should be set to `true`.
/// This is because a `Text` widget's actual rendered height also depends on the
/// current `MediaQuery.textScaleFactor`.
class FilledPlaceholder extends StatelessWidget {
  const FilledPlaceholder({
    super.key,
    this.color = LxColors.grey850,
    this.width = double.infinity,
    this.height = double.infinity,
    this.borderRadius = LxRadius.r200,
    this.forText = false,
    this.child,
  });

  final Color color;
  final double width;
  final double height;
  final double borderRadius;
  final bool forText;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    final double heightFactor;
    if (!this.forText) {
      heightFactor = 1.0;
    } else {
      heightFactor = MediaQuery.of(context).textScaleFactor;
    }

    return SizedBox(
      width: this.width,
      height: this.height * heightFactor,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: this.color,
          borderRadius: BorderRadius.circular(this.borderRadius),
        ),
        child: this.child,
      ),
    );
  }
}

enum LxCloseButtonKind {
  closeFromTop,
  closeFromRoot,
  closeDrawer,
}

/// × - Close button, usually placed on the [AppBar].
///
/// Example usage:
///
/// * Close an independent leaf page
/// * Abort a partially completed, multi-step form
/// * Exit a modal popup
/// * Close an app drawer
class LxCloseButton extends StatelessWidget {
  const LxCloseButton({
    super.key,
    this.kind = LxCloseButtonKind.closeFromTop,
  });

  final LxCloseButtonKind kind;

  void onTap(BuildContext context) {
    switch (this.kind) {
      case LxCloseButtonKind.closeFromTop:
        Navigator.of(context, rootNavigator: false).pop();
      case LxCloseButtonKind.closeFromRoot:
        Navigator.of(context, rootNavigator: true).pop();
      case LxCloseButtonKind.closeDrawer:
        Scaffold.of(context).closeDrawer();
    }
  }

  @override
  Widget build(BuildContext context) {
    return IconButton(
      icon: const Icon(Icons.close_rounded),
      onPressed: () => this.onTap(context),
    );
  }
}

/// ← - Back button, usually placed on the [AppBar] to go back a page in a
/// sub-flow.
///
/// Example usage:
///
/// * Go back to the previous page In a multi-step form.
class LxBackButton extends StatelessWidget {
  const LxBackButton({super.key});

  @override
  Widget build(BuildContext context) {
    return IconButton(
      icon: const Icon(Icons.arrow_back_rounded),
      onPressed: () => Navigator.of(context).pop(),
    );
  }
}

typedef StateStreamWidgetBuilder<T> = Widget Function(
  BuildContext context,
  T data,
);

/// A small helper `Widget` that builds a new widget everytime a `StateStream`
/// gets an update.
///
/// This is slightly nicer than the standard `StreamBuilder` because
/// `StateStream`s always have an initial value and never error.
class StateStreamBuilder<T> extends StreamBuilder<T> {
  StateStreamBuilder({
    super.key,
    required StateStream<T> stream,
    required StateStreamWidgetBuilder builder,
  }) : super(
          stream: stream,
          initialData: stream.value,
          builder: (BuildContext context, AsyncSnapshot<T> snapshot) =>
              builder(context, snapshot.data),
        );
}

extension StreamControllerExt<T> on StreamController<T> {
  /// Calls `add(event)` as long as the `StreamController` is not already
  /// closed.
  void addIfNotClosed(T event) {
    if (!this.isClosed) {
      this.add(event);
    }
  }
}
