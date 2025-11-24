import 'package:flutter/material.dart';
import 'package:lexeapp/clipboard.dart' show LxClipboard;
import 'package:lexeapp/components.dart'
    show
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxFilledButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/style.dart' show LxIcons, Space;

class AppLoadErrorPage extends StatelessWidget {
  const AppLoadErrorPage({super.key, required this.errorMessage});

  final String errorMessage;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: ScrollableSinglePageBody(
        padding: const EdgeInsets.symmetric(horizontal: Space.s600),
        body: [
          const SizedBox(height: Space.s800),
          const HeadingText(text: "Oops!"),
          const SubheadingText(
            text:
                "Something went wrong while loading your wallet. Contact support if error persist.",
          ),
          const SizedBox(height: Space.s600),
          ErrorMessageSection(
            ErrorMessage(title: "Error details", message: this.errorMessage),
          ),
        ],
        bottom: LxFilledButton.strong(
          label: const Text("Copy error"),
          icon: const Icon(LxIcons.copy),
          onTap: () =>
              LxClipboard.copyTextWithFeedback(context, this.errorMessage),
        ),
      ),
    );
  }
}
