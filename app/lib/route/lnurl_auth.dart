/// LNURL-auth (LUD-04): approve signing in to a service with a private login
/// linked to the user's Lexe wallet.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show LnurlAuthAction, LnurlAuthRequest;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        AccessItem,
        AccessKind,
        AccessRow,
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        InfoCard,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxOutlinedButton,
        ScrollableSinglePageBody,
        SuccessCheckmarkModal,
        WarningCard;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, LxIcons, Space;

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

  /// The heading, button label, and error title for the request's LUD-04
  /// `action`.
  ({
    String heading,
    String button,
    String warning,
    String approval,
    String errorTitle,
  })
  actionText() => switch (this.widget.request.action) {
    LnurlAuthAction.register => (
      heading: "Sign up for",
      button: "Sign up",
      warning: "a sign-up",
      approval: "Signed up",
      errorTitle: "Couldn't sign up",
    ),
    LnurlAuthAction.login ||
    LnurlAuthAction.link ||
    LnurlAuthAction.auth ||
    null => (
      heading: "Sign in to",
      button: "Sign in",
      warning: "sign-ins",
      approval: "Signed in",
      errorTitle: "Couldn't sign in",
    ),
  };

  static const List<AccessItem> accessItems = [
    (
      icon: LxIcons.security,
      title: Text("No wallet access"),
      description: Text(
        "This site can't see your balance or payments, or spend your money.",
      ),
      kind: AccessKind.scope,
    ),
    (
      icon: LxIcons.private,
      title: Text("Private login"),
      description: Text(
        "You get a separate login for every site, so they can't recognize you "
        "elsewhere.",
      ),
      kind: AccessKind.scope,
    ),
  ];

  Future<void> onApprove() async {
    if (this.isPending.value) return;

    this.isPending.value = true;
    this.errorMessage.value = null;

    final result = await Result.tryFfiAsync(
      () => this.widget.app.approveLnurlAuth(request: this.widget.request),
    );
    if (!this.mounted) return;

    final actionText = this.actionText();

    switch (result) {
      case Ok():
        info("LnurlAuthPage: approved");
        await showDialog(
          context: this.context,
          builder: (_) => SuccessCheckmarkModal(label: actionText.approval),
        );
        if (!this.mounted) return;
        unawaited(
          Navigator.of(this.context).maybePop(LnurlAuthFlowResult.approved),
        );
      case Err(:final err):
        error("LnurlAuthPage: error approving: ${err.message}");
        this.isPending.value = false;
        this.errorMessage.value = ErrorMessage(
          title: actionText.errorTitle,
          message: err.message,
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final domain = this.widget.request.domain;
    final actionText = this.actionText();

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
          // Heading
          HeadingText(text: actionText.heading),
          Text(
            domain,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size400,
              fontVariations: const [Fonts.weightMedium],
              // Helps the user spot lookalike domains, e.g. `examp1e.com`.
              fontFeatures: const [
                Fonts.featDisambugation,
                Fonts.featNoContextualAlts,
              ],
              height: 1.2,
            ),
          ),
          const SizedBox(height: Space.s400),

          // Scam warning
          WarningCard(
            emphasis: "Only approve ${actionText.warning} you initiated.",
            detail: "Anyone who sent you this link can login to your account.",
          ),
          const SizedBox(height: Space.s200),

          // Access
          InfoCard(
            header: const Text("Grants"),
            bodyPadding: Space.s400,
            children: [
              for (final item in accessItems)
                AccessRow(item: item, expanded: true, onTap: null),
            ],
          ),

          // Error
          Padding(
            padding: const EdgeInsets.only(top: Space.s400),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (context, errorMessage, _) =>
                  ErrorMessageSection(errorMessage, bodyPadding: Space.s400),
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
                // Approve
                AnimatedFillButton(
                  label: Text(actionText.button),
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

                // Cancel
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
