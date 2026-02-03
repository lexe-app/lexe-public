/// UI flow for changing the Google Drive backup password.
library;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/gdrive.dart' show GDriveClient;
import 'package:app_rs_dart/ffi/types.dart' show Config;
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
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;
import 'package:lexeapp/validators.dart' as validators;

/// Entry point for the change backup password flow.
class ChangeBackupPasswordPage extends StatelessWidget {
  const ChangeBackupPasswordPage({
    super.key,
    required this.config,
    required this.app,
    required this.gdriveAuth,
  });

  final Config config;
  final AppHandle app;
  final GDriveAuth gdriveAuth;

  @override
  Widget build(BuildContext context) => MultistepFlow<void>(
    builder: (_) => ChangeBackupPasswordIntroPage(
      config: this.config,
      app: this.app,
      gdriveAuth: this.gdriveAuth,
    ),
  );
}

class ChangeBackupPasswordIntroPage extends StatefulWidget {
  const ChangeBackupPasswordIntroPage({
    super.key,
    required this.config,
    required this.app,
    required this.gdriveAuth,
  });

  final Config config;
  final AppHandle app;
  final GDriveAuth gdriveAuth;

  @override
  State<ChangeBackupPasswordIntroPage> createState() =>
      _ChangeBackupPasswordIntroPageState();
}

class _ChangeBackupPasswordIntroPageState
    extends State<ChangeBackupPasswordIntroPage> {
  final ValueNotifier<bool> isAuthorizing = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isAuthorizing.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onContinue() async {
    if (this.isAuthorizing.value) return;

    this.isAuthorizing.value = true;
    try {
      await this.onContinueInner();
    } finally {
      if (this.mounted) this.isAuthorizing.value = false;
    }
  }

  Future<void> onContinueInner() async {
    // Hide error message
    this.errorMessage.value = null;

    final authResult = await this.widget.gdriveAuth.tryAuth();
    if (!this.mounted) return;

    final GDriveClient gdriveClient;
    switch (authResult) {
      case Ok(:final ok):
        // User canceled.
        if (ok == null) return;
        gdriveClient = ok;
      case Err(:final err):
        final errStr = err.toString();
        error("change-backup-password: Failed to auth gdrive: $errStr");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error connecting your Google Drive",
          message: errStr,
        );
        return;
    }

    await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => ChangeBackupPasswordFormPage(
          config: this.widget.config,
          app: this.widget.app,
          gdriveClient: gdriveClient,
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          // Big Google Drive icon
          const Icon(
            LxIcons.gdrive,
            size: Space.s900,
            weight: 300,
            opticalSize: 48,
            grade: -50,
          ),
          MarkdownBody(
            data: '''
# Change your backup password

We'll re-encrypt your Google Drive backup with a new password.

- We'll ask Google Drive for access.
- Then you'll choose a new password.
''',
            styleSheet: LxTheme.markdownStyle,
          ),

          // Error message
          Padding(
            padding: const EdgeInsets.only(top: Space.s500),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isAuthorizing,
            builder: (context, isAuthorizing, widget) => AnimatedFillButton(
              onTap: this.onContinue,
              loading: isAuthorizing,
              label: const Text("Continue"),
              icon: const Icon(LxIcons.next),
              style: FilledButton.styleFrom(
                backgroundColor: LxColors.foreground,
                foregroundColor: LxColors.background,
                iconColor: LxColors.background,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class ChangeBackupPasswordFormPage extends StatefulWidget {
  const ChangeBackupPasswordFormPage({
    super.key,
    required this.config,
    required this.app,
    required this.gdriveClient,
  });

  final Config config;
  final AppHandle app;
  final GDriveClient gdriveClient;

  @override
  State<ChangeBackupPasswordFormPage> createState() =>
      _ChangeBackupPasswordFormPageState();
}

class _ChangeBackupPasswordFormPageState
    extends State<ChangeBackupPasswordFormPage> {
  final GlobalKey<FormFieldState<String>> currentPasswordFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> newPasswordFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> confirmPasswordFieldKey = GlobalKey();

  final ValueNotifier<bool> isSaving = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isSaving.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    if (this.isSaving.value) return;

    this.errorMessage.value = null;

    final currentPasswordState = this.currentPasswordFieldKey.currentState!;
    final newPasswordState = this.newPasswordFieldKey.currentState!;
    final confirmPasswordState = this.confirmPasswordFieldKey.currentState!;

    final currentPasswordIsValid = currentPasswordState.validate();
    final newPasswordIsValid = newPasswordState.validate();
    final confirmPasswordIsValid = confirmPasswordState.validate();
    if (!currentPasswordIsValid ||
        !newPasswordIsValid ||
        !confirmPasswordIsValid) {
      return;
    }

    final String currentPassword;
    switch (validators.validatePassword(currentPasswordState.value)) {
      case Ok(:final ok):
        currentPassword = ok;
      case Err():
        return;
    }

    final String newPassword;
    switch (validators.validatePassword(newPasswordState.value)) {
      case Ok(:final ok):
        newPassword = ok;
      case Err():
        return;
    }

    this.isSaving.value = true;
    try {
      await this.onSubmitInner(
        currentPassword: currentPassword,
        newPassword: newPassword,
      );
    } finally {
      if (this.mounted) this.isSaving.value = false;
    }
  }

  Future<void> onSubmitInner({
    required String currentPassword,
    required String newPassword,
  }) async {
    final config = this.widget.config;
    final userPk = this.widget.app.walletUser().userPk;

    final result = await Result.tryFfiAsync(
      () => this.widget.gdriveClient.intoRestoreClient().rotateBackupPassword(
        deployEnv: config.deployEnv,
        network: config.network,
        useSgx: config.useSgx,
        userPk: userPk,
        currentPassword: currentPassword,
        newPassword: newPassword,
      ),
    );
    if (!this.mounted) return;

    switch (result) {
      case Ok():
        await Navigator.of(this.context).pushReplacement(
          MaterialPageRoute(
            builder: (_) => const ChangeBackupPasswordSuccessPage(),
          ),
        );
      case Err(:final err):
        error("change-backup-password: rotate failed: ${err.message}");
        this.errorMessage.value = ErrorMessage(
          title: "Failed to change backup password",
          message: err.message,
        );
        return;
    }
  }

  @override
  Widget build(BuildContext context) {
    final textFieldStyle = Fonts.fontUI.copyWith(
      fontSize: Fonts.size700,
      fontVariations: [Fonts.weightMedium],
      fontFeatures: [Fonts.featDisambugation],
      letterSpacing: -0.5,
    );

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const HeadingText(text: "Change backup password"),
          const SizedBox(height: Space.s200),
          const SubheadingText(
            text: "Enter your current backup password, then choose a new one.",
          ),
          const SizedBox(height: Space.s400),
          const Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: "WARNING: ",
                  style: TextStyle(color: LxColors.warningText),
                ),
                TextSpan(
                  text:
                      "Store this password in a safe place, like a password manager. You need it to recover your funds.",
                  style: TextStyle(color: LxColors.fgTertiary),
                ),
              ],
            ),
          ),
          const SizedBox(height: Space.s600),

          // Current password
          TextFormField(
            key: this.currentPasswordFieldKey,
            autofocus: true,
            textInputAction: TextInputAction.next,
            validator: (str) => validators.validatePassword(str).err,
            onEditingComplete: () {
              final state = this.currentPasswordFieldKey.currentState!;
              if (state.validate()) {
                FocusScope.of(this.context).nextFocus();
              }
            },
            decoration: baseInputDecoration.copyWith(
              hintText: "Current password",
            ),
            obscureText: true,
            style: textFieldStyle,
          ),
          const SizedBox(height: Space.s200),

          // New password
          TextFormField(
            key: this.newPasswordFieldKey,
            autofocus: false,
            textInputAction: TextInputAction.next,
            validator: (str) => validators.validatePassword(str).err,
            onEditingComplete: () {
              final state = this.newPasswordFieldKey.currentState!;
              if (state.validate()) {
                FocusScope.of(this.context).nextFocus();
              }
            },
            decoration: baseInputDecoration.copyWith(hintText: "New password"),
            obscureText: true,
            style: textFieldStyle,
          ),
          const SizedBox(height: Space.s200),

          // Confirm new password
          TextFormField(
            key: this.confirmPasswordFieldKey,
            autofocus: false,
            textInputAction: TextInputAction.done,
            validator: (str) => validators
                .validateConfirmPassword(
                  password: this.newPasswordFieldKey.currentState!.value,
                  confirmPassword: str,
                )
                .err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(
              hintText: "Confirm new password",
            ),
            obscureText: true,
            style: textFieldStyle,
          ),

          // Error message
          Padding(
            padding: const EdgeInsets.only(top: Space.s500),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isSaving,
            builder: (context, isSaving, widget) => AnimatedFillButton(
              onTap: this.onSubmit,
              loading: isSaving,
              label: const Text("Change password"),
              icon: const Icon(LxIcons.next),
              style: FilledButton.styleFrom(
                backgroundColor: LxColors.moneyGoUp,
                foregroundColor: LxColors.grey1000,
                iconColor: LxColors.grey1000,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class ChangeBackupPasswordSuccessPage extends StatelessWidget {
  const ChangeBackupPasswordSuccessPage({super.key});

  void onDone(BuildContext context) {
    Navigator.of(context, rootNavigator: true).pop();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        automaticallyImplyLeading: false,
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.s400),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          const SizedBox(height: Space.s500),

          // Google Drive icon with success badge
          Align(
            alignment: Alignment.topCenter,
            child: Badge(
              label: const Icon(
                LxIcons.completedBadge,
                size: Fonts.size400,
                color: LxColors.background,
              ),
              backgroundColor: LxColors.moneyGoUp,
              largeSize: Space.s500,
              child: const DecoratedBox(
                decoration: BoxDecoration(
                  color: LxColors.grey825,
                  borderRadius: BorderRadius.all(
                    Radius.circular(Space.s800 / 2),
                  ),
                ),
                child: SizedBox.square(
                  dimension: Space.s800,
                  child: Icon(
                    LxIcons.gdrive,
                    size: Space.s650,
                    color: LxColors.fgSecondary,
                    fill: 1.0,
                    weight: LxIcons.weightExtraLight,
                  ),
                ),
              ),
            ),
          ),

          const SizedBox(height: Space.s500),

          // "Updated" label
          Text(
            "Backup password updated",
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size300,
              color: LxColors.fgTertiary,
              fontVariations: [Fonts.weightNormal],
            ),
            textAlign: TextAlign.center,
          ),

          const SizedBox(height: Space.s300),

          // Success title
          Text(
            "You're all set!",
            style: Fonts.fontUI.copyWith(
              letterSpacing: -0.5,
              fontSize: Fonts.size600,
              fontVariations: [Fonts.weightNormal],
              fontFeatures: [Fonts.featSlashedZero],
              color: LxColors.moneyGoUp,
            ),
            textAlign: TextAlign.center,
          ),

          const SizedBox(height: Space.s600),

          // Subtext
          Text(
            "Your Google Drive backup is now protected with your new password.",
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size200,
              color: LxColors.fgSecondary,
            ),
          ),
        ],
        bottom: LxFilledButton.strong(
          label: const Text("Done"),
          icon: const Icon(LxIcons.next),
          onTap: () => this.onDone(context),
        ),
      ),
    );
  }
}
