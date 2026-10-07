/// LNURL-auth (LUD-04): approve authenticating to a service with a key
/// unique to its domain.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show LnurlAuthAction, LnurlAuthRequest;
import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart'
    show MarkdownBody;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxOutlinedButton,
        ScrollableSinglePageBody,
        SuccessCheckmarkModal,
        WarningCard;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;

enum LnurlAuthFlowResult { approved }

/// The LNURL-auth approval screen. Pops with [LnurlAuthFlowResult.approved]
/// once the service accepts the signature, or with null if the user backs out.
class LnurlAuthPage extends StatefulWidget {
  const LnurlAuthPage({super.key, required this.app, required this.request});

  final AppHandle app;
  final LnurlAuthRequest request;

  @override
  State<LnurlAuthPage> createState() => _LnurlAuthPageState();
}

class _LnurlAuthPageState extends State<LnurlAuthPage> {
  final ValueNotifier<bool> isPending = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isPending.dispose();
    super.dispose();
  }

  /// The heading for the request's LUD-04 `action`.
  String heading() => switch (this.widget.request.action) {
    LnurlAuthAction.register => "Sign up with Lexe",
    LnurlAuthAction.login => "Sign in with Lexe",
    LnurlAuthAction.link => "Link with Lexe",
    LnurlAuthAction.auth => "Authorize with Lexe",
    null => "Continue with Lexe",
  };

  /// The error title for the request's LUD-04 `action`.
  String errorTitle() => switch (this.widget.request.action) {
    LnurlAuthAction.register => "Failed to sign up",
    LnurlAuthAction.login => "Failed to sign in",
    LnurlAuthAction.link => "Failed to link account",
    LnurlAuthAction.auth => "Failed to approve action",
    null => "Failed to authenticate",
  };

  Future<void> onApprove() async {
    if (this.isPending.value) return;

    this.isPending.value = true;
    this.errorMessage.value = null;

    final result = await Result.tryFfiAsync(
      () => this.widget.app.approveLnurlAuth(request: this.widget.request),
    );
    if (!this.mounted) return;

    switch (result) {
      case Ok():
        info("LnurlAuthPage: approved");
        await showDialog(
          context: this.context,
          builder: (_) => const SuccessCheckmarkModal(label: "Approved"),
        );
        if (!this.mounted) return;
        unawaited(
          Navigator.of(this.context).maybePop(LnurlAuthFlowResult.approved),
        );
      case Err(:final err):
        error("LnurlAuthPage: error approving: ${err.message}");
        this.isPending.value = false;
        this.errorMessage.value = ErrorMessage(
          title: this.errorTitle(),
          message: err.message,
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final domain = this.widget.request.domain;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          HeadingText(text: this.heading()),
          Text(
            domain,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size400,
              fontVariations: const [Fonts.weightMedium],
              // Helps the user spot lookalike domains, e.g. `examp1e.com`.
              fontFeatures: const [Fonts.featDisambugation],
              height: 1.2,
            ),
          ),
          const SizedBox(height: Space.s400),
          WarningCard(
            emphasis: "Scammers can use this to break into your account.",
            detail:
                "Only approve if the site above gave you this code or link "
                "directly.",
          ),
          const SizedBox(height: Space.s400),

          // Explanation
          MarkdownBody(
            data: '''
Your Lexe wallet is your login, no password needed.

- This site learns nothing about your wallet.
- Each site sees you as a different person.
''',
            styleSheet: LxTheme.markdownStyle,
          ),

          // Error
          Padding(
            padding: const EdgeInsets.only(top: Space.s500),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (context, errorMessage, _) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isPending,
            builder: (context, isPending, _) => Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                AnimatedFillButton(
                  label: const Text("Approve"),
                  icon: const Icon(LxIcons.next),
                  onTap: isPending ? null : this.onApprove,
                  loading: isPending,
                  style: FilledButton.styleFrom(
                    backgroundColor: LxColors.moneyGoUp,
                    foregroundColor: LxColors.grey1000,
                    iconColor: LxColors.grey1000,
                  ),
                ),
                const SizedBox(height: Space.s300),
                SizedBox(
                  width: double.infinity,
                  // LUD-04 has no rejection callback, so just back out.
                  child: LxOutlinedButton(
                    label: const Text("Cancel"),
                    onTap: isPending
                        ? null
                        : () => Navigator.of(context).maybePop(),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
