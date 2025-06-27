/// The wallet restore UI flow.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/gdrive.dart'
    show GDriveClient, GDriveRestoreCandidate;
import 'package:app_rs_dart/ffi/types.dart' show Config, RootSeed;
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
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText,
        baseInputDecoration;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveServerAuthCode;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxTheme, Space;

/// A tiny interface so we can mock the [AppHandle.restore] call in design mode.
abstract interface class RestoreApi {
  static const RestoreApi prod = _ProdRestoreApi._();

  Future<FfiResult<AppHandle>> restore({
    required Config config,
    required String googleAuthCode,
    required RootSeed rootSeed,
  });
}

class _ProdRestoreApi implements RestoreApi {
  const _ProdRestoreApi._();

  @override
  Future<FfiResult<AppHandle>> restore({
    required Config config,
    required String googleAuthCode,
    required RootSeed rootSeed,
  }) =>
      Result.tryFfiAsync(() => AppHandle.restore(
            config: config,
            googleAuthCode: googleAuthCode,
            rootSeed: rootSeed,
          ));
}

/// The entry point into the gdrive wallet restore UI flow.
class RestorePage extends StatelessWidget {
  const RestorePage(
      {super.key,
      required this.config,
      required this.gdriveAuth,
      required this.restoreApi});

  final Config config;
  final GDriveAuth gdriveAuth;
  final RestoreApi restoreApi;

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
        builder: (_) => RestoreGDriveAuthPage(
          config: this.config,
          gdriveAuth: this.gdriveAuth,
          restoreApi: this.restoreApi,
        ),
      );
}

/// First we need the user to authorize the app's access to their GDrive in
/// order to locate their wallet backups.
class RestoreGDriveAuthPage extends StatefulWidget {
  const RestoreGDriveAuthPage(
      {super.key,
      required this.config,
      required this.gdriveAuth,
      required this.restoreApi});

  final Config config;
  final GDriveAuth gdriveAuth;
  final RestoreApi restoreApi;

  @override
  State<RestoreGDriveAuthPage> createState() =>
      _RestoreGDriveAuthPageStateState();
}

class _RestoreGDriveAuthPageStateState extends State<RestoreGDriveAuthPage> {
  final ValueNotifier<bool> isRestoring = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isRestoring.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onAuthPressed() async {
    if (this.isRestoring.value) return;

    this.isRestoring.value = true;
    try {
      await this.onAuthPressedInner();
    } finally {
      if (this.mounted) this.isRestoring.value = false;
    }
  }

  Future<void> onAuthPressedInner() async {
    // Hide error message
    this.errorMessage.value = null;

    // Ask user to auth with Google Drive.
    final Result<(GDriveClient, GDriveServerAuthCode)?, Exception> authResult =
        (await this.widget.gdriveAuth.tryAuth()).andThen((client) {
      if (client == null) return const Ok(null);
      final serverAuthCode = client.serverCode();
      if (serverAuthCode == null) {
        return Err(Exception("GDrive auth didn't return a server auth code"));
      }

      return Ok((
        client,
        GDriveServerAuthCode(serverAuthCode: serverAuthCode),
      ));
    });
    if (!this.mounted) return;

    final GDriveServerAuthCode serverAuthCode;
    final GDriveClient gdriveClient;
    switch (authResult) {
      case Ok(:final ok):
        // user canceled. they might want to try again, so don't pop yet.
        if (ok == null) return;
        (gdriveClient, serverAuthCode) = ok;
      case Err(:final err):
        final errStr = err.toString();
        error("restore: Failed to auth user with GDrive: $errStr");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error connecting your Google Drive",
          message: errStr,
        );
        return;
    }

    info("restore: authed with gdrive");

    // Try to locate any google drive backups for the current env.
    final config = this.widget.config;
    final findResult = await Result.tryFfiAsync(
      () => gdriveClient.intoRestoreClient().findRestoreCandidates(
            deployEnv: config.deployEnv,
            network: config.network,
            useSgx: config.useSgx,
          ),
    );
    if (!this.mounted) return;

    final List<GDriveRestoreCandidate> candidates;
    switch (findResult) {
      case Ok(:final ok):
        candidates = ok;
      case Err(:final err):
        error("restore: ${err.message}");
        this.errorMessage.value = ErrorMessage(
          title: "There was an error locating your Google Drive backup",
          message: err.message,
        );
        return;
    }

    final candidatesDbg =
        candidates.map((x) => x.userPk()).toList(growable: false);
    info("restore: found candidate UserPks: $candidatesDbg");

    // We authed, but there were no backups :(
    if (candidates.isEmpty) {
      warn("No backups in Google Drive");
      this.errorMessage.value = const ErrorMessage(
        title: "We couldn't find any Lexe Wallet backups for this account",
      );
      return;
    }

    // Either (1) goto password prompt if only one candidate, or (2) ask user to
    // choose which wallet first.
    final AppHandle? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(builder: (_) {
        // (normal case): Only one backup, open the password prompt page directly.
        if (candidates.length == 1) {
          return RestorePasswordPage(
            config: this.widget.config,
            restoreApi: this.widget.restoreApi,
            serverAuthCode: serverAuthCode,
            candidate: candidates.single,
          );
        } else {
          // It's possible (esp. for Lexe devs) to have multiple wallets for a single
          // gdrive account. Open a page to ask the user which UserPk they want to
          // restore.
          //
          // TODO(phlip9): UserPk isn't really a user-facing ID, so asking people to
          // choose by UserPk is definitely suboptimal. Figure out some kind of wallet
          // nickname system or something.
          return RestoreChooseWalletPage(
            config: this.widget.config,
            restoreApi: this.widget.restoreApi,
            serverAuthCode: serverAuthCode,
            candidates: candidates,
          );
        }
      }),
    );
    if (flowResult == null) return;
    if (!this.mounted) return;

    info("restore: successful restore");

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
# Restore wallet from Google Drive

Already have a Lexe Wallet? Connect your Google Drive to restore from an existing Lexe Wallet backup.

- **Your wallet backup is encrypted**. You'll need your backup password in a moment.
- Lexe cannot access any files in your Drive.
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
            valueListenable: this.isRestoring,
            builder: (context, isRestoring, widget) => AnimatedFillButton(
              onTap: this.onAuthPressed,
              loading: isRestoring,
              label: const Text("Connect Google Drive"),
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

/// In rare cases, the user might have multiple wallets backed up in this gdrive
/// account. Ask them to choose one.
class RestoreChooseWalletPage extends StatefulWidget {
  const RestoreChooseWalletPage({
    super.key,
    required this.candidates,
    required this.serverAuthCode,
    required this.config,
    required this.restoreApi,
  });

  final Config config;
  final RestoreApi restoreApi;
  final GDriveServerAuthCode serverAuthCode;
  final List<GDriveRestoreCandidate> candidates;

  @override
  State<RestoreChooseWalletPage> createState() =>
      _RestoreChooseWalletPageState();
}

class _RestoreChooseWalletPageState extends State<RestoreChooseWalletPage> {
  Future<void> selectCandidate(final GDriveRestoreCandidate candidate) async {
    info("restore: chose UserPk: ${candidate.userPk()}");

    // Goto password prompt.
    final AppHandle? flowResult =
        await Navigator.of(this.context).push(MaterialPageRoute(
            builder: (_) => RestorePasswordPage(
                  config: this.widget.config,
                  serverAuthCode: this.widget.serverAuthCode,
                  candidate: candidate,
                  restoreApi: this.widget.restoreApi,
                )));
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
          const HeadingText(text: "Choose wallet to restore"),
          const SubheadingText(text: "Wallets are listed by User public key"),
          const SizedBox(height: Space.s600),

          // List candidates (by UserPk)
          Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.start,
            children: this
                .widget
                .candidates
                .map(
                  // TODO(phlip9): add "created on XXX" subtitle to help differentiate?
                  (candidate) => ListTile(
                    title: Text(candidate.userPk()),
                    trailing: const Icon(LxIcons.nextSecondary),
                    onTap: () => this.selectCandidate(candidate),
                    contentPadding: EdgeInsets.zero,
                  ),
                )
                .toList(),
          ),
        ],
      ),
    );
  }
}

/// Ask the user for their backup password to decrypt the root seed backup.
class RestorePasswordPage extends StatefulWidget {
  const RestorePasswordPage({
    super.key,
    required this.candidate,
    required this.serverAuthCode,
    required this.config,
    required this.restoreApi,
  });

  final Config config;
  final RestoreApi restoreApi;
  final GDriveServerAuthCode serverAuthCode;
  final GDriveRestoreCandidate candidate;

  @override
  State<RestorePasswordPage> createState() => _RestorePasswordPageState();
}

class _RestorePasswordPageState extends State<RestorePasswordPage> {
  final GlobalKey<FormFieldState<String>> passwordFieldKey = GlobalKey();
  final ValueNotifier<bool> isRestoring = ValueNotifier(false);
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.isRestoring.dispose();
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onSubmit() async {
    if (this.isRestoring.value) return;

    // Hide error message
    this.errorMessage.value = null;

    final fieldState = this.passwordFieldKey.currentState!;
    final password = fieldState.value;
    if (password == null || !fieldState.validate()) return;

    final RootSeed rootSeed;
    switch (this.tryDecrypt(fieldState.value)) {
      case Ok(:final ok):
        rootSeed = ok;
      case Err():
        return;
    }

    this.isRestoring.value = true;
    try {
      await this.onSubmitInner(rootSeed);
    } finally {
      if (this.mounted) this.isRestoring.value = false;
    }
  }

  Future<void> onSubmitInner(RootSeed rootSeed) async {
    info("restore: recovered root seed");

    final restoreApi = this.widget.restoreApi;
    final config = this.widget.config;
    final serverAuthCode = this.widget.serverAuthCode.serverAuthCode;

    final result = await restoreApi.restore(
      config: config,
      googleAuthCode: serverAuthCode,
      rootSeed: rootSeed,
    );
    if (!this.mounted) return;

    final AppHandle flowResult;
    switch (result) {
      case Ok(:final ok):
        flowResult = ok;
      case Err(:final err):
        error("restore: AppHandle.restore failed: $err");
        this.errorMessage.value = ErrorMessage(
          title: "Error restoring wallet",
          message: err.message,
        );
        return;
    }

    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  Result<RootSeed, String> tryDecrypt(final String? password) {
    if (password == null || password.isEmpty) return const Err("");
    return Result.tryFfi(
      () => this.widget.candidate.tryDecrypt(password: password),
    ).mapErr((err) {
      warn("restore: decrypt: ${err.message}");
      return "Invalid password";
    });
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
          const HeadingText(text: "Enter wallet backup password"),
          const SizedBox(height: Space.s200),
          const SubheadingText(
              text: "This password was set when the wallet was first created"),
          const SizedBox(height: Space.s600),

          // Password field
          TextFormField(
            key: this.passwordFieldKey,
            autofocus: true,
            textInputAction: TextInputAction.done,
            validator: (password) => this.tryDecrypt(password).err,
            onEditingComplete: this.onSubmit,
            decoration: baseInputDecoration.copyWith(hintText: "Password"),
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
        // Restore ->
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.isRestoring,
            builder: (context, isRestoring, widget) => AnimatedFillButton(
              onTap: this.onSubmit,
              loading: isRestoring,
              label: const Text("Restore"),
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
