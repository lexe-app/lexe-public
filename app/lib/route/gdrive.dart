import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart' show GDriveSignupCredentials;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart'
    show MarkdownBody;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        ErrorMessage,
        ErrorMessageSection,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;
import 'package:lexeapp/validators.dart' as validators;

final class GDriveAuthCtx {
  const GDriveAuthCtx(this.app, this.gdriveAuth);

  final AppHandle app;
  final GDriveAuth gdriveAuth;
}

/// Entry point for setting up Google Drive backup after signup.
///
/// Pops `true` once the user's backup is set up, and `null` if they back out.
class GDrivePage extends StatelessWidget {
  const GDrivePage({super.key, required this.ctx});

  final GDriveAuthCtx ctx;

  @override
  Widget build(BuildContext context) =>
      MultistepFlow<bool>(builder: (_) => GDriveAuthPage(ctx: this.ctx));
}

/// This page has a button to ask for the user's consent for GDrive permissions.
class GDriveAuthPage extends StatefulWidget {
  const GDriveAuthPage({super.key, required this.ctx});

  final GDriveAuthCtx ctx;

  @override
  State<StatefulWidget> createState() => _GDriveAuthPageState();
}

class _GDriveAuthPageState extends State<GDriveAuthPage> {
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onAuthPressed() async {
    final ctx = this.widget.ctx;

    // Hide error message
    this.errorMessage.value = null;

    final result = await ctx.gdriveAuth.tryAuthCodeOnly();
    if (!this.mounted) return;

    final GDriveServerAuthCode authInfo;
    switch (result) {
      case Ok(:final ok):
        // user canceled. they might want to try again, so don't pop yet.
        if (ok == null) return;
        authInfo = ok;
      case Err(:final err):
        // Pull out the error message, without too much extra formatting.
        final String errStr;
        switch (err) {
          case PlatformException(:final code, :final message):
            errStr = "$message (code=$code)";
          case FfiError(:final message):
            errStr = message;
          default:
            errStr = err.toString();
        }

        error("Failed to auth user with GDrive: $errStr");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error connecting your Google Drive",
          message: errStr,
        );
        return;
    }

    final bool? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => GDriveBackupPasswordPage(ctx: ctx, authInfo: authInfo),
      ),
    );
    if (flowResult == null || !this.mounted) return;

    info("GDriveAuthPage: successfully set up gdrive backup");

    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        body: [
          GDrivePreamble(),
          // Error message box
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
          child: Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              LxFilledButton.strong(
                label: const Text("Connect Google Drive"),
                icon: const Icon(LxIcons.next),
                onTap: this.onAuthPressed,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// General Google Drive backup information, shown before asking for consent.
class GDrivePreamble extends StatelessWidget {
  const GDrivePreamble({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
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
# Connect your Google Drive

Lexe will create a **LexeData** folder in your Google Drive to store
encrypted recovery data and keep it up-to-date.

- Your node can only access the files it creates, and **nothing else**.
- Neither Google nor Lexe can decrypt your recovery data, but you can, using
your **backup password**.
- With your recovery data, **you can always recover your funds**—even if Lexe goes away.
''',
          // styleSheet: LxTheme.buildMarkdownStyle(),
          styleSheet: LxTheme.markdownStyle,
        ),
      ],
    );
  }
}

class GDriveBackupPasswordPage extends StatefulWidget {
  const GDriveBackupPasswordPage({
    super.key,
    required this.ctx,
    required this.authInfo,
  });

  final GDriveAuthCtx ctx;
  final GDriveServerAuthCode authInfo;

  @override
  State<GDriveBackupPasswordPage> createState() =>
      _GDriveBackupPasswordPageState();
}

class _GDriveBackupPasswordPageState extends State<GDriveBackupPasswordPage> {
  final GlobalKey<GDriveBackupPasswordFieldsState> passwordFieldsKey =
      GlobalKey();

  final ValueNotifier<bool> isSettingUp = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isSettingUp.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    // Ignore press while signing up
    if (this.isSettingUp.value) return;

    // Hide error message
    this.errorMessage.value = null;

    // Get the validated password
    final fieldState = this.passwordFieldsKey.currentState!;
    final password = fieldState.validateAndGetPassword();
    // Do nothing if the password is invalid
    if (password == null) {
      return;
    }

    info("GDriveBackupPasswordPage: ready to set up gdrive backup");

    this.isSettingUp.value = true;
    try {
      await this.onSubmitInner(password);
    } finally {
      if (this.mounted) this.isSettingUp.value = false;
    }
  }

  Future<void> onSubmitInner(String password) async {
    final ctx = this.widget.ctx;
    final gdriveSignupCreds = GDriveSignupCredentials(
      backupPassword: password,
      googleAuthCode: this.widget.authInfo.serverAuthCode,
    );
    final result = await Result.tryFfiAsync(
      () => ctx.app.setupGdrive(gdriveSignupCredentials: gdriveSignupCreds),
    );
    if (!this.mounted) return;

    if (result case Err(:final err)) {
      error(
        "GDriveBackupPasswordPage: failed to set up gdrive: ${err.message}",
      );
      this.errorMessage.value = ErrorMessage(
        title: "Failed to connect Google Drive",
        message: err.message,
      );
      return;
    }

    unawaited(Navigator.of(this.context).maybePop(true));
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
          GDriveBackupPasswordPreamble(),
          const SizedBox(height: Space.s600),

          // Password fields
          GDriveBackupPasswordFields(
            key: this.passwordFieldsKey,
            onSubmit: this.onSubmit,
          ),

          // Error message
          Padding(
            padding: const EdgeInsets.only(top: Space.s300),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) => Padding(
                padding: EdgeInsets.only(
                  bottom: errorMessage != null ? Space.s300 : 0,
                ),
                child: ErrorMessageSection(errorMessage),
              ),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isSettingUp,
            builder: (context, isSettingUp, widget) => AnimatedFillButton(
              label: const Text("Backup to GDrive"),
              icon: const Icon(LxIcons.next),
              onTap: this.onSubmit,
              loading: isSettingUp,
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

class GDriveBackupPasswordPreamble extends StatelessWidget {
  const GDriveBackupPasswordPreamble({super.key});

  @override
  Widget build(BuildContext context) {
    return MarkdownBody(
      data: '''
# Enter your backup password

Enter at least 12 characters.

This password encrypts your recovery data so Google can't read it.
Store it in a safe place, like a password manager—you **need this to
recover your funds**.
''',
      styleSheet: LxTheme.markdownStyle.copyWith(
        pPadding: const EdgeInsets.symmetric(vertical: Space.s100),
      ),
    );
  }
}

class GDriveBackupPasswordFields extends StatefulWidget {
  const GDriveBackupPasswordFields({super.key, required this.onSubmit});

  /// Called when the user finishes the last field.
  final VoidCallback onSubmit;

  @override
  State<GDriveBackupPasswordFields> createState() =>
      GDriveBackupPasswordFieldsState();
}

class GDriveBackupPasswordFieldsState
    extends State<GDriveBackupPasswordFields> {
  final GlobalKey<FormFieldState<String>> _passwordFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> _confirmPasswordFieldKey =
      GlobalKey();

  /// Ensure the password fields match and that the password is valid, then
  /// return the password if it is, and `null` if the password is invalid.
  /// Triggers form field errors if validation fails.
  String? validateAndGetPassword() {
    final passwordIsValid = this._passwordFieldKey.currentState!.validate();
    final fieldState = this._confirmPasswordFieldKey.currentState!;
    if (!passwordIsValid || !fieldState.validate()) {
      return null;
    }

    final String password;
    switch (validators.validatePassword(
      this._passwordFieldKey.currentState!.value,
    )) {
      case Ok(:final ok):
        password = ok;
      case Err():
        // Should be unreachable, but just to be safe.
        return null;
    }
    return password;
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Password field
        TextFormField(
          key: this._passwordFieldKey,
          autofocus: true,
          textInputAction: TextInputAction.next,
          validator: (str) => validators.validatePassword(str).err,
          onEditingComplete: () {
            // Only show the input error on field completion (good UX).
            // Only move to the next field if the input is valid.
            final state = this._passwordFieldKey.currentState!;
            if (state.validate()) {
              FocusScope.of(this.context).nextFocus();
            }
          },
          decoration: baseInputDecoration.copyWith(hintText: "Password"),
          obscureText: true,
          style: Fonts.fontPassword,
        ),
        const SizedBox(height: Space.s100),

        // Confirm password field
        TextFormField(
          key: this._confirmPasswordFieldKey,
          autofocus: false,
          textInputAction: TextInputAction.done,
          validator: (str) => validators
              .validateConfirmPassword(
                password: this._passwordFieldKey.currentState!.value,
                confirmPassword: str,
              )
              .err,
          onEditingComplete: () {
            FocusScope.of(this.context).unfocus();
            this.widget.onSubmit();
          },
          decoration: baseInputDecoration.copyWith(
            hintText: "Confirm password",
          ),
          obscureText: true,
          style: Fonts.fontPassword,
        ),
      ],
    );
  }
}
