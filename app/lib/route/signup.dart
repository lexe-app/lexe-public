import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/form.dart' as form;
import 'package:app_rs_dart/ffi/types.dart' show Config;
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart' show MarkdownBody;
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
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;

/// A tiny interface so we can mock the [AppHandle.signup] call in design mode.
abstract interface class SignupApi {
  static const SignupApi prod = _ProdSignupApi._();

  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
  });
}

/// Collect all the context required for the Signup flow.
final class SignupCtx {
  const SignupCtx(this.config, this.gdriveAuth, this.signupApi);

  final Config config;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;
}

class _ProdSignupApi implements SignupApi {
  const _ProdSignupApi._();

  @override
  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
  }) =>
      Result.tryFfiAsync(() => AppHandle.signup(
            config: config,
            googleAuthCode: googleAuthCode,
            password: password,
          ));
}

/// The entry point for the signup flow.
class SignupPage extends StatelessWidget {
  const SignupPage({super.key, required this.ctx});

  final SignupCtx ctx;

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
        builder: (_) => SignupGDriveAuthPage(ctx: this.ctx),
      );
}

/// This page has a button to ask for the user's consent for GDrive permissions.
class SignupGDriveAuthPage extends StatefulWidget {
  const SignupGDriveAuthPage({super.key, required this.ctx});

  final SignupCtx ctx;

  @override
  State<StatefulWidget> createState() => _SignupGDriveAuthPageState();
}

class _SignupGDriveAuthPageState extends State<SignupGDriveAuthPage> {
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
        final errStr = err.toString();
        error("Failed to auth user with GDrive: $errStr");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error connecting your Google Drive",
          message: errStr,
        );
        return;
    }

    // ignore: use_build_context_synchronously
    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => SignupBackupPasswordPage(ctx: ctx, authInfo: authInfo),
      ),
    );
    if (flowResult == null) return;
    if (!this.mounted) return;

    info("SignupGDriveAuthPage: successful signup");

    // ignore: use_build_context_synchronously
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

Your **node needs access to Drive** to persist small amounts
of critical data on a regular basis.

- Your node can only access the files it creates in the **LexeData** folder.
- Lexe cannot access any files in your Drive.
- All data in Drive is stored end-to-end encrypted and is only readable by
  you and your node.
''',
            styleSheet: LxTheme.markdownStyle,
          ),
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (_context, errorMessage, _widget) => Padding(
              padding: const EdgeInsets.symmetric(vertical: Space.s500),
              child: ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottom: LxFilledButton.strong(
          label: const Text("Connect Google Drive"),
          icon: const Icon(LxIcons.next),
          onTap: this.onAuthPressed,
        ),
      ),
    );
  }
}

class SignupBackupPasswordPage extends StatefulWidget {
  const SignupBackupPasswordPage({
    super.key,
    required this.ctx,
    required this.authInfo,
  });

  final SignupCtx ctx;
  final GDriveServerAuthCode authInfo;

  @override
  State<SignupBackupPasswordPage> createState() =>
      _SignupBackupPasswordPageState();
}

class _SignupBackupPasswordPageState extends State<SignupBackupPasswordPage> {
  final GlobalKey<FormFieldState<String>> passwordFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> confirmPasswordFieldKey = GlobalKey();

  final ValueNotifier<bool> isSigningUp = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isSigningUp.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Result<String, String?> validatePassword(String? password) {
    if (password == null || password.isEmpty) {
      return const Err("");
    }

    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
    final maybeErrMsg = form.validatePassword(password: password);
    if (maybeErrMsg == null) {
      return Ok(password);
    } else {
      return Err(maybeErrMsg);
    }
  }

  Result<String, String?> validateConfirmPassword(String? confirmPassword) {
    if (confirmPassword == null || confirmPassword.isEmpty) {
      return const Err("");
    }

    final password = this.passwordFieldKey.currentState!.value;
    if (password == confirmPassword) {
      return Ok(confirmPassword);
    } else if (password == null) {
      return const Err("");
    } else {
      return const Err("Passwords don't match");
    }
  }

  Future<void> onSubmit() async {
    // Ignore press while signing up
    if (this.isSigningUp.value) return;

    // Hide error message
    this.errorMessage.value = null;

    final passwordIsValid = this.passwordFieldKey.currentState!.validate();
    final fieldState = this.confirmPasswordFieldKey.currentState!;
    if (!passwordIsValid || !fieldState.validate()) {
      return;
    }

    final String password;
    switch (this.validatePassword(fieldState.value!)) {
      case Ok(:final ok):
        password = ok;
      case Err():
        return;
    }

    info("SignupBackupPasswordPage: ready to sign up");

    this.isSigningUp.value = true;
    try {
      await this.onSubmitInner(password);
    } finally {
      if (this.mounted) this.isSigningUp.value = false;
    }
  }

  Future<void> onSubmitInner(String password) async {
    final ctx = this.widget.ctx;
    final result = await ctx.signupApi.signup(
      config: ctx.config,
      googleAuthCode: this.widget.authInfo.serverAuthCode,
      password: password,
    );
    if (!this.mounted) return;

    final AppHandle flowResult;
    switch (result) {
      case Ok(:final ok):
        flowResult = ok;
      case Err(:final err):
        error("Failed to signup: $err");
        this.errorMessage.value = ErrorMessage(
          title: "Failed to signup",
          message: err.message,
        );
        return;
    }

    unawaited(Navigator.of(this.context).maybePop(flowResult));
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
          const HeadingText(text: "Enter a backup password"),
          const SubheadingText(text: "with at least 12 characters"),
          const SizedBox(height: Space.s600),

          // Password field
          TextFormField(
            key: this.passwordFieldKey,
            autofocus: true,
            textInputAction: TextInputAction.next,
            validator: (str) => this.validatePassword(str).err,
            onEditingComplete: () {
              // Only show the input error on field completion (good UX).
              // Only move to the next field if the input is valid.
              final state = this.passwordFieldKey.currentState!;
              if (state.validate()) {
                FocusScope.of(this.context).nextFocus();
              }
            },
            decoration: baseInputDecoration.copyWith(hintText: "Password"),
            obscureText: true,
            style: textFieldStyle,
          ),
          const SizedBox(height: Space.s200),

          // Confirm password field
          TextFormField(
            key: this.confirmPasswordFieldKey,
            autofocus: false,
            textInputAction: TextInputAction.done,
            validator: (str) => this.validateConfirmPassword(str).err,
            onEditingComplete: this.onSubmit,
            decoration:
                baseInputDecoration.copyWith(hintText: "Confirm password"),
            obscureText: true,
            style: textFieldStyle,
          ),

          // Error message
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (_context, errorMessage, _widget) => Padding(
              padding: const EdgeInsets.symmetric(vertical: Space.s500),
              child: ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottom: Column(
          mainAxisSize: MainAxisSize.min,
          mainAxisAlignment: MainAxisAlignment.end,
          verticalDirection: VerticalDirection.down,
          children: [
            const Expanded(child: SizedBox(height: Space.s500)),

            // Disable the button and show a loading indicator while sending the
            // request.
            ValueListenableBuilder(
              valueListenable: this.isSigningUp,
              builder: (context, isSending, widget) => AnimatedFillButton(
                label: const Text("Sign up"),
                icon: const Icon(LxIcons.next),
                onTap: this.onSubmit,
                loading: isSending,
                style: FilledButton.styleFrom(
                  backgroundColor: LxColors.moneyGoUp,
                  foregroundColor: LxColors.grey1000,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
