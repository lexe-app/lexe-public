import 'dart:async';

import 'package:flutter/material.dart';

import 'package:lexeapp/bindings.dart' show api;
import 'package:lexeapp/bindings_generated_api.dart' show AppHandle, Config;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveAuthInfo;
import 'package:lexeapp/logger.dart' show error, info;
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart' show Fonts, LxColors, Space;

/// A tiny interface for mocking the `signup` call.
abstract interface class SignupApi {
  static const SignupApi prod = _ProdSignupApi();

  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
  });
}

class _ProdSignupApi implements SignupApi {
  const _ProdSignupApi();

  @override
  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required String googleAuthCode,
    required String password,
  }) =>
      Result.tryFfiAsync(() => AppHandle.signup(
            bridge: api,
            config: config,
            googleAuthCode: googleAuthCode,
            password: password,
          ));
}

/// The entry point for the signup flow.
class SignupPage extends StatelessWidget {
  const SignupPage({
    super.key,
    required this.config,
    required this.gdriveAuth,
    required this.signupApi,
  });

  final Config config;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
        builder: (_) => SignupGDriveAuthPage(
          config: config,
          gdriveAuth: gdriveAuth,
          signupApi: signupApi,
        ),
      );
}

/// This page has a button to ask for the user's consent for GDrive permissions.
class SignupGDriveAuthPage extends StatefulWidget {
  const SignupGDriveAuthPage({
    super.key,
    required this.config,
    required this.gdriveAuth,
    required this.signupApi,
  });

  final Config config;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;

  @override
  State<StatefulWidget> createState() => _SignupGDriveAuthPageState();
}

class _SignupGDriveAuthPageState extends State<SignupGDriveAuthPage> {
  Future<void> onAuthPressed() async {
    final result = await this.widget.gdriveAuth.tryAuth();
    if (!this.mounted) return;

    final GDriveAuthInfo authInfo;
    switch (result) {
      case Ok(:final ok):
        // user canceled. they might want to try again, so don't pop yet.
        if (ok == null) return;
        authInfo = ok;
      case Err(:final err):
        error("Failed to auth user with GDrive: $err");
        return;
    }

    final AppHandle? flowResult =
        // ignore: use_build_context_synchronously
        await Navigator.of(this.context).push(MaterialPageRoute(
            builder: (_) => SignupBackupPasswordPage(
                  config: this.widget.config,
                  signupApi: this.widget.signupApi,
                  authInfo: authInfo,
                )));
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
        leading: const LxBackButton(),
      ),
      body: ScrollableSinglePageBody(
        body: const [
          HeadingText(text: "Google Drive Auth"),
        ],
        bottom: LxFilledButton(
          label: const Text("Sign in with Google Drive"),
          icon: const Icon(Icons.arrow_forward_rounded),
          onTap: this.onAuthPressed,
        ),
      ),
    );
  }
}

class SignupBackupPasswordPage extends StatefulWidget {
  const SignupBackupPasswordPage({
    super.key,
    required this.config,
    required this.signupApi,
    required this.authInfo,
  });

  final Config config;
  final SignupApi signupApi;
  final GDriveAuthInfo authInfo;

  @override
  State<SignupBackupPasswordPage> createState() =>
      _SignupBackupPasswordPageState();
}

class _SignupBackupPasswordPageState extends State<SignupBackupPasswordPage> {
  final GlobalKey<FormFieldState<String>> passwordFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> confirmPasswordFieldKey = GlobalKey();

  final ValueNotifier<bool> isSigningUp = ValueNotifier(false);

  @override
  void dispose() {
    this.isSigningUp.dispose();
    super.dispose();
  }

  Result<String, String?> validatePassword(String? password) {
    if (password == null || password.isEmpty) {
      return const Err("");
    }

    // TODO(phlip9): this API should return a bare error enum and flutter should
    // convert that to a human-readable error message (for translations).
    final maybeErrMsg = api.formValidatePassword(password: password);
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

    final result = await this.widget.signupApi.signup(
          config: this.widget.config,
          googleAuthCode: this.widget.authInfo.authCode,
          password: password,
        );
    if (!this.mounted) return;

    this.isSigningUp.value = false;

    switch (result) {
      case Ok(:final ok):
        // ignore: use_build_context_synchronously
        unawaited(Navigator.of(this.context).maybePop(ok));
      case Err(:final err):
        error("Failed to signup: $err");
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
        leading: const LxBackButton(),
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
          const SizedBox(height: Space.s800),
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
                icon: const Icon(Icons.arrow_forward_rounded),
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
