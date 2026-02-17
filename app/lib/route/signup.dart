import 'dart:async';

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show Config, DeployEnv, GDriveSignupCredentials, RootSeed;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart'
    show MarkdownBody;
import 'package:lexeapp/cfg.dart' show lexeDocsUrl;
import 'package:lexeapp/clipboard.dart';
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
        SeedWordsCard,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/send/page.dart' show StackedButton;
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;
import 'package:lexeapp/url.dart' as url;
import 'package:lexeapp/validators.dart' as validators;

/// Require a signup code to complete signup.
const bool requireSignupCode = true;

/// A tiny interface so we can mock the [AppHandle.signup] call in design mode.
abstract interface class SignupApi {
  static const SignupApi prod = _ProdSignupApi._();

  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required RootSeed rootSeed,
    required GDriveSignupCredentials? gdriveSignupCreds,
    required String? signupCode,
    required String? partner,
  });
}

/// Collect all the context required for the Signup flow.
final class SignupCtx {
  const SignupCtx(this.config, this.rootSeed, this.gdriveAuth, this.signupApi);

  final Config config;
  final RootSeed rootSeed;
  final GDriveAuth gdriveAuth;
  final SignupApi signupApi;
}

class _ProdSignupApi implements SignupApi {
  const _ProdSignupApi._();

  @override
  Future<FfiResult<AppHandle>> signup({
    required Config config,
    required RootSeed rootSeed,
    required GDriveSignupCredentials? gdriveSignupCreds,
    required String? signupCode,
    required String? partner,
  }) => Result.tryFfiAsync(
    () => AppHandle.signup(
      config: config,
      rootSeed: rootSeed,
      gdriveSignupCreds: gdriveSignupCreds,
      signupCode: signupCode,
      partner: partner,
    ),
  );
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
          MarkdownBody(
            data: '''
# Enter your beta signup code

During Lexe's closed beta, a signup code is required to create a wallet.

We'll send you a signup code to your email. If you
would like to join the beta, add your email to the waitlist at:
[lexe.app](https://lexe.app)
''',
            // styleSheet: LxTheme.buildMarkdownStyle(),
            styleSheet: LxTheme.markdownStyle,
            onTapLink: (_, href, _) => unawaited(url.open(href!)),
          ),
          const SizedBox(height: Space.s300),

          // Signup code field
          TextFormField(
            key: this.signupCodeKey,
            autofocus: true,
            textInputAction: TextInputAction.done,
            validator: (str) => this.validateSignupCode(str).err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(hintText: "XXXX-XXXX"),
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
  const SignupGDriveAuthPage({
    super.key,
    required this.ctx,
    required this.signupCode,
  });

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
    if (flowResult == null || !this.mounted) return;

    info("SignupGDriveAuthPage: successful signup");

    // ignore: use_build_context_synchronously
    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  Future<void> onSeedOnlyPressed() async {
    info("SignupGDriveAuthPage: user tapped seed phrase-only backup");

    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(
        builder: (_) => SignupBackupSeedConfirmPage(
          ctx: this.widget.ctx,
          signupCode: this.widget.signupCode,
        ),
      ),
    );
    if (flowResult == null || !this.mounted) return;

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
              const SizedBox(height: Space.s300),
              LxFilledButton(
                label: const Center(child: Text("Seed phrase-only backup")),
                onTap: this.onSeedOnlyPressed,
              ),
            ],
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
    switch (validators.validatePassword(
      this.passwordFieldKey.currentState!.value,
    )) {
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
    final gdriveSignupCreds = GDriveSignupCredentials(
      backupPassword: password,
      googleAuthCode: this.widget.authInfo.serverAuthCode,
    );
    final result = await ctx.signupApi.signup(
      config: ctx.config,
      rootSeed: ctx.rootSeed,
      gdriveSignupCreds: gdriveSignupCreds,
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
          MarkdownBody(
            data: '''
# Enter your backup password

Enter at least 12 characters.

This password encrypts your recovery data so Google can't read it.
Store it in a safe place, like a password manager—you **need this to
recover your funds**.
''',
            // styleSheet: LxTheme.buildMarkdownStyle().copyWith(
            styleSheet: LxTheme.markdownStyle.copyWith(
              h1Padding: const EdgeInsets.only(
                top: Space.s200,
                bottom: Space.s200,
              ),
            ),
            // styleSheet: LxTheme.buildMarkdownStyle(),
            // styleSheet: LxTheme.markdownStyle,
          ),
          const SizedBox(height: Space.s100),

          // Password field
          TextFormField(
            key: this.passwordFieldKey,
            autofocus: true,
            textInputAction: TextInputAction.next,
            validator: (str) => validators.validatePassword(str).err,
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
          const SizedBox(height: Space.s100),

          // Confirm password field
          TextFormField(
            key: this.confirmPasswordFieldKey,
            autofocus: false,
            textInputAction: TextInputAction.done,
            validator: (str) => validators
                .validateConfirmPassword(
                  password: this.passwordFieldKey.currentState!.value,
                  confirmPassword: str,
                )
                .err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(
              hintText: "Confirm password",
            ),
            obscureText: true,
            style: textFieldStyle,
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
        bottomPadding: EdgeInsets.zero,
        bottom: Padding(
          padding: const EdgeInsets.symmetric(vertical: Space.s300),
          child: ValueListenableBuilder(
            valueListenable: this.isSigningUp,
            builder: (context, isSending, widget) => SignupButton(
              label: const Text("Sign up"),
              icon: const Icon(LxIcons.next),
              onTap: this.onSubmit,
              isLoading: isSending,
            ),
          ),
        ),
      ),
    );
  }
}

/// Ask the user to really confirm that they want a seed phrase-only backup.
///
/// This option provides weaker recoverability and unilateral exit guarantees
/// vs. active Google Drive backup. We still want to support users without a
/// Google Drive account though, so we give them this option.
///
/// Once we have VSS backup to a third party, the messaging can change since
/// all users will be OK.
class SignupBackupSeedConfirmPage extends StatelessWidget {
  const SignupBackupSeedConfirmPage({
    super.key,
    required this.ctx,
    required this.signupCode,
  });

  final SignupCtx ctx;
  final String? signupCode;

  Future<void> onConfirmPressed(BuildContext context) async {
    info(
      "SignupBackupSeedConfirmPage: user confirmed they want seed "
      "phrase-only backup",
    );

    final AppHandle? flowResult = await Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) =>
            SignupBackupSeedPage(ctx: this.ctx, signupCode: this.signupCode),
      ),
    );
    if (flowResult == null || !context.mounted) return;

    unawaited(Navigator.of(context).maybePop(flowResult));
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
          MarkdownBody(
            data:
                '''
# Only backup seed phrase?

A seed phrase-only backup allows you to restore your node if you lose your
phone, but relies on Lexe to provide your encrypted recovery data.

[Learn more]($lexeDocsUrl)
''',
            styleSheet: LxTheme.markdownStyle,
            onTapLink: (_, href, _) => unawaited(url.open(href!)),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: LxFilledButton.strong(
            label: const Text("Confirm"),
            icon: const Icon(LxIcons.next),
            onTap: () => this.onConfirmPressed(context),
          ),
        ),
      ),
    );
  }
}

/// Show the user their 24 word seed phrase. Require them to actively confirm
/// that they've backed it up before they can finish signup.
class SignupBackupSeedPage extends StatefulWidget {
  const SignupBackupSeedPage({
    super.key,
    required this.ctx,
    required this.signupCode,
  });

  final SignupCtx ctx;
  final String? signupCode;

  @override
  State<SignupBackupSeedPage> createState() => _SignupBackupSeedPageState();
}

class _SignupBackupSeedPageState extends State<SignupBackupSeedPage> {
  /// Whether the user has tapped the "switch" tile to confirm they've backed
  /// up their seed phrase.
  final ValueNotifier<bool> isConfirmed = ValueNotifier(false);

  /// Whether the signup request is in progress.
  final ValueNotifier<bool> isSigningUp = ValueNotifier(false);

  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  /// The 24 seed words to display.
  late final List<String> seedWords = widget.ctx.rootSeed.seedPhrase();

  @override
  void dispose() {
    this.errorMessage.dispose();
    this.isSigningUp.dispose();
    this.isConfirmed.dispose();
    super.dispose();
  }

  void onConfirm(bool value) {
    this.isConfirmed.value = value;
  }

  void onCopy() {
    final words = this.seedWords.join(" ");
    unawaited(LxClipboard.copyTextWithFeedback(this.context, words));
  }

  Future<void> onSubmit() async {
    if (this.isSigningUp.value) return;

    // Clear error message
    this.errorMessage.value = null;

    info("SignupBackupSeedPage: signing up with seed phrase-only backup");

    this.isSigningUp.value = true;
    try {
      await this.onSubmitInner();
    } finally {
      if (this.mounted) this.isSigningUp.value = false;
    }
  }

  Future<void> onSubmitInner() async {
    final ctx = this.widget.ctx;
    final result = await ctx.signupApi.signup(
      config: ctx.config,
      rootSeed: ctx.rootSeed,
      gdriveSignupCreds: null,
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
          const HeadingText(text: "Backup your seed phrase"),
          const SubheadingText(
            text: "Store this in a safe place, like a password manager.",
          ),
          const SizedBox(height: Space.s600),

          // 24-words seed phrase card
          Align(
            alignment: Alignment.center,
            child: SeedWordsCard(seedWords: this.seedWords),
          ),
          const SizedBox(height: Space.s500),

          // Confirm switch or error message
          //
          // Show the error message in place of the switch if there's an error,
          // since we don't have enough space otherwise. We also don't need the
          // switch if there's an error, since the user must have already
          // confirmed in order to attempt signing up.
          ValueListenableBuilder(
            valueListenable: this.errorMessage,
            builder: (_context, errorMessage, _widget) => ErrorMessageSection(
              errorMessage,
              // Require user to confirm
              other: ValueListenableBuilder(
                valueListenable: this.isConfirmed,
                builder: (context, isConfirmed, child) {
                  return ValueListenableBuilder(
                    valueListenable: this.isSigningUp,
                    builder: (context, isSigningUp, child) {
                      return SwitchListTile(
                        value: isConfirmed,
                        // Disable switch while signing up
                        onChanged: (!isSigningUp) ? this.onConfirm : null,
                        title: const Text(
                          "I have backed up my seed phrase. I understand my funds cannot be recovered if I lose the seed phrase.",
                          style: TextStyle(
                            fontSize: Fonts.size200,
                            height: 1.4,
                          ),
                        ),
                        contentPadding: EdgeInsets.zero,
                        inactiveTrackColor: LxColors.grey1000,
                        activeTrackColor: LxColors.moneyGoUp,
                        inactiveThumbColor: LxColors.grey850,
                        controlAffinity: ListTileControlAffinity.leading,
                      );
                    },
                  );
                },
              ),
            ),
          ),
        ],
        // Bottom buttons (copy, sign up ->)
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s300, bottom: Space.s200),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              Row(
                children: [
                  // Copy
                  Expanded(
                    child: GestureDetector(
                      behavior: HitTestBehavior.translucent,
                      onTap: this.onCopy,
                      child: StackedButton(
                        button: LxFilledButton(
                          onTap: this.onCopy,
                          icon: const Center(child: Icon(LxIcons.copy)),
                        ),
                        label: "Copy",
                      ),
                    ),
                  ),
                  const SizedBox(width: Space.s200),
                  // Sign up ->
                  Expanded(
                    child: ValueListenableBuilder(
                      valueListenable: this.isConfirmed,
                      builder: (_context, isConfirmed, _widget) =>
                          ValueListenableBuilder(
                            valueListenable: this.isSigningUp,
                            builder: (context, isSigningUp, child) {
                              final isEnabled = isConfirmed && !isSigningUp;

                              return GestureDetector(
                                behavior: HitTestBehavior.translucent,
                                onTap: isEnabled ? this.onSubmit : null,
                                child: StackedButton(
                                  button: SignupButton(
                                    label: const Icon(LxIcons.next),
                                    icon: const Center(),
                                    onTap: isEnabled ? this.onSubmit : null,
                                    isLoading: isSigningUp,
                                  ),
                                  label: "Sign up",
                                ),
                              );
                            },
                          ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class SignupButton extends StatelessWidget {
  const SignupButton({
    super.key,
    required this.label,
    required this.icon,
    required this.onTap,
    required this.isLoading,
  });

  final Widget label;
  final Widget icon;
  final VoidCallback? onTap;
  final bool isLoading;

  @override
  Widget build(BuildContext context) {
    return AnimatedFillButton(
      label: this.label,
      icon: this.icon,
      onTap: this.onTap,
      loading: this.isLoading,
      style: FilledButton.styleFrom(
        backgroundColor: LxColors.moneyGoUp,
        foregroundColor: LxColors.grey1000,
        iconColor: LxColors.grey1000,
      ),
    );
  }
}
