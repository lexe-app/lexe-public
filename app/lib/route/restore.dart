/// The wallet restore UI flow.
library;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:flutter/material.dart';
import 'package:flutter_markdown/flutter_markdown.dart' show MarkdownBody;
import 'package:lexeapp/components.dart'
    show LxBackButton, LxFilledButton, MultistepFlow, ScrollableSinglePageBody;
import 'package:lexeapp/gdrive_auth.dart' show GDriveAuth, GDriveAuthInfo;
import 'package:lexeapp/logger.dart';
import 'package:lexeapp/result.dart';
import 'package:lexeapp/route/send/page.dart'
    show ErrorMessage, ErrorMessageSection;
import 'package:lexeapp/style.dart' show LxIcons, LxTheme, Space;

class RestorePage extends StatelessWidget {
  const RestorePage({super.key, required this.gdriveAuth});

  final GDriveAuth gdriveAuth;

  @override
  Widget build(BuildContext context) => MultistepFlow<AppHandle?>(
        builder: (_) => RestoreGDriveAuthPage(gdriveAuth: this.gdriveAuth),
      );
}

class RestoreGDriveAuthPage extends StatefulWidget {
  const RestoreGDriveAuthPage({super.key, required this.gdriveAuth});

  final GDriveAuth gdriveAuth;

  @override
  State<RestoreGDriveAuthPage> createState() =>
      _RestoreGDriveAuthPageStateState();
}

class _RestoreGDriveAuthPageStateState extends State<RestoreGDriveAuthPage> {
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  @override
  void dispose() {
    this.errorMessage.dispose();
    super.dispose();
  }

  Future<void> onAuthPressed() async {
    // Hide error message
    this.errorMessage.value = null;

    final result = await this.widget.gdriveAuth.tryAuth();
    if (!this.mounted) return;

    final GDriveAuthInfo authInfo;
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

    info("authInfo: $authInfo");

    // // ignore: use_build_context_synchronously
    // final AppHandle? flowResult = await Navigator.of(this.context).push(
    //   MaterialPageRoute(
    //     builder: (_) => SignupBackupPasswordPage(
    //       config: this.widget.config,
    //       signupApi: this.widget.signupApi,
    //       authInfo: authInfo,
    //     ),
    //   ),
    // );
    // if (flowResult == null) return;
    // if (!this.mounted) return;
    //
    // info("SignupGDriveAuthPage: successful signup");
    //
    // // ignore: use_build_context_synchronously
    // unawaited(Navigator.of(this.context).maybePop(flowResult));
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
        bottom: LxFilledButton.strong(
          label: const Text("Connect Google Drive"),
          icon: const Icon(LxIcons.next),
          onTap: this.onAuthPressed,
        ),
      ),
    );
  }
}
