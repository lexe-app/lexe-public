/// The wallet restore UI flow.
library;

import 'dart:async' show unawaited;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/gdrive.dart'
    show GDriveClient, GDriveRestoreCandidate;
import 'package:app_rs_dart/ffi/types.dart' show Config;
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart' show MarkdownBody;
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        HeadingText,
        LxBackButton,
        MultistepFlow,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/send/page.dart'
    show ErrorMessage, ErrorMessageSection;
import 'package:lexeapp/style.dart' show LxColors, LxIcons, LxTheme, Space;

/// The entry point into the gdrive wallet restore UI flow.
class RestorePage extends StatelessWidget {
  const RestorePage(
      {super.key, required this.config, required this.gdriveAuth});

  final Config config;
  final GDriveAuth gdriveAuth;

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
        builder: (_) => RestoreGDriveAuthPage(
            config: this.config, gdriveAuth: this.gdriveAuth),
      );
}

/// First we need the user to authorize the app's access to their GDrive in
/// order to locate their wallet backups.
class RestoreGDriveAuthPage extends StatefulWidget {
  const RestoreGDriveAuthPage(
      {super.key, required this.config, required this.gdriveAuth});

  final Config config;
  final GDriveAuth gdriveAuth;

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
      this.isRestoring.value = false;
    }
  }

  Future<void> onAuthPressedInner() async {
    // Hide error message
    this.errorMessage.value = null;

    // Ask user to auth with Google Drive.
    final authResult = await this.widget.gdriveAuth.tryAuth();
    if (!this.mounted) return;

    final GDriveClient gdriveClient;
    switch (authResult) {
      case Ok(:final ok):
        // user canceled. they might want to try again, so don't pop yet.
        if (ok == null) return;
        gdriveClient = ok;
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
    info("restore: found candidates: $candidatesDbg");

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
    final bool? flowResult = await Navigator.of(this.context).push(
      MaterialPageRoute(builder: (_) {
        // (normal case): Only one backup, open the password prompt page directly.
        if (candidates.length == 1) {
          // TODO(phlip9): impl
          throw UnimplementedError();
        } else {
          // It's possible (esp. for Lexe devs) to have multiple wallets for a single
          // gdrive account. Open a page to ask the user which UserPk they want to
          // restore.
          //
          // TODO(phlip9): UserPk isn't really a user-facing ID, so asking people to
          // choose by UserPk is definitely suboptimal. Figure out some kind of wallet
          // nickname system or something.

          return RestoreChooseWalletPage(candidates: candidates);
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
# Restore Wallet from Google Drive backup
''',
            styleSheet: LxTheme.markdownStyle,
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
  const RestoreChooseWalletPage({super.key, required this.candidates});

  final List<GDriveRestoreCandidate> candidates;

  @override
  State<RestoreChooseWalletPage> createState() =>
      _RestoreChooseWalletPageState();
}

class _RestoreChooseWalletPageState extends State<RestoreChooseWalletPage> {
  Future<void> selectCandidate(final GDriveRestoreCandidate candidate) async {
    info("restore: chose candidate UserPk: ${candidate.userPk()}");
    // TODO(phlip9): impl
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
          const HeadingText(text: "Choose wallet to restore"),
          const SubheadingText(text: "Listed by UserPk"),
          const SizedBox(height: Space.s600),
          Column(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.start,
            children: this
                .widget
                .candidates
                .map(
                  // TODO(phlip9): add "created at: XXX" subtitle
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
