import 'package:flutter/material.dart';

import '../bindings.dart' show api;
import '../bindings_generated_api.dart' show AppHandle, Config;
import '../components.dart'
    show
        HeadingText,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxFilledButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        baseInputDecoration;
import '../gdrive_auth.dart' show GDriveAuthInfo, tryGDriveAuth;
import '../logger.dart' show dbg, error, info;
import '../result.dart' show Err, Ok, Result;
import '../style.dart' show Fonts, Space;

/// The entry point for the signup flow.
class SignupPage extends StatelessWidget {
  const SignupPage({
    super.key,
    required this.config,
  });

  final Config config;

  @override
  Widget build(BuildContext context) =>
      MultistepFlow(builder: (_) => const SignupGDriveAuthPage());
}

/// This page has a button to ask for the user's consent for GDrive permissions.
class SignupGDriveAuthPage extends StatefulWidget {
  const SignupGDriveAuthPage({super.key});

  @override
  State<StatefulWidget> createState() => _SignupGDriveAuthPageState();
}

class _SignupGDriveAuthPageState extends State<SignupGDriveAuthPage> {
  Future<void> onAuthPressed() async {
    final GDriveAuthInfo authInfo;
    try {
      final maybeAuthInfo = await tryGDriveAuth();
      if (!this.mounted) return;

      // user canceled. they might want to try again, so don't pop yet.
      if (maybeAuthInfo == null) return;
      authInfo = maybeAuthInfo;
    } on Exception catch (err) {
      error("Failed to auth user with GDrive: $err");
      return;
    }

    // TODO(phlip9): pass auth info to flow
    dbg(authInfo);

    // ignore: use_build_context_synchronously
    final AppHandle? flowResult = await Navigator.of(this.context).push(
        MaterialPageRoute(builder: (_) => const SignupBackupPasswordPage()));
    if (!this.mounted) return;

    if (flowResult != null) {
      // ignore: use_build_context_synchronously
      await Navigator.of(this.context).maybePop(flowResult);
    }
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
  const SignupBackupPasswordPage({super.key});

  @override
  State<SignupBackupPasswordPage> createState() =>
      _SignupBackupPasswordPageState();
}

class _SignupBackupPasswordPageState extends State<SignupBackupPasswordPage> {
  final GlobalKey<FormFieldState<String>> passwordFieldKey = GlobalKey();
  final GlobalKey<FormFieldState<String>> confirmPasswordFieldKey = GlobalKey();

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
    final fieldState = this.confirmPasswordFieldKey.currentState!;
    if (!fieldState.validate()) {
      return;
    }

    final String password;
    switch (this.validatePassword(fieldState.value!)) {
      case Ok(:final ok):
        password = ok;
      case Err():
        return;
    }

    info("signing up: '$password'");
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
        bottom: LxFilledButton(
          label: const Text("Sign up"),
          icon: const Icon(Icons.arrow_forward_rounded),
          onTap: () {},
        ),
      ),
    );
  }
}
