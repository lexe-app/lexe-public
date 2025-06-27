import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/form.dart' as form;
import 'package:app_rs_dart/ffi/types.dart' show Config, DeployEnv;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;
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
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;

/// Require a signup code to complete signup.
const bool requireSignupCode = true;

/// A tiny interface so we can mock the [AppHandle.signup] call in design mode.
abstract interface class SignupApi {
  static const SignupApi prod = _ProdSignupApi._();

  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
    required String? signupCode,
    required String? partner,
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
    required String? signupCode,
    required String? partner,
  }) =>
      Result.tryFfiAsync(() => AppHandle.signup(
            config: config,
            googleAuthCode: googleAuthCode,
            password: password,
            signupCode: signupCode,
            partner: partner,
          ));
}

/// The entry point for the signup flow.
class SignupPage extends StatelessWidget {
  const SignupPage({super.key, required this.ctx});

  final SignupCtx ctx;

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
        builder: (_) => (requireSignupCode)
            ? SignupCodePage(ctx: this.ctx)
            : SignupGDriveAuthPage(ctx: this.ctx, signupCode: null),
      );
}

/// Ask the user for a signup code. While we're in closed beta, we'll require a
/// signup code to limit testers.
class SignupCodePage extends StatefulWidget {
  const SignupCodePage({super.key, required this.ctx});

  final SignupCtx ctx;

  @override
  State<SignupCodePage> createState() => _SignupCodePageState();
}

class _SignupCodePageState extends State<SignupCodePage> {
  final GlobalKey<FormFieldState<String>> signupCodeKey = GlobalKey();

  Result<String?, String?> validateSignupCode(final String? signupCode) {
    final ctx = this.widget.ctx;
    if (signupCode == null || signupCode.isEmpty) {
      // Signup code is only required in prod.
      if (ctx.config.deployEnv == DeployEnv.prod) {
        return const Err("");
      } else {
        return const Ok(null);
      }
    }

    // Remove whitespace and ensure all alphanumeric or dash.
    final trimmed = signupCode.trim();
    final nonAlphanumDash = RegExp(r'[^a-zA-Z0-9-]');
    if (!trimmed.contains(nonAlphanumDash)) {
      return Ok(trimmed);
    } else {
      return const Err("");
    }
  }

  Future<void> onSubmit() async {
    final codeField = this.signupCodeKey.currentState!;
    if (!codeField.validate()) {
      return;
    }
    final String? signupCode;
    switch (this.validateSignupCode(codeField.value)) {
      case Ok(:final ok):
        signupCode = ok;
      case Err():
        return;
    }

    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) =>
            SignupGDriveAuthPage(ctx: this.widget.ctx, signupCode: signupCode),
      ),
    );
    if (flowResult == null) return;
    if (!this.mounted) return;

    unawaited(Navigator.of(this.context).maybePop(flowResult));
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
          const HeadingText(text: "Enter signup code"),
          const SizedBox(height: Space.s600),

          // Signup code field
          TextFormField(
            key: this.signupCodeKey,
            autofocus: false,
            textInputAction: TextInputAction.done,
            validator: (str) => this.validateSignupCode(str).err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(hintText: "Signup code"),
            obscureText: false,
            enableSuggestions: false,
            autocorrect: false,
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size700,
              fontVariations: [Fonts.weightMedium],
              fontFeatures: [Fonts.featDisambugation],
              letterSpacing: -0.5,
            ),
          ),
        ],
        bottom: LxFilledButton.strong(
          label: const Text("Continue"),
          icon: const Icon(LxIcons.next),
          onTap: this.onSubmit,
        ),
      ),
    );
  }
}

/// This page has a button to ask for the user's consent for GDrive permissions.
class SignupGDriveAuthPage extends StatefulWidget {
  const SignupGDriveAuthPage(
      {super.key, required this.ctx, required this.signupCode});

  final SignupCtx ctx;
  final String? signupCode;

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

    // ignore: use_build_context_synchronously
    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => SignupBackupPasswordPage(
          ctx: ctx,
          authInfo: authInfo,
          signupCode: this.widget.signupCode,
        ),
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
            // styleSheet: LxTheme.buildMarkdownStyle(),
            styleSheet: LxTheme.markdownStyle,
          ),

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
          child: LxFilledButton.strong(
            label: const Text("Connect Google Drive"),
            icon: const Icon(LxIcons.next),
            onTap: this.onAuthPressed,
          ),
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
    required this.signupCode,
  });

  final SignupCtx ctx;
  final GDriveServerAuthCode authInfo;
  final String? signupCode;

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
      signupCode: this.widget.signupCode,
      partner: null,
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
          const SizedBox(height: Space.s100),
          MarkdownBody(
            data: '''
## Enter a backup password

Enter at least 12 characters.

You **need this to recover your funds**. Store it in a safe place, like a
password manager!
''',
            // styleSheet: LxTheme.buildMarkdownStyle().copyWith(
            styleSheet: LxTheme.markdownStyle.copyWith(
              blockSpacing: Space.s0,
              pPadding: const EdgeInsets.symmetric(vertical: Space.s100),
              h2Padding: const EdgeInsets.only(bottom: Space.s300),
            ),
            // styleSheet: LxTheme.markdownStyle,
          ),
          const SizedBox(height: Space.s500),

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
          Padding(
            padding: const EdgeInsets.only(top: Space.s400),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (_context, errorMessage, _widget) =>
                  ErrorMessageSection(errorMessage),
            ),
          ),
        ],
        bottomPadding: EdgeInsets.zero,
        bottom: Padding(
          padding: const EdgeInsets.symmetric(vertical: Space.s400),
          child: ValueListenableBuilder(
            valueListenable: this.isSigningUp,
            builder: (context, isSending, widget) => AnimatedFillButton(
              label: const Text("Sign up"),
              icon: const Icon(LxIcons.next),
              onTap: this.onSubmit,
              loading: isSending,
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
